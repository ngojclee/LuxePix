use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageBuffer, Rgba};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PHASH_VERSION: i64 = 1;
const PHASH_SIZE: usize = 32;
const LOW_FREQUENCY_SIZE: usize = 8;
const HASH_BITS: u32 = 63;
const LOW_CONFIDENCE_SCORE: f32 = 0.72;
const MODEL_PACK_SCHEMA_VERSION: u32 = 1;
const BRIDGE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PerceptualHash(u64);

impl PerceptualHash {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let image = image::ImageReader::open(path)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())?
            .decode()
            .map_err(|error| error.to_string())?;
        Ok(Self::from_image(&image))
    }

    pub fn from_image(image: &DynamicImage) -> Self {
        let resized = image
            .resize_exact(
                PHASH_SIZE as u32,
                PHASH_SIZE as u32,
                image::imageops::FilterType::Lanczos3,
            )
            .to_luma8();
        let cosine = cosine_table();
        let mut row_dct = [[0.0f32; PHASH_SIZE]; PHASH_SIZE];
        for (y, row) in row_dct.iter_mut().enumerate() {
            for (u, coefficient) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (x, cosine_value) in cosine[u].iter().enumerate() {
                    sum += f32::from(resized.get_pixel(x as u32, y as u32)[0]) * cosine_value;
                }
                *coefficient = dct_scale(u) * sum;
            }
        }

        let mut coefficients = [0.0f32; LOW_FREQUENCY_SIZE * LOW_FREQUENCY_SIZE];
        for (v, coefficient_row) in coefficients
            .chunks_exact_mut(LOW_FREQUENCY_SIZE)
            .enumerate()
        {
            for (u, coefficient) in coefficient_row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (row, cosine_value) in row_dct.iter().zip(cosine[v].iter()) {
                    sum += row[u] * cosine_value;
                }
                *coefficient = dct_scale(v) * sum;
            }
        }

        let mut median_values = coefficients[1..].to_vec();
        median_values.sort_by(f32::total_cmp);
        let median = median_values[median_values.len() / 2];
        let mut bits = 0u64;
        for (bit, coefficient) in coefficients[1..].iter().enumerate() {
            if *coefficient > median {
                bits |= 1u64 << bit;
            }
        }
        Self(bits)
    }

    pub fn similarity(self, other: Self) -> f32 {
        let distance = (self.0 ^ other.0).count_ones();
        1.0 - distance as f32 / HASH_BITS as f32
    }

    pub fn to_database(self) -> String {
        format!("{:016x}", self.0)
    }

    pub fn from_database(value: &str) -> Option<Self> {
        u64::from_str_radix(value.trim(), 16).ok().map(Self)
    }
}

fn cosine_table() -> &'static [[f32; PHASH_SIZE]; PHASH_SIZE] {
    static TABLE: OnceLock<[[f32; PHASH_SIZE]; PHASH_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [[0.0f32; PHASH_SIZE]; PHASH_SIZE];
        for (frequency, row) in table.iter_mut().enumerate() {
            for (sample, value) in row.iter_mut().enumerate() {
                *value = ((std::f32::consts::PI * (2 * sample + 1) as f32 * frequency as f32)
                    / (2 * PHASH_SIZE) as f32)
                    .cos();
            }
        }
        table
    })
}

