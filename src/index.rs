use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, ErrorCode, Transaction};
use walkdir::{DirEntry, WalkDir};

use crate::config::SourceConfig;
use crate::scanner::is_image_file;
use crate::search::{
    extract_product_code, normalize_code, normalize_path_key, validate_extraction_rule,
};
use crate::similarity::{PerceptualHash, PHASH_VERSION};

const DATABASE_SCHEMA_VERSION: i64 = 2;
const PRODUCT_BATCH_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexState {
    Disabled,
    #[default]
    Idle,
    Indexing,
    Paused,
    Failed,
}

impl IndexState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Idle => "Ready",
            Self::Indexing => "Indexing",
            Self::Paused => "Paused",
            Self::Failed => "Needs attention",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceIndexStatus {
    pub name: String,
    pub path: PathBuf,
    pub available: bool,
    pub products_indexed: usize,
    pub images_indexed: usize,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct IndexSnapshot {
    pub state: IndexState,
    pub current_source: Option<String>,
    pub folders_scanned: usize,
    pub products_indexed: usize,
    pub images_indexed: usize,
    pub sources: HashMap<String, SourceIndexStatus>,
    pub last_error: Option<String>,
    pub last_completed: Option<Instant>,
    pub recovered_corrupt_database: bool,
}

#[derive(Clone, Debug)]
pub enum IndexEvent {
    SourceAvailability {
        job_id: u64,
        source: SourceConfig,
        available: bool,
        error: Option<String>,
    },
    Progress {
        job_id: u64,
        source_path: String,
        folders_scanned: usize,
        products_indexed: usize,
        images_indexed: usize,
    },
    SourceCompleted {
        job_id: u64,
        source_path: String,
        products_indexed: usize,
        images_indexed: usize,
    },
    Completed {
        job_id: u64,
        cancelled: bool,
    },
    Failed {
        job_id: u64,
        error: String,
    },
}

struct IndexJob {
    id: u64,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

pub struct IndexManager {
    database_path: PathBuf,
    event_tx: mpsc::Sender<IndexEvent>,
    event_rx: mpsc::Receiver<IndexEvent>,
    active_job: Option<IndexJob>,
    next_job_id: u64,
    snapshot: IndexSnapshot,
}

impl IndexManager {
    pub fn new(database_path: PathBuf, enabled: bool) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let mut snapshot = IndexSnapshot {
            state: if enabled {
                IndexState::Idle
            } else {
                IndexState::Disabled
            },
            ..IndexSnapshot::default()
        };
        if enabled {
            match initialize_database(&database_path) {
                Ok(recovered) => snapshot.recovered_corrupt_database = recovered,
                Err(error) => {
                    snapshot.state = IndexState::Failed;
                    snapshot.last_error = Some(error);
                }
            }
        }

        Self {
            database_path,
            event_tx,
            event_rx,
            active_job: None,
            next_job_id: 1,
            snapshot,
        }
    }

    pub fn snapshot(&self) -> &IndexSnapshot {
        &self.snapshot
    }

    pub fn is_active(&self) -> bool {
        self.active_job.is_some()
    }

    pub fn source_status(&self, path: &str) -> Option<&SourceIndexStatus> {
        self.snapshot
            .sources
            .get(&normalize_path_key(Path::new(path)))
    }

    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if enabled {
            let recovered = initialize_database(&self.database_path)?;
            self.snapshot.recovered_corrupt_database |= recovered;
            if self.active_job.is_none() {
                self.snapshot.state = IndexState::Idle;
            }
        } else {
            self.cancel();
            self.snapshot.state = IndexState::Disabled;
        }
        Ok(())
    }

    pub fn start_reconcile(
        &mut self,
        sources: Vec<SourceConfig>,
        rebuild: bool,
    ) -> Result<(), String> {
        if self.snapshot.state == IndexState::Disabled {
            return Err("Indexing is disabled in Settings.".into());
        }
        if self.active_job.is_some() {
            return Err("An index job is already running.".into());
        }

        initialize_database(&self.database_path)?;
        let job_id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let paused = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let job = IndexJob {
            id: job_id,
            paused: Arc::clone(&paused),
            cancelled: Arc::clone(&cancelled),
        };
        let database_path = self.database_path.clone();
        let event_tx = self.event_tx.clone();

        thread::Builder::new()
            .name(format!("luxepix-index-{job_id}"))
            .spawn(move || {
                if let Err(error) = run_index_job(
                    job_id,
                    &database_path,
                    &sources,
                    rebuild,
                    &paused,
                    &cancelled,
                    &event_tx,
                ) {
                    let _ = event_tx.send(IndexEvent::Failed { job_id, error });
                }
            })
            .map_err(|error| error.to_string())?;

        self.active_job = Some(job);
        self.snapshot.state = IndexState::Indexing;
        self.snapshot.current_source = None;
        self.snapshot.folders_scanned = 0;
        self.snapshot.products_indexed = 0;
        self.snapshot.images_indexed = 0;
        self.snapshot.last_error = None;
        Ok(())
    }

    pub fn pause(&mut self) {
        if let Some(job) = &self.active_job {
            job.paused.store(true, Ordering::Release);
            self.snapshot.state = IndexState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if let Some(job) = &self.active_job {
            job.paused.store(false, Ordering::Release);
            self.snapshot.state = IndexState::Indexing;
        }
    }

    pub fn cancel(&mut self) {
        if let Some(job) = &self.active_job {
            job.cancelled.store(true, Ordering::Release);
            job.paused.store(false, Ordering::Release);
        }
    }

    pub fn reconcile_if_due(&mut self, sources: &[SourceConfig]) -> Result<bool, String> {
        if self.snapshot.state == IndexState::Disabled || self.active_job.is_some() {
            return Ok(false);
        }
        let interval = sources
            .iter()
            .map(|source| source.reconciliation_minutes.clamp(1, 1_440))
            .min()
            .unwrap_or(5);
        let due = self
            .snapshot
            .last_completed
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(interval * 60));
        if due {
            self.start_reconcile(sources.to_vec(), false)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn drain(&mut self, limit: usize) -> Vec<IndexEvent> {
        let mut events = Vec::new();
        for _ in 0..limit {
            let Ok(event) = self.event_rx.try_recv() else {
                break;
            };
            self.apply_event(&event);
            events.push(event);
        }
        events
    }

    fn apply_event(&mut self, event: &IndexEvent) {
        let job_id = match event {
            IndexEvent::SourceAvailability { job_id, .. }
            | IndexEvent::Progress { job_id, .. }
            | IndexEvent::SourceCompleted { job_id, .. }
            | IndexEvent::Completed { job_id, .. }
            | IndexEvent::Failed { job_id, .. } => *job_id,
        };
        if self.active_job.as_ref().map(|job| job.id) != Some(job_id) {
            return;
        }

        match event {
            IndexEvent::SourceAvailability {
                source,
                available,
                error,
                ..
            } => {
                self.snapshot.current_source = Some(source.name.clone());
                self.snapshot.sources.insert(
                    normalize_path_key(Path::new(&source.path)),
                    SourceIndexStatus {
                        name: source.name.clone(),
                        path: PathBuf::from(&source.path),
                        available: *available,
                        products_indexed: 0,
                        images_indexed: 0,
                        last_error: error.clone(),
                    },
                );
            }
            IndexEvent::Progress {
                source_path,
                folders_scanned,
                products_indexed,
                images_indexed,
                ..
            } => {
                self.snapshot.folders_scanned = *folders_scanned;
                self.snapshot.products_indexed = *products_indexed;
                self.snapshot.images_indexed = *images_indexed;
                if let Some(source) = self
                    .snapshot
                    .sources
                    .get_mut(&normalize_path_key(Path::new(source_path)))
                {
                    source.products_indexed = *products_indexed;
                    source.images_indexed = *images_indexed;
                }
            }
            IndexEvent::SourceCompleted {
                source_path,
                products_indexed,
                images_indexed,
                ..
            } => {
                if let Some(source) = self
                    .snapshot
                    .sources
                    .get_mut(&normalize_path_key(Path::new(source_path)))
                {
                    source.products_indexed = *products_indexed;
                    source.images_indexed = *images_indexed;
                }
            }
            IndexEvent::Completed { cancelled, .. } => {
                let disabled = self.snapshot.state == IndexState::Disabled;
                self.active_job = None;
                self.snapshot.current_source = None;
                self.snapshot.state = if disabled {
                    IndexState::Disabled
                } else {
                    IndexState::Idle
                };
                if !cancelled && !disabled {
                    self.snapshot.last_completed = Some(Instant::now());
                }
            }
            IndexEvent::Failed { error, .. } => {
                let disabled = self.snapshot.state == IndexState::Disabled;
                self.active_job = None;
                self.snapshot.current_source = None;
                if !disabled {
                    self.snapshot.state = IndexState::Failed;
                    self.snapshot.last_error = Some(error.clone());
                }
            }
        }
    }
}

impl Drop for IndexManager {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug)]
struct ProductRecord {
    folder_path: PathBuf,
    folder_name: String,
    product_code: String,
    normalized_code: String,
    normalized_folder_path: String,
    preview_path: Option<PathBuf>,
    image_count: usize,
    total_files: usize,
    size_bytes: u64,
    modified_unix: u64,
    images: Vec<ImageRecord>,
}

#[derive(Debug)]
struct ImageRecord {
    path: PathBuf,
    normalized_path: String,
    size_bytes: u64,
    modified_unix: u64,
    is_preview: bool,
    perceptual_hash: Option<String>,
}

fn run_index_job(
    job_id: u64,
    database_path: &Path,
    sources: &[SourceConfig],
    _rebuild: bool,
    paused: &AtomicBool,
    cancelled: &AtomicBool,
    event_tx: &mpsc::Sender<IndexEvent>,
) -> Result<(), String> {
    let mut connection = open_database(database_path).map_err(|error| error.to_string())?;

    remove_unconfigured_sources(&connection, sources).map_err(|error| error.to_string())?;
    let generation = generation_token();
    let mut total_folders = 0usize;
    let mut total_products = 0usize;
    let mut total_images = 0usize;

    for source in sources {
        wait_if_paused(paused, cancelled);
        if cancelled.load(Ordering::Acquire) {
            let _ = event_tx.send(IndexEvent::Completed {
                job_id,
                cancelled: true,
            });
            return Ok(());
        }

        let source_path = PathBuf::from(&source.path);
        let rule_error = validate_extraction_rule(source).err();
        let availability_error = if !source_path.is_dir() {
            Some("Source folder is not currently accessible.".to_string())
        } else {
            rule_error
        };
        let available = availability_error.is_none();
        let source_id = upsert_source(
            &connection,
            source,
            available,
            generation,
            availability_error.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        let _ = event_tx.send(IndexEvent::SourceAvailability {
            job_id,
            source: source.clone(),
            available,
            error: availability_error.clone(),
        });
        if !available {
            continue;
        }

        let mut batch = Vec::with_capacity(PRODUCT_BATCH_SIZE);
        let mut source_products = 0usize;
        let mut source_images = 0usize;
        let walker = WalkDir::new(&source_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(should_walk);

        for entry in walker {
            wait_if_paused(paused, cancelled);
            if cancelled.load(Ordering::Acquire) {
                let _ = event_tx.send(IndexEvent::Completed {
                    job_id,
                    cancelled: true,
                });
                return Ok(());
            }
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_dir() {
                continue;
            }
            total_folders += 1;
            if let Some(product) = inspect_product_folder(entry.path(), source) {
                source_images += product.image_count;
                source_products += 1;
                batch.push(product);
            }

            if batch.len() >= PRODUCT_BATCH_SIZE {
                write_product_batch(&mut connection, source_id, generation, &batch)
                    .map_err(|error| error.to_string())?;
                batch.clear();
            }
            if total_folders.is_multiple_of(25) {
                let _ = event_tx.send(IndexEvent::Progress {
                    job_id,
                    source_path: source.path.clone(),
                    folders_scanned: total_folders,
                    products_indexed: total_products + source_products,
                    images_indexed: total_images + source_images,
                });
            }
        }
        if !batch.is_empty() {
            write_product_batch(&mut connection, source_id, generation, &batch)
                .map_err(|error| error.to_string())?;
        }
        connection
            .execute(
                "DELETE FROM products WHERE source_id = ?1 AND generation <> ?2",
                params![source_id, generation],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE sources
                 SET last_indexed_unix = ?2, generation = ?3, last_error = NULL, available = 1
                 WHERE id = ?1",
                params![source_id, unix_now(), generation],
            )
            .map_err(|error| error.to_string())?;

        total_products += source_products;
        total_images += source_images;
        let _ = event_tx.send(IndexEvent::SourceCompleted {
            job_id,
            source_path: source.path.clone(),
            products_indexed: source_products,
            images_indexed: source_images,
        });
    }

    let _ = event_tx.send(IndexEvent::Progress {
        job_id,
        source_path: String::new(),
        folders_scanned: total_folders,
        products_indexed: total_products,
        images_indexed: total_images,
    });
    let _ = event_tx.send(IndexEvent::Completed {
        job_id,
        cancelled: false,
    });
    Ok(())
}

fn wait_if_paused(paused: &AtomicBool, cancelled: &AtomicBool) {
    while paused.load(Ordering::Acquire) && !cancelled.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(60));
    }
}

fn should_walk(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    !name.starts_with('.') && !entry.file_type().is_symlink()
}

fn inspect_product_folder(path: &Path, source: &SourceConfig) -> Option<ProductRecord> {
    let entries = fs::read_dir(path).ok()?;
    let mut images = Vec::new();
    let mut total_files = 0usize;
    let mut size_bytes = 0u64;
    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        total_files += 1;
        let file_path = entry.path();
        let metadata = entry.metadata().ok();
        let file_size = metadata.as_ref().map(fs::Metadata::len).unwrap_or(0);
        size_bytes = size_bytes.saturating_add(file_size);
        if is_image_file(&file_path) {
            let perceptual_hash = PerceptualHash::from_path(&file_path)
                .ok()
                .map(PerceptualHash::to_database);
            images.push(ImageRecord {
                normalized_path: normalize_path_key(&file_path),
                path: file_path,
                size_bytes: file_size,
                modified_unix: metadata.as_ref().map(metadata_modified_unix).unwrap_or(0),
                is_preview: false,
                perceptual_hash,
            });
        }
    }
    if images.is_empty() {
        return None;
    }
    images.sort_by(|left, right| {
        natord::compare(&left.path.to_string_lossy(), &right.path.to_string_lossy())
    });
    if let Some(first) = images.first_mut() {
        first.is_preview = true;
    }

    let folder_name = path.file_name()?.to_string_lossy().to_string();
    let product_code = extract_product_code(source, &folder_name);
    Some(ProductRecord {
        folder_path: path.to_path_buf(),
        folder_name,
        normalized_code: normalize_code(&product_code),
        product_code,
        normalized_folder_path: normalize_path_key(path),
        preview_path: images.first().map(|image| image.path.clone()),
        image_count: images.len(),
        total_files,
        size_bytes,
        modified_unix: fs::metadata(path)
            .ok()
            .as_ref()
            .map(metadata_modified_unix)
            .unwrap_or(0),
        images,
    })
}

fn write_product_batch(
    connection: &mut Connection,
    source_id: i64,
    generation: i64,
    products: &[ProductRecord],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for product in products {
        write_product(&transaction, source_id, generation, product)?;
    }
    transaction.commit()
}

fn write_product(
    transaction: &Transaction<'_>,
    source_id: i64,
    generation: i64,
    product: &ProductRecord,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO products (
             source_id, folder_path, normalized_folder_path, folder_name,
             product_code, normalized_code, preview_path, image_count,
             total_files, size_bytes, modified_unix, generation
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(source_id, normalized_folder_path) DO UPDATE SET
             folder_path = excluded.folder_path,
             folder_name = excluded.folder_name,
             product_code = excluded.product_code,
             normalized_code = excluded.normalized_code,
             preview_path = excluded.preview_path,
             image_count = excluded.image_count,
             total_files = excluded.total_files,
             size_bytes = excluded.size_bytes,
             modified_unix = excluded.modified_unix,
             generation = excluded.generation",
        params![
            source_id,
            product.folder_path.to_string_lossy(),
            product.normalized_folder_path,
            product.folder_name,
            product.product_code,
            product.normalized_code,
            product
                .preview_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            product.image_count as i64,
            product.total_files as i64,
            product.size_bytes as i64,
            product.modified_unix as i64,
            generation,
        ],
    )?;
    let product_id: i64 = transaction.query_row(
        "SELECT id
         FROM products
         WHERE source_id = ?1 AND normalized_folder_path = ?2",
        params![source_id, product.normalized_folder_path],
        |row| row.get(0),
    )?;
    transaction.execute("DELETE FROM images WHERE product_id = ?1", [product_id])?;
    for image in &product.images {
        transaction.execute(
            "INSERT INTO images (
                 product_id, path, normalized_path, size_bytes, modified_unix,
                 is_preview, perceptual_hash, hash_version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                product_id,
                image.path.to_string_lossy(),
                image.normalized_path,
                image.size_bytes as i64,
                image.modified_unix as i64,
                i64::from(image.is_preview),
                image.perceptual_hash.as_deref(),
                if image.perceptual_hash.is_some() {
                    PHASH_VERSION
                } else {
                    0
                },
            ],
        )?;
    }
    Ok(())
}