fn dct_scale(frequency: usize) -> f32 {
    if frequency == 0 {
        (1.0 / PHASH_SIZE as f32).sqrt()
    } else {
        (2.0 / PHASH_SIZE as f32).sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct SimilarityResult {
    pub product_id: i64,
    pub product_code: String,
    pub folder_name: String,
    pub folder_path: PathBuf,
    pub source_name: String,
    pub source_path: PathBuf,
    pub matched_image: PathBuf,
    pub score: f32,
}

impl SimilarityResult {
    pub fn low_confidence(&self) -> bool {
        self.score < LOW_CONFIDENCE_SCORE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimilarityBackend {
    PerceptualHash,
    ClipReranked,
}

impl SimilarityBackend {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PerceptualHash => "Local pHash",
            Self::ClipReranked => "Local AI Model Pack",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimilarityResponse {
    pub results: Vec<SimilarityResult>,
    pub elapsed: Duration,
    pub backend: SimilarityBackend,
    pub fallback_warning: Option<String>,
}

impl Default for SimilarityResponse {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            elapsed: Duration::ZERO,
            backend: SimilarityBackend::PerceptualHash,
            fallback_warning: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SimilarityEngine {
    database_path: PathBuf,
    model_pack: Option<ModelPack>,
}

impl SimilarityEngine {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            database_path,
            model_pack: None,
        }
    }

    pub fn set_model_pack(&mut self, model_pack: Option<ModelPack>) {
        self.model_pack = model_pack;
    }

    pub fn search_path(&self, path: &Path, limit: usize) -> Result<SimilarityResponse, String> {
        let query_hash = PerceptualHash::from_path(path)?;
        self.search_hash(query_hash, Some(path), limit)
    }

    pub fn search_image(
        &self,
        image: &DynamicImage,
        query_path: Option<&Path>,
        limit: usize,
    ) -> Result<SimilarityResponse, String> {
        self.search_hash(PerceptualHash::from_image(image), query_path, limit)
    }

    pub fn search_hash(
        &self,
        query_hash: PerceptualHash,
        query_path: Option<&Path>,
        limit: usize,
    ) -> Result<SimilarityResponse, String> {
        let started = Instant::now();
        if !self.database_path.is_file() {
            return Ok(SimilarityResponse {
                elapsed: started.elapsed(),
                ..SimilarityResponse::default()
            });
        }
        let connection = Connection::open_with_flags(
            &self.database_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(|error| error.to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT
                     p.id, p.product_code, p.folder_name, p.folder_path,
                     s.name, s.path, i.path, i.perceptual_hash
                 FROM images i
                 JOIN products p ON p.id = i.product_id
                 JOIN sources s ON s.id = p.source_id
                 WHERE i.hash_version = ?1
                   AND i.perceptual_hash IS NOT NULL",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([PHASH_VERSION], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut best_by_product: HashMap<i64, SimilarityResult> = HashMap::new();
        for row in rows {
            let (
                product_id,
                product_code,
                folder_name,
                folder_path,
                source_name,
                source_path,
                image_path,
                stored_hash,
            ) = row.map_err(|error| error.to_string())?;
            let Some(stored_hash) = PerceptualHash::from_database(&stored_hash) else {
                continue;
            };
            let score = query_hash.similarity(stored_hash);
            let candidate = SimilarityResult {
                product_id,
                product_code,
                folder_name,
                folder_path: PathBuf::from(folder_path),
                source_name,
                source_path: PathBuf::from(source_path),
                matched_image: PathBuf::from(image_path),
                score,
            };
            let should_replace = best_by_product
                .get(&product_id)
                .is_none_or(|current| score > current.score);
            if should_replace {
                best_by_product.insert(product_id, candidate);
            }
        }

        let shortlist_limit = limit.clamp(5, 50).max(25);
        let mut results: Vec<_> = best_by_product.into_values().collect();
        results.sort_by(compare_similarity_results);
        results.truncate(shortlist_limit);

        let mut backend = SimilarityBackend::PerceptualHash;
        let mut fallback_warning = None;
        if let (Some(model_pack), Some(query_path)) = (&self.model_pack, query_path) {
            match ClipBridge::rerank(model_pack, query_path, &results) {
                Ok(scores) => {
                    let by_id: HashMap<i64, f32> = scores.into_iter().collect();
                    for result in &mut results {
                        if let Some(score) = by_id.get(&result.product_id) {
                            result.score = score.clamp(0.0, 1.0);
                        }
                    }
                    results.sort_by(compare_similarity_results);
                    backend = SimilarityBackend::ClipReranked;
                }
                Err(error) => {
                    fallback_warning = Some(format!(
                        "Model Pack unavailable; showing pHash results: {error}"
                    ));
                }
            }
        }

        results.truncate(limit.clamp(5, 50));
        Ok(SimilarityResponse {
            results,
            elapsed: started.elapsed(),
            backend,
            fallback_warning,
        })
    }
}

fn compare_similarity_results(
    left: &SimilarityResult,
    right: &SimilarityResult,
) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.product_code.cmp(&right.product_code))
        .then_with(|| left.folder_path.cmp(&right.folder_path))
}

#[derive(Clone)]
pub enum SimilarityQuery {
    Path(PathBuf),
    Image { image: DynamicImage, label: String },
}

impl SimilarityQuery {
    fn label(&self) -> String {
        match self {
            Self::Path(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            Self::Image { label, .. } => label.clone(),
        }
    }
}

#[derive(Debug)]
pub struct SimilarityCompletion {
    pub id: u64,
    pub query_label: String,
    pub result: Result<SimilarityResponse, String>,
}

pub struct SimilarityManager {
    engine: SimilarityEngine,
    cache_dir: PathBuf,
    result_tx: mpsc::Sender<SimilarityCompletion>,
    result_rx: mpsc::Receiver<SimilarityCompletion>,
    next_id: u64,
    latest_id: Option<u64>,
}

impl SimilarityManager {
    pub fn new(engine: SimilarityEngine, cache_dir: PathBuf) -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            engine,
            cache_dir,
            result_tx,
            result_rx,
            next_id: 1,
            latest_id: None,
        }
    }

    pub fn set_model_pack(&mut self, model_pack: Option<ModelPack>) {
        self.engine.set_model_pack(model_pack);
    }

    pub fn submit(&mut self, query: SimilarityQuery, limit: usize) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.latest_id = Some(id);
        let engine = self.engine.clone();
        let cache_dir = self.cache_dir.clone();
        let result_tx = self.result_tx.clone();
        let query_label = query.label();
        let thread_label = query_label.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("luxepix-similarity-{id}"))
            .spawn(move || {
                let result = match query {
                    SimilarityQuery::Path(path) => engine.search_path(&path, limit),
                    SimilarityQuery::Image { image, .. } => {
                        let _ = fs::create_dir_all(&cache_dir);
                        let temporary_path =
                            cache_dir.join(format!("query-{}-{id}.png", std::process::id()));
                        let saved =
                            image.save_with_format(&temporary_path, image::ImageFormat::Png);
                        let result = match saved {
                            Ok(()) => engine.search_image(&image, Some(&temporary_path), limit),
                            Err(_) => engine.search_image(&image, None, limit),
                        };
                        let _ = fs::remove_file(temporary_path);
                        result
                    }
                };
                let _ = result_tx.send(SimilarityCompletion {
                    id,
                    query_label: thread_label,
                    result,
                });
            });
        if let Err(error) = spawn_result {
            let _ = self.result_tx.send(SimilarityCompletion {
                id,
                query_label,
                result: Err(error.to_string()),
            });
        }
        id
    }

    pub fn drain_latest(&mut self, limit: usize) -> Vec<SimilarityCompletion> {
        let mut completions = Vec::new();
        for _ in 0..limit {
            let Ok(completion) = self.result_rx.try_recv() else {
                break;
            };
            if self.latest_id == Some(completion.id) {
                completions.push(completion);
            }
        }
        completions
    }
}

pub fn clipboard_image() -> Result<DynamicImage, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let image = clipboard
        .get_image()
        .map_err(|error| format!("The clipboard does not contain a bitmap image: {error}"))?;
    let width = u32::try_from(image.width).map_err(|_| "Clipboard image is too wide.")?;
    let height = u32::try_from(image.height).map_err(|_| "Clipboard image is too tall.")?;
    let pixels = image.bytes.into_owned();
    let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, pixels)
        .ok_or_else(|| "Clipboard image data is incomplete.".to_string())?;
    Ok(DynamicImage::ImageRgba8(buffer))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelPackManifest {
    pub schema_version: u32,
    pub pack_version: String,
    pub helper: String,
    pub model: String,
    #[serde(default)]
    pub runtime_files: Vec<String>,
    #[serde(default)]
    pub checksums: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct ModelPack {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub helper_path: PathBuf,
    pub model_path: PathBuf,
    pub manifest: ModelPackManifest,
}

impl ModelPack {
    pub fn discover(
        executable_dir: &Path,
        similarity_dir: &Path,
        custom_path: Option<&Path>,
    ) -> Result<Option<Self>, String> {
        let mut candidates = Vec::new();
        if let Some(custom_path) = custom_path.filter(|path| !path.as_os_str().is_empty()) {
            candidates.push(custom_path.to_path_buf());
        }
        candidates.push(executable_dir.join("model-pack"));
        candidates.push(executable_dir.join("models"));
        candidates.push(similarity_dir.join("model-pack"));

        for candidate in candidates {
            let manifest_path = if candidate.is_file() {
                candidate
            } else {
                candidate.join("model-pack.json")
            };
            if manifest_path.is_file() {
                return Self::load(&manifest_path).map(Some);
            }
        }
        Ok(None)
    }

    pub fn load(manifest_path: &Path) -> Result<Self, String> {
        let bytes = fs::read(manifest_path).map_err(|error| error.to_string())?;
        let manifest: ModelPackManifest =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if manifest.schema_version != MODEL_PACK_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported Model Pack schema {}.",
                manifest.schema_version
            ));
        }
        if manifest.pack_version.trim().is_empty() {
            return Err("Model Pack version is missing.".into());
        }
        if !manifest
            .helper
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("LuxePixAI.exe"))
        {
            return Err("Model Pack helper must be named LuxePixAI.exe.".into());
        }
        let root = manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let helper_path = safe_pack_path(&root, &manifest.helper)?;
        let model_path = safe_pack_path(&root, &manifest.model)?;
        if !helper_path.is_file() {
            return Err(format!(
                "Model Pack helper is missing: {}",
                helper_path.display()
            ));
        }
        if !model_path.is_file() {
            return Err(format!(
                "Model Pack model is missing: {}",
                model_path.display()
            ));
        }
        for runtime_file in &manifest.runtime_files {
            let path = safe_pack_path(&root, runtime_file)?;
            if !path.is_file() {
                return Err(format!(
                    "Model Pack runtime file is missing: {}",
                    path.display()
                ));
            }
        }
        for required in std::iter::once(&manifest.helper)
            .chain(std::iter::once(&manifest.model))
            .chain(manifest.runtime_files.iter())
        {
            if !manifest.checksums.contains_key(required) {
                return Err(format!("Model Pack checksum is missing for {required}."));
            }
        }
        for (relative_path, expected) in &manifest.checksums {
            let path = safe_pack_path(&root, relative_path)?;
            let actual = sha256_file(&path)?;
            if !actual.eq_ignore_ascii_case(expected.trim()) {
                return Err(format!(
                    "Model Pack checksum mismatch for {}.",
                    path.display()
                ));
            }
        }
        Ok(Self {
            root,
            manifest_path: manifest_path.to_path_buf(),
            helper_path,
            model_path,
            manifest,
        })
    }
}

fn safe_pack_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("Unsafe Model Pack path: {relative}"));
    }
    Ok(root.join(path))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Serialize)]
struct BridgeRequest<'a> {
    schema_version: u32,
    query_path: &'a Path,
    model_path: &'a Path,
    candidates: Vec<BridgeCandidate<'a>>,
}

#[derive(Serialize)]
struct BridgeCandidate<'a> {
    product_id: i64,
    image_path: &'a Path,
    phash_score: f32,
}

#[derive(Deserialize)]
struct BridgeResponse {
    schema_version: u32,
    scores: Vec<BridgeScore>,
}

#[derive(Deserialize)]
struct BridgeScore {
    product_id: i64,
    score: f32,
}

struct ClipBridge;

impl ClipBridge {
    fn rerank(
        model_pack: &ModelPack,
        query_path: &Path,
        candidates: &[SimilarityResult],
    ) -> Result<Vec<(i64, f32)>, String> {
        let request = BridgeRequest {
            schema_version: BRIDGE_SCHEMA_VERSION,
            query_path,
            model_path: &model_pack.model_path,
            candidates: candidates
                .iter()
                .map(|candidate| BridgeCandidate {
                    product_id: candidate.product_id,
                    image_path: &candidate.matched_image,
                    phash_score: candidate.score,
                })
                .collect(),
        };
        let payload = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let mut child = Command::new(&model_pack.helper_path)
            .arg("--rerank-json")
            .arg(&model_pack.manifest_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&payload)
                .map_err(|error| error.to_string())?;
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match child.try_wait().map_err(|error| error.to_string())? {
                Some(status) if status.success() => break,
                Some(status) => {
                    return Err(format!("LuxePixAI exited with status {status}."));
                }
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(40)),
                None => {
                    let _ = child.kill();
                    return Err("LuxePixAI timed out.".into());
                }
            }
        }
        let mut output = Vec::new();
        if let Some(mut stdout) = child.stdout.take() {
            stdout
                .read_to_end(&mut output)
                .map_err(|error| error.to_string())?;
        }
        let response: BridgeResponse =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        if response.schema_version != BRIDGE_SCHEMA_VERSION {
            return Err(format!(
                "LuxePixAI returned unsupported schema {}.",
                response.schema_version
            ));
        }
        Ok(response
            .scores
            .into_iter()
            .filter(|score| score.score.is_finite())
            .map(|score| (score.product_id, score.score))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelPack, ModelPackManifest, PerceptualHash, SimilarityBackend, SimilarityEngine,
        PHASH_VERSION,
    };
    use image::{DynamicImage, Rgb, RgbImage};
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "luxepix-similarity-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    fn patterned_image(phase: u32) -> DynamicImage {
        let mut image = RgbImage::new(96, 96);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let stripe = if (((x + phase) / 12) + y / 16).is_multiple_of(2) {
                210
            } else {
                30
            };
            *pixel = Rgb([stripe, (x * 2 % 255) as u8, (y * 3 % 255) as u8]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn perceptual_hash_is_deterministic_and_round_trips() {
        let image = patterned_image(0);
        let hash = PerceptualHash::from_image(&image);
        let encoded = hash.to_database();

        assert_eq!(hash, PerceptualHash::from_image(&image));
        assert_eq!(Some(hash), PerceptualHash::from_database(&encoded));
        assert_eq!(hash.similarity(hash), 1.0);
    }

    #[test]
    fn product_ranking_keeps_only_the_best_image_per_product() {
        let root = test_root("grouping");
        fs::create_dir_all(&root).expect("create test root");
        let database = root.join("luxepix.db");
        let connection = Connection::open(&database).expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE sources (
                     id INTEGER PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL
                 );
                 CREATE TABLE products (
                     id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL,
                     product_code TEXT NOT NULL, folder_name TEXT NOT NULL,
                     folder_path TEXT NOT NULL
                 );
                 CREATE TABLE images (
                     id INTEGER PRIMARY KEY, product_id INTEGER NOT NULL,
                     path TEXT NOT NULL, perceptual_hash TEXT, hash_version INTEGER NOT NULL
                 );
                 INSERT INTO sources VALUES (1, 'Catalog', 'N:\\Products');
                 INSERT INTO products VALUES (1, 1, '1001', '1001 Blue', 'N:\\Products\\1001');
                 INSERT INTO products VALUES (2, 1, '1002', '1002 Red', 'N:\\Products\\1002');",
            )
            .expect("create fixture schema");
        let query = PerceptualHash::from_image(&patterned_image(0));
        let weak = PerceptualHash::from_image(&patterned_image(7));
        connection
            .execute(
                "INSERT INTO images VALUES (1, 1, 'a.jpg', ?1, ?2)",
                rusqlite::params![weak.to_database(), PHASH_VERSION],
            )
            .expect("insert weak image");
        connection
            .execute(
                "INSERT INTO images VALUES (2, 1, 'b.jpg', ?1, ?2)",
                rusqlite::params![query.to_database(), PHASH_VERSION],
            )
            .expect("insert best image");
        connection
            .execute(
                "INSERT INTO images VALUES (3, 2, 'c.jpg', ?1, ?2)",
                rusqlite::params![weak.to_database(), PHASH_VERSION],
            )
            .expect("insert second product");
        drop(connection);

        let response = SimilarityEngine::new(database)
            .search_hash(query, None, 10)
            .expect("search hashes");

        assert_eq!(response.backend, SimilarityBackend::PerceptualHash);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].product_id, 1);
        assert_eq!(response.results[0].matched_image, PathBuf::from("b.jpg"));
        assert_eq!(response.results[0].score, 1.0);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn model_pack_rejects_unsafe_paths_and_bad_checksums() {
        let root = test_root("model-pack");
        fs::create_dir_all(&root).expect("create model root");
        fs::write(root.join("LuxePixAI.exe"), b"helper").expect("write helper");
        fs::write(root.join("vision.onnx"), b"model").expect("write model");
        let model_hash = format!("{:x}", Sha256::digest(b"model"));
        let helper_hash = format!("{:x}", Sha256::digest(b"helper"));
        let manifest = ModelPackManifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            helper: "LuxePixAI.exe".into(),
            model: "vision.onnx".into(),
            runtime_files: Vec::new(),
            checksums: HashMap::from([
                ("LuxePixAI.exe".into(), helper_hash),
                ("vision.onnx".into(), model_hash),
            ]),
        };
        let manifest_path = root.join("model-pack.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
        assert!(ModelPack::load(&manifest_path).is_ok());

        let unsafe_manifest = ModelPackManifest {
            model: "../vision.onnx".into(),
            ..manifest
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&unsafe_manifest).expect("serialize unsafe manifest"),
        )
        .expect("write unsafe manifest");
        assert!(ModelPack::load(&manifest_path).is_err());
        fs::remove_dir_all(root).expect("remove model root");
    }
}