fn upsert_source(
    connection: &Connection,
    source: &SourceConfig,
    available: bool,
    generation: i64,
    error: Option<&str>,
) -> rusqlite::Result<i64> {
    let normalized_path = normalize_path_key(Path::new(&source.path));
    connection.execute(
        "INSERT INTO sources (
             path, normalized_path, name, code_mode, code_pattern,
             available, last_error, generation
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(normalized_path) DO UPDATE SET
             path = excluded.path,
             name = excluded.name,
             code_mode = excluded.code_mode,
             code_pattern = excluded.code_pattern,
             available = excluded.available,
             last_error = excluded.last_error,
             generation = excluded.generation",
        params![
            source.path,
            normalized_path,
            source.name,
            format!("{:?}", source.code_mode).to_ascii_lowercase(),
            source.code_pattern,
            i64::from(available),
            error,
            generation,
        ],
    )?;
    connection.query_row(
        "SELECT id FROM sources WHERE normalized_path = ?1",
        [normalized_path],
        |row| row.get(0),
    )
}

fn remove_unconfigured_sources(
    connection: &Connection,
    sources: &[SourceConfig],
) -> rusqlite::Result<()> {
    let configured: HashSet<String> = sources
        .iter()
        .map(|source| normalize_path_key(Path::new(&source.path)))
        .collect();
    let mut statement = connection.prepare("SELECT id, normalized_path FROM sources")?;
    let existing = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, path) in existing {
        if !configured.contains(&path) {
            connection.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

fn initialize_database(path: &Path) -> Result<bool, String> {
    match open_database(path) {
        Ok(_) => Ok(false),
        Err(error) if is_corrupt_database_error(&error) => {
            let backup = corrupt_backup_path(path);
            if path.exists() {
                fs::rename(path, &backup).map_err(|rename_error| {
                    format!(
                        "The search database is corrupt and could not be moved to {}: {}",
                        backup.display(),
                        rename_error
                    )
                })?;
                let wal = path.with_extension("db-wal");
                let shm = path.with_extension("db-shm");
                let _ = fs::remove_file(wal);
                let _ = fs::remove_file(shm);
            }
            open_database(path).map_err(|recreate_error| recreate_error.to_string())?;
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn open_database(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "case_sensitive_like", true)?;
    migrate_database(&connection)?;
    Ok(connection)
}

fn migrate_database(connection: &Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > DATABASE_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS sources (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL,
                 normalized_path TEXT NOT NULL UNIQUE,
                 name TEXT NOT NULL,
                 code_mode TEXT NOT NULL,
                 code_pattern TEXT NOT NULL,
                 available INTEGER NOT NULL DEFAULT 0,
                 last_indexed_unix INTEGER,
                 last_error TEXT,
                 generation INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS products (
                 id INTEGER PRIMARY KEY,
                 source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
                 folder_path TEXT NOT NULL,
                 normalized_folder_path TEXT NOT NULL,
                 folder_name TEXT NOT NULL,
                 product_code TEXT NOT NULL,
                 normalized_code TEXT NOT NULL,
                 preview_path TEXT,
                 image_count INTEGER NOT NULL DEFAULT 0,
                 total_files INTEGER NOT NULL DEFAULT 0,
                 size_bytes INTEGER NOT NULL DEFAULT 0,
                 modified_unix INTEGER NOT NULL DEFAULT 0,
                 generation INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(source_id, normalized_folder_path)
             );
             CREATE TABLE IF NOT EXISTS images (
                 id INTEGER PRIMARY KEY,
                 product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
                 path TEXT NOT NULL,
                 normalized_path TEXT NOT NULL,
                 size_bytes INTEGER NOT NULL DEFAULT 0,
                 modified_unix INTEGER NOT NULL DEFAULT 0,
                 is_preview INTEGER NOT NULL DEFAULT 0,
                 perceptual_hash TEXT,
                 hash_version INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(product_id, normalized_path)
             );
             CREATE INDEX IF NOT EXISTS idx_sources_path
                 ON sources(normalized_path);
             CREATE INDEX IF NOT EXISTS idx_products_code
                 ON products(normalized_code);
             CREATE INDEX IF NOT EXISTS idx_products_folder
                 ON products(normalized_folder_path);
             CREATE INDEX IF NOT EXISTS idx_products_source_code
                 ON products(source_id, normalized_code);
             CREATE INDEX IF NOT EXISTS idx_images_product
                 ON images(product_id);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    if version < 2 {
        connection.execute_batch(
            "BEGIN;
             CREATE INDEX IF NOT EXISTS idx_images_hash_version
                 ON images(hash_version)
                 WHERE perceptual_hash IS NOT NULL;
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    Ok(())
}

fn is_corrupt_database_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(
                inner.code,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
            )
    )
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let stamp = unix_now();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_string())
        .unwrap_or_else(|| "db".into());
    path.with_extension(format!("{extension}.corrupt-{stamp}"))
}

fn generation_token() -> i64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (nanos & i64::MAX as u128) as i64
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn metadata_modified_unix(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{initialize_database, open_database, run_index_job, IndexEvent};
    use crate::config::{ProductCodeMode, SourceConfig};
    use crate::search::{SearchEngine, SearchGroupKind, SearchScope};
    use crate::similarity::PHASH_VERSION;
    use image::{Rgb, RgbImage};
    use rusqlite::Connection;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "luxepix-index-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    fn run_fixture_index(database: &std::path::Path, source: SourceConfig, rebuild: bool) {
        let (tx, rx) = mpsc::channel();
        run_index_job(
            1,
            database,
            &[source],
            rebuild,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            &tx,
        )
        .expect("run index job");
        assert!(rx.try_iter().any(|event| matches!(
            event,
            IndexEvent::Completed {
                cancelled: false,
                ..
            }
        )));
    }

    #[test]
    fn creates_versioned_wal_database() {
        let root = fixture_root("migration");
        let database = root.join("luxepix.db");

        initialize_database(&database).expect("initialize database");
        let connection = Connection::open(&database).expect("open database");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('sources', 'products', 'images')",
                [],
                |row| row.get(0),
            )
            .expect("read table count");

        assert_eq!(version, 2);
        assert_eq!(table_count, 3);
        drop(connection);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn rebuild_groups_multiple_images_under_one_product() {
        let root = fixture_root("grouping");
        let source_root = root.join("Products");
        let product = source_root.join("SKU-1042 Blue");
        fs::create_dir_all(&product).expect("create product");
        fs::write(product.join("front.jpg"), b"front").expect("write front");
        fs::write(product.join("side.png"), b"side").expect("write side");
        fs::write(product.join("notes.txt"), b"notes").expect("write notes");
        let database = root.join("index").join("luxepix.db");
        let source = SourceConfig {
            name: "Catalog".into(),
            path: source_root.to_string_lossy().to_string(),
            code_mode: ProductCodeMode::Prefix,
            code_pattern: "SKU".into(),
            ..SourceConfig::default()
        };

        run_fixture_index(&database, source, true);
        let response = SearchEngine::new(database)
            .search("104", &SearchScope::AllSources, 10)
            .expect("search index");

        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.groups[0].kind, SearchGroupKind::Prefix);
        assert_eq!(response.groups[0].results.len(), 1);
        assert_eq!(response.groups[0].results[0].product_code, "1042");
        assert_eq!(response.groups[0].results[0].image_count, 2);
        assert_eq!(response.groups[0].results[0].image_paths.len(), 2);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn reconciliation_removes_deleted_product_rows() {
        let root = fixture_root("reconcile");
        let source_root = root.join("Products");
        let first = source_root.join("1001");
        let second = source_root.join("1002");
        fs::create_dir_all(&first).expect("create first product");
        fs::create_dir_all(&second).expect("create second product");
        fs::write(first.join("a.jpg"), b"a").expect("write first");
        fs::write(second.join("b.jpg"), b"b").expect("write second");
        let database = root.join("index").join("luxepix.db");
        let source = SourceConfig {
            path: source_root.to_string_lossy().to_string(),
            ..SourceConfig::default()
        };

        run_fixture_index(&database, source.clone(), true);
        fs::remove_dir_all(&second).expect("remove second product");
        run_fixture_index(&database, source, false);
        let connection = Connection::open(&database).expect("open database");
        let count: i64 = connection
            .query_row("SELECT count(*) FROM products", [], |row| row.get(0))
            .expect("count products");

        assert_eq!(count, 1);
        drop(connection);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn corrupt_database_is_preserved_and_recreated() {
        let root = fixture_root("recovery");
        fs::create_dir_all(&root).expect("create fixture");
        let database = root.join("luxepix.db");
        fs::write(&database, b"not a sqlite database").expect("write corrupt database");

        let recovered = initialize_database(&database).expect("recover database");
        let backups = fs::read_dir(&root)
            .expect("list fixture")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count();

        assert!(recovered);
        assert!(database.is_file());
        assert_eq!(backups, 1);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn version_one_database_migrates_for_perceptual_hash_queries() {
        let root = fixture_root("hash-migration");
        fs::create_dir_all(&root).expect("create fixture");
        let database = root.join("luxepix.db");
        let connection = Connection::open(&database).expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE images (
                     id INTEGER PRIMARY KEY,
                     product_id INTEGER NOT NULL,
                     path TEXT NOT NULL,
                     normalized_path TEXT NOT NULL,
                     size_bytes INTEGER NOT NULL DEFAULT 0,
                     modified_unix INTEGER NOT NULL DEFAULT 0,
                     is_preview INTEGER NOT NULL DEFAULT 0,
                     perceptual_hash TEXT,
                     hash_version INTEGER NOT NULL DEFAULT 0
                 );
                 PRAGMA user_version = 1;",
            )
            .expect("create version one database");
        drop(connection);

        let connection = open_database(&database).expect("migrate database");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        let index_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_images_hash_version'",
                [],
                |row| row.get(0),
            )
            .expect("find hash index");

        assert_eq!(version, 2);
        assert_eq!(index_count, 1);
        drop(connection);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn indexing_persists_real_perceptual_hashes_for_valid_images() {
        let root = fixture_root("image-hashes");
        let source_root = root.join("Products");
        let product = source_root.join("1001 Blue");
        fs::create_dir_all(&product).expect("create product");
        let mut image = RgbImage::new(80, 80);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if (x / 10 + y / 8) % 2 == 0 {
                Rgb([230, 40, 80])
            } else {
                Rgb([20, 170, 210])
            };
        }
        image
            .save(product.join("preview.png"))
            .expect("save valid image");
        let database = root.join("index").join("luxepix.db");
        let source = SourceConfig {
            path: source_root.to_string_lossy().to_string(),
            ..SourceConfig::default()
        };

        run_fixture_index(&database, source, true);
        let connection = Connection::open(&database).expect("open database");
        let (hash, version): (Option<String>, i64) = connection
            .query_row(
                "SELECT perceptual_hash, hash_version FROM images LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read image hash");

        assert_eq!(hash.as_deref().map(str::len), Some(16));
        assert_eq!(version, PHASH_VERSION);
        drop(connection);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn search_scopes_and_fallback_groups_are_kept_separate() {
        let root = fixture_root("search-scopes");
        let source_root = root.join("Products");
        let collection_a = source_root.join("Collection A");
        let collection_b = source_root.join("Collection B");
        let product_a = collection_a.join("AB-1042 Blue");
        let product_b = collection_b.join("AB-2042 Red");
        fs::create_dir_all(&product_a).expect("create product A");
        fs::create_dir_all(&product_b).expect("create product B");
        fs::write(product_a.join("a.jpg"), b"a").expect("write product A");
        fs::write(product_b.join("b.jpg"), b"b").expect("write product B");
        let database = root.join("index").join("luxepix.db");
        let source = SourceConfig {
            name: "Catalog".into(),
            path: source_root.to_string_lossy().to_string(),
            ..SourceConfig::default()
        };

        run_fixture_index(&database, source.clone(), true);
        let engine = SearchEngine::new(database);
        let scoped = engine
            .search("AB", &SearchScope::Current(collection_a), 10)
            .expect("search current folder");
        assert_eq!(scoped.result_count(), 1);
        assert_eq!(
            scoped.first().expect("scoped result").folder_path,
            product_a
        );

        let contains = engine
            .search("042B", &SearchScope::CurrentSource(source.path.clone()), 10)
            .expect("contains fallback");
        assert_eq!(contains.groups[0].kind, SearchGroupKind::Contains);

        let fuzzy = engine
            .search("AC1043BLU", &SearchScope::AllSources, 10)
            .expect("fuzzy fallback");
        assert!(fuzzy
            .groups
            .iter()
            .any(|group| group.kind == SearchGroupKind::Fuzzy));
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    #[ignore = "performance gate; run with cargo test --release -- --ignored"]
    fn prefix_search_across_100k_products_stays_under_100ms_p95() {
        let root = fixture_root("performance");
        let database = root.join("index").join("luxepix.db");
        let mut connection = open_database(&database).expect("open database");
        connection
            .execute(
                "INSERT INTO sources (
                     path, normalized_path, name, code_mode, code_pattern, available, generation
                 ) VALUES ('N:\\Products', 'n:/products', 'Catalog', 'foldername', '', 1, 1)",
                [],
            )
            .expect("insert source");
        let transaction = connection.transaction().expect("start transaction");
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO products (
                         source_id, folder_path, normalized_folder_path, folder_name,
                         product_code, normalized_code, image_count, total_files,
                         size_bytes, modified_unix, generation
                     ) VALUES (1, ?1, ?2, ?3, ?4, ?4, 0, 0, 0, 0, 1)",
                )
                .expect("prepare product insert");
            for number in 0..100_000 {
                let code = format!("SKU{number:06}");
                let path = format!(r"N:\Products\{code}");
                let normalized_path = format!("n:/products/{}", code.to_ascii_lowercase());
                statement
                    .execute(rusqlite::params![path, normalized_path, code, code])
                    .expect("insert product");
            }
        }
        transaction.commit().expect("commit products");
        drop(connection);

        let engine = SearchEngine::new(database);
        let mut timings = Vec::new();
        for _ in 0..30 {
            let started = Instant::now();
            let response = engine
                .search("SKU099", &SearchScope::AllSources, 10)
                .expect("search 100k products");
            timings.push(started.elapsed());
            assert_eq!(response.result_count(), 10);
        }
        timings.sort_unstable();
        let p95 = timings[(timings.len() * 95 / 100).min(timings.len() - 1)];
        println!("100,000-product prefix-search p95: {p95:?}");

        assert!(
            p95 < Duration::from_millis(100),
            "100k-product prefix p95 was {p95:?}"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
