use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchRenameRule {
    pub find: String,
    pub replace: String,
    pub prefix: String,
    pub suffix: String,
    pub sequence_enabled: bool,
    pub sequence_start: u32,
    pub sequence_padding: usize,
}

impl Default for BatchRenameRule {
    fn default() -> Self {
        Self {
            find: String::new(),
            replace: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            sequence_enabled: false,
            sequence_start: 1,
            sequence_padding: 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BatchRenamePreview {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub error: Option<String>,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub struct DuplicateGroup {
    pub size_bytes: u64,
    pub sha256: String,
    pub paths: Vec<PathBuf>,
}

impl DuplicateGroup {
    pub fn wasted_bytes(&self) -> u64 {
        self.size_bytes
            .saturating_mul(self.paths.len().saturating_sub(1) as u64)
    }
}

#[derive(Clone, Debug)]
pub struct FolderSizeEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub file_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerTaskKind {
    BatchRename,
    DuplicateFinder,
    CreateArchive,
    ExtractArchive,
    FolderSizes,
}

impl PowerTaskKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::BatchRename => "Batch Rename",
            Self::DuplicateFinder => "Duplicate Finder",
            Self::CreateArchive => "Create ZIP",
            Self::ExtractArchive => "Extract ZIP",
            Self::FolderSizes => "Folder Size Analysis",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PowerTaskSnapshot {
    pub id: u64,
    pub kind: PowerTaskKind,
    pub title: String,
    pub completed: usize,
    pub total: usize,
    pub current_path: Option<PathBuf>,
}

impl PowerTaskSnapshot {
    pub fn progress_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f64 / self.total as f64).clamp(0.0, 1.0) as f32
        }
    }
}

#[derive(Debug)]
pub enum PowerToolResult {
    BatchRenamed(Vec<(PathBuf, PathBuf)>),
    Duplicates {
        root: PathBuf,
        groups: Vec<DuplicateGroup>,
    },
    ArchiveCreated(PathBuf),
    ArchiveExtracted(PathBuf),
    FolderSizes {
        root: PathBuf,
        entries: Vec<FolderSizeEntry>,
    },
}

pub struct PowerToolCompletion {
    pub kind: PowerTaskKind,
    pub result: Result<PowerToolResult, String>,
}

enum PowerEvent {
    Prepared {
        id: u64,
        total: usize,
    },
    Progress {
        id: u64,
        completed: usize,
        path: Option<PathBuf>,
    },
    Finished {
        id: u64,
        kind: PowerTaskKind,
        result: Result<PowerToolResult, String>,
    },
}

pub struct PowerToolManager {
    event_tx: mpsc::Sender<PowerEvent>,
    event_rx: mpsc::Receiver<PowerEvent>,
    active: Option<PowerTaskSnapshot>,
    cancellation: Option<Arc<AtomicBool>>,
    next_id: u64,
}

impl Default for PowerToolManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerToolManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            event_tx,
            event_rx,
            active: None,
            cancellation: None,
            next_id: 1,
        }
    }

    pub fn active(&self) -> Option<&PowerTaskSnapshot> {
        self.active.as_ref()
    }

    pub fn active_count(&self) -> usize {
        usize::from(self.active.is_some())
    }

    pub fn cancel(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Relaxed);
        }
    }

    pub fn start_batch_rename(&mut self, preview: Vec<BatchRenamePreview>) -> Result<(), String> {
        self.start_task(
            PowerTaskKind::BatchRename,
            format!("Rename {} item(s)", preview.len()),
            move |id, cancellation, tx| {
                let total = preview.iter().filter(|item| item.changed).count();
                let _ = tx.send(PowerEvent::Prepared { id, total });
                execute_batch_rename(&preview, &cancellation, |completed, path| {
                    let _ = tx.send(PowerEvent::Progress {
                        id,
                        completed,
                        path: Some(path.to_path_buf()),
                    });
                })
                .map(PowerToolResult::BatchRenamed)
            },
        )
    }

    pub fn start_duplicate_scan(&mut self, root: PathBuf) -> Result<(), String> {
        let title = format!("Find duplicates in {}", root.display());
        self.start_task(
            PowerTaskKind::DuplicateFinder,
            title,
            move |id, cancellation, tx| {
                find_duplicates(&root, &cancellation, |completed, total, path| {
                    let _ = tx.send(PowerEvent::Prepared { id, total });
                    let _ = tx.send(PowerEvent::Progress {
                        id,
                        completed,
                        path: path.map(Path::to_path_buf),
                    });
                })
                .map(|groups| PowerToolResult::Duplicates { root, groups })
            },
        )
    }

    pub fn start_create_archive(
        &mut self,
        paths: Vec<PathBuf>,
        destination: PathBuf,
    ) -> Result<(), String> {
        let title = format!("Create {}", destination.display());
        self.start_task(
            PowerTaskKind::CreateArchive,
            title,
            move |id, cancellation, tx| {
                create_zip_archive(
                    &paths,
                    &destination,
                    &cancellation,
                    |completed, total, path| {
                        let _ = tx.send(PowerEvent::Prepared { id, total });
                        let _ = tx.send(PowerEvent::Progress {
                            id,
                            completed,
                            path: path.map(Path::to_path_buf),
                        });
                    },
                )
                .map(PowerToolResult::ArchiveCreated)
            },
        )
    }

    pub fn start_extract_archive(
        &mut self,
        archive: PathBuf,
        destination: PathBuf,
    ) -> Result<(), String> {
        let title = format!("Extract {}", archive.display());
        self.start_task(
            PowerTaskKind::ExtractArchive,
            title,
            move |id, cancellation, tx| {
                extract_zip_archive(
                    &archive,
                    &destination,
                    &cancellation,
                    |completed, total, path| {
                        let _ = tx.send(PowerEvent::Prepared { id, total });
                        let _ = tx.send(PowerEvent::Progress {
                            id,
                            completed,
                            path: path.map(Path::to_path_buf),
                        });
                    },
                )
                .map(PowerToolResult::ArchiveExtracted)
            },
        )
    }

    pub fn start_folder_sizes(&mut self, root: PathBuf) -> Result<(), String> {
        let title = format!("Analyze folder sizes in {}", root.display());
        self.start_task(
            PowerTaskKind::FolderSizes,
            title,
            move |id, cancellation, tx| {
                analyze_folder_sizes(&root, &cancellation, |completed, total, path| {
                    let _ = tx.send(PowerEvent::Prepared { id, total });
                    let _ = tx.send(PowerEvent::Progress {
                        id,
                        completed,
                        path: path.map(Path::to_path_buf),
                    });
                })
                .map(|entries| PowerToolResult::FolderSizes { root, entries })
            },
        )
    }

    pub fn process_events(&mut self, limit: usize) -> Vec<PowerToolCompletion> {
        let mut completions = Vec::new();
        for _ in 0..limit {
            let event = match self.event_rx.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            match event {
                PowerEvent::Prepared { id, total } => {
                    if let Some(active) = &mut self.active {
                        if active.id == id {
                            active.total = total;
                        }
                    }
                }
                PowerEvent::Progress {
                    id,
                    completed,
                    path,
                } => {
                    if let Some(active) = &mut self.active {
                        if active.id == id {
                            active.completed = completed;
                            active.current_path = path;
                        }
                    }
                }
                PowerEvent::Finished { id, kind, result } => {
                    if self.active.as_ref().is_some_and(|active| active.id == id) {
                        self.active = None;
                        self.cancellation = None;
                        completions.push(PowerToolCompletion { kind, result });
                    }
                }
            }
        }
        completions
    }

    fn start_task(
        &mut self,
        kind: PowerTaskKind,
        title: String,
        work: impl FnOnce(
                u64,
                Arc<AtomicBool>,
                mpsc::Sender<PowerEvent>,
            ) -> Result<PowerToolResult, String>
            + Send
            + 'static,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("Another power tool is already running.".into());
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        self.active = Some(PowerTaskSnapshot {
            id,
            kind,
            title,
            completed: 0,
            total: 0,
            current_path: None,
        });
        self.cancellation = Some(Arc::clone(&cancellation));
        let tx = self.event_tx.clone();
        let thread_tx = tx.clone();
        thread::Builder::new()
            .name(format!("luxepix-power-tool-{}", id))
            .spawn(move || {
                let result = work(id, cancellation, thread_tx);
                let _ = tx.send(PowerEvent::Finished { id, kind, result });
            })
            .map_err(|error| {
                self.active = None;
                self.cancellation = None;
                format!("Could not start {}: {error}", kind.label())
            })?;
        Ok(())
    }
}

pub fn preview_batch_rename(paths: &[PathBuf], rule: &BatchRenameRule) -> Vec<BatchRenamePreview> {
    let sources: HashSet<String> = paths.iter().map(|path| path_key(path)).collect();
    let mut destinations = HashSet::new();
    let mut previews = Vec::with_capacity(paths.len());

    for (index, source) in paths.iter().enumerate() {
        let original_name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_directory = source.is_dir();
        let extension = if is_directory {
            None
        } else {
            source
                .extension()
                .map(|value| value.to_string_lossy().to_string())
        };
        let base = if extension.is_some() {
            source
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            original_name.clone()
        };
        let replaced = if rule.find.is_empty() {
            base
        } else {
            base.replace(&rule.find, &rule.replace)
        };
        let mut new_base = format!("{}{}{}", rule.prefix, replaced, rule.suffix);
        if rule.sequence_enabled {
            let number = rule.sequence_start.saturating_add(index as u32);
            new_base.push('-');
            new_base.push_str(&format!(
                "{number:0width$}",
                width = rule.sequence_padding.clamp(1, 12)
            ));
        }
        let new_name = extension
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{new_base}.{extension}"))
            .unwrap_or(new_base);
        let destination = source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&new_name);
        let changed = path_key(source) != path_key(&destination);
        let mut error = validate_windows_name(&new_name).err();
        let destination_key = path_key(&destination);
        if error.is_none() && !destinations.insert(destination_key.clone()) {
            error = Some("Two items would receive the same name.".into());
        }
        if error.is_none() && destination.exists() && !sources.contains(&destination_key) {
            error = Some("Another item already uses this name.".into());
        }
        previews.push(BatchRenamePreview {
            source: source.clone(),
            destination,
            error,
            changed,
        });
    }
    previews
}

pub fn unique_destination(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let extension = path.extension().map(|value| value.to_string_lossy());
    for index in 2..100_000u32 {
        let name = extension
            .as_ref()
            .map(|extension| format!("{stem} ({index}).{extension}"))
            .unwrap_or_else(|| format!("{stem} ({index})"));
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

fn execute_batch_rename(
    preview: &[BatchRenamePreview],
    cancellation: &AtomicBool,
    mut progress: impl FnMut(usize, &Path),
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if let Some(error) = preview.iter().find_map(|item| item.error.as_ref()) {
        return Err(format!("Batch rename preview contains an error: {error}"));
    }
    let changes: Vec<_> = preview.iter().filter(|item| item.changed).collect();
    if changes.is_empty() {
        return Err("Batch rename would not change any names.".into());
    }
    let token = unique_token();
    let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::with_capacity(changes.len());
    for (index, item) in changes.iter().enumerate() {
        if cancellation.load(Ordering::Relaxed) {
            rollback_staged_renames(&staged, 0);
            return Err("Batch rename cancelled.".into());
        }
        let parent = item
            .source
            .parent()
            .ok_or_else(|| "A selected item has no parent directory.".to_string())?;
        let temporary = parent.join(format!(".LuxePix-rename-{token}-{index}.tmp"));
        if temporary.exists() {
            rollback_staged_renames(&staged, 0);
            return Err("Could not reserve a temporary batch rename path.".into());
        }
        if let Err(error) = fs::rename(&item.source, &temporary) {
            rollback_staged_renames(&staged, 0);
            return Err(format!(
                "Could not stage {} for rename: {error}",
                item.source.display()
            ));
        }
        staged.push((item.source.clone(), item.destination.clone(), temporary));
    }

    let mut completed = 0usize;
    for (source, destination, temporary) in &staged {
        if cancellation.load(Ordering::Relaxed) {
            rollback_staged_renames(&staged, completed);
            return Err("Batch rename cancelled.".into());
        }
        if let Err(error) = fs::rename(temporary, destination) {
            rollback_staged_renames(&staged, completed);
            return Err(format!(
                "Could not rename {} to {}: {error}",
                source.display(),
                destination.display()
            ));
        }
        completed += 1;
        progress(completed, destination);
    }

    Ok(staged
        .into_iter()
        .map(|(source, destination, _)| (source, destination))
        .collect())
}

fn rollback_staged_renames(staged: &[(PathBuf, PathBuf, PathBuf)], completed: usize) {
    for (_, destination, temporary) in staged.iter().take(completed).rev() {
        let _ = fs::rename(destination, temporary);
    }
    for (source, _, temporary) in staged.iter().rev() {
        if temporary.exists() {
            let _ = fs::rename(temporary, source);
        }
    }
}

fn find_duplicates(
    root: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(usize, usize, Option<&Path>),
) -> Result<Vec<DuplicateGroup>, String> {
    if !root.is_dir() {
        return Err(format!(
            "Duplicate search root is unavailable: {}",
            root.display()
        ));
    }
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for entry in WalkDir::new(root).follow_links(false) {
        if cancellation.load(Ordering::Relaxed) {
            return Err("Duplicate search cancelled.".into());
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        by_size
            .entry(metadata.len())
            .or_default()
            .push(entry.path().to_path_buf());
    }
    let candidates: Vec<PathBuf> = by_size
        .into_values()
        .filter(|paths| paths.len() > 1)
        .flatten()
        .collect();
    let total = candidates.len();
    progress(0, total, None);
    let mut by_hash: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();
    for (index, path) in candidates.iter().enumerate() {
        if cancellation.load(Ordering::Relaxed) {
            return Err("Duplicate search cancelled.".into());
        }
        let size = fs::metadata(path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
            .len();
        let hash = sha256_file(path, cancellation)?;
        by_hash.entry((size, hash)).or_default().push(path.clone());
        progress(index + 1, total, Some(path));
    }
    let mut groups: Vec<DuplicateGroup> = by_hash
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|((size_bytes, sha256), mut paths)| {
            paths.sort_by_key(|path| path_key(path));
            DuplicateGroup {
                size_bytes,
                sha256,
                paths,
            }
        })
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.wasted_bytes()));
    Ok(groups)
}

#[derive(Clone)]
struct ArchiveEntry {
    source: PathBuf,
    name: String,
    directory: bool,
}

fn create_zip_archive(
    paths: &[PathBuf],
    destination: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(usize, usize, Option<&Path>),
) -> Result<PathBuf, String> {
    if paths.is_empty() {
        return Err("Select at least one item to create a ZIP archive.".into());
    }
    if destination.exists() {
        return Err(format!(
            "Archive destination already exists: {}",
            destination.display()
        ));
    }
    let base = common_selection_parent(paths)?;
    let mut entries = Vec::new();
    for selected in paths {
        if selected.is_dir() {
            for entry in WalkDir::new(selected).follow_links(false) {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry.file_type().is_symlink() {
                    continue;
                }
                let relative = entry
                    .path()
                    .strip_prefix(&base)
                    .map_err(|error| error.to_string())?;
                entries.push(ArchiveEntry {
                    source: entry.path().to_path_buf(),
                    name: zip_name(relative)?,
                    directory: entry.file_type().is_dir(),
                });
            }
        } else if selected.is_file() {
            let relative = selected
                .strip_prefix(&base)
                .map_err(|error| error.to_string())?;
            entries.push(ArchiveEntry {
                source: selected.clone(),
                name: zip_name(relative)?,
                directory: false,
            });
        } else {
            return Err(format!(
                "Selected item is unavailable: {}",
                selected.display()
            ));
        }
    }
    let total = entries.len();
    progress(0, total, None);
    let part_path = destination.with_extension("zip.part");
    let _ = fs::remove_file(&part_path);
    let result = (|| {
        let file = fs::File::create(&part_path).map_err(|error| error.to_string())?;
        let mut archive = ZipWriter::new(file);
        let file_options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let directory_options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut buffer = [0u8; 64 * 1024];
        for (index, entry) in entries.iter().enumerate() {
            if cancellation.load(Ordering::Relaxed) {
                return Err("ZIP creation cancelled.".to_string());
            }
            if entry.directory {
                archive
                    .add_directory(
                        format!("{}/", entry.name.trim_end_matches('/')),
                        directory_options,
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                archive
                    .start_file(&entry.name, file_options)
                    .map_err(|error| error.to_string())?;
                let mut source =
                    fs::File::open(&entry.source).map_err(|error| error.to_string())?;
                loop {
                    if cancellation.load(Ordering::Relaxed) {
                        return Err("ZIP creation cancelled.".to_string());
                    }
                    let read = source
                        .read(&mut buffer)
                        .map_err(|error| error.to_string())?;
                    if read == 0 {
                        break;
                    }
                    archive
                        .write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                }
            }
            progress(index + 1, total, Some(&entry.source));
        }
        let file = archive.finish().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&part_path);
        return Err(error);
    }
    fs::rename(&part_path, destination).map_err(|error| error.to_string())?;
    Ok(destination.to_path_buf())
}

fn extract_zip_archive(
    archive_path: &Path,
    destination: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(usize, usize, Option<&Path>),
) -> Result<PathBuf, String> {
    if destination.exists() {
        return Err(format!(
            "Extraction destination already exists: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let result = (|| {
        let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
        let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
        let total = archive.len();
        progress(0, total, None);
        for index in 0..total {
            if cancellation.load(Ordering::Relaxed) {
                return Err("ZIP extraction cancelled.".to_string());
            }
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| format!("Unsafe path in ZIP archive: {}", entry.name()))?
                .to_path_buf();
            if relative.as_os_str().is_empty()
                || relative.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err(format!("Unsafe path in ZIP archive: {}", entry.name()));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(format!(
                    "ZIP symbolic links are not allowed: {}",
                    entry.name()
                ));
            }
            let output = destination.join(&relative);
            if entry.is_dir() {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            } else {
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
                std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
                target.sync_all().map_err(|error| error.to_string())?;
            }
            progress(index + 1, total, Some(&output));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    Ok(destination.to_path_buf())
}

fn analyze_folder_sizes(
    root: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(usize, usize, Option<&Path>),
) -> Result<Vec<FolderSizeEntry>, String> {
    if !root.is_dir() {
        return Err(format!(
            "Folder size root is unavailable: {}",
            root.display()
        ));
    }
    let mut folders: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir() && !file_type.is_symlink())
                .map(|_| entry.path())
        })
        .collect();
    folders.sort_by_key(|path| path_key(path));
    let total = folders.len();
    progress(0, total, None);
    let mut entries = Vec::with_capacity(total);
    for (index, folder) in folders.into_iter().enumerate() {
        if cancellation.load(Ordering::Relaxed) {
            return Err("Folder size analysis cancelled.".into());
        }
        let mut size_bytes = 0u64;
        let mut file_count = 0usize;
        for entry in WalkDir::new(&folder).follow_links(false) {
            if cancellation.load(Ordering::Relaxed) {
                return Err("Folder size analysis cancelled.".into());
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if entry.file_type().is_file() {
                if let Ok(metadata) = entry.metadata() {
                    size_bytes = size_bytes.saturating_add(metadata.len());
                    file_count = file_count.saturating_add(1);
                }
            }
        }
        entries.push(FolderSizeEntry {
            path: folder.clone(),
            size_bytes,
            file_count,
        });
        progress(index + 1, total, Some(&folder));
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.size_bytes));
    Ok(entries)
}

fn common_selection_parent(paths: &[PathBuf]) -> Result<PathBuf, String> {
    let parent = paths[0]
        .parent()
        .ok_or_else(|| "A selected item has no parent directory.".to_string())?
        .to_path_buf();
    if paths
        .iter()
        .skip(1)
        .any(|path| path.parent() != Some(parent.as_path()))
    {
        return Err("ZIP creation requires selected items from the same folder.".into());
    }
    Ok(parent)
}

fn zip_name(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("Unsafe ZIP entry path: {}", path.display()));
    }
    let value = path.to_string_lossy().replace('\\', "/");
    if value.is_empty() || value.starts_with('/') {
        return Err(format!("Unsafe ZIP entry path: {}", path.display()));
    }
    Ok(value)
}

fn validate_windows_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || name == "." || name == ".." {
        return Err("The generated name is empty or reserved.".into());
    }
    if name.ends_with([' ', '.']) {
        return Err("Windows names cannot end with a space or period.".into());
    }
    if name
        .chars()
        .any(|character| character < ' ' || r#"<>:"/\|?*"#.contains(character))
    {
        return Err("The generated name contains a Windows-invalid character.".into());
    }
    let stem = Path::new(name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        return Err("The generated name is reserved by Windows.".into());
    }
    Ok(())
}

fn sha256_file(path: &Path, cancellation: &AtomicBool) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancellation.load(Ordering::Relaxed) {
            return Err("Duplicate search cancelled.".into());
        }
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn unique_token() -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{time}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_folder_sizes, create_zip_archive, execute_batch_rename, extract_zip_archive,
        find_duplicates, preview_batch_rename, BatchRenameRule,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "luxepix-power-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn batch_rename_preview_preserves_extensions_and_executes() {
        let root = test_root("rename");
        fs::create_dir_all(&root).expect("create root");
        let first = root.join("old-1.jpg");
        let second = root.join("old-2.png");
        fs::write(&first, b"one").expect("write first");
        fs::write(&second, b"two").expect("write second");
        let rule = BatchRenameRule {
            find: "old".into(),
            replace: "new".into(),
            prefix: "SKU-".into(),
            sequence_enabled: true,
            sequence_start: 7,
            sequence_padding: 2,
            ..BatchRenameRule::default()
        };
        let preview = preview_batch_rename(&[first, second], &rule);
        assert!(preview.iter().all(|item| item.error.is_none()));
        execute_batch_rename(&preview, &AtomicBool::new(false), |_, _| {}).expect("execute rename");
        assert!(root.join("SKU-new-1-07.jpg").exists());
        assert!(root.join("SKU-new-2-08.png").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn duplicate_finder_groups_equal_content() {
        let root = test_root("duplicates");
        fs::create_dir_all(root.join("a")).expect("create a");
        fs::create_dir_all(root.join("b")).expect("create b");
        fs::write(root.join("a").join("one.jpg"), b"same").expect("write one");
        fs::write(root.join("b").join("two.jpg"), b"same").expect("write two");
        fs::write(root.join("unique.jpg"), b"different").expect("write unique");
        let groups =
            find_duplicates(&root, &AtomicBool::new(false), |_, _, _| {}).expect("find duplicates");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn zip_round_trip_uses_safe_relative_paths() {
        let root = test_root("zip");
        let source = root.join("source");
        fs::create_dir_all(source.join("folder")).expect("create source");
        fs::write(source.join("folder").join("image.jpg"), b"image").expect("write image");
        let archive = root.join("archive.zip");
        create_zip_archive(
            &[source.join("folder")],
            &archive,
            &AtomicBool::new(false),
            |_, _, _| {},
        )
        .expect("create archive");
        let extracted = root.join("extracted");
        extract_zip_archive(&archive, &extracted, &AtomicBool::new(false), |_, _, _| {})
            .expect("extract archive");
        assert_eq!(
            fs::read(extracted.join("folder").join("image.jpg")).expect("read image"),
            b"image"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn folder_size_analysis_sorts_largest_first() {
        let root = test_root("sizes");
        fs::create_dir_all(root.join("small")).expect("create small");
        fs::create_dir_all(root.join("large")).expect("create large");
        fs::write(root.join("small").join("one.bin"), [0u8; 2]).expect("write small");
        fs::write(root.join("large").join("two.bin"), [0u8; 9]).expect("write large");
        let entries = analyze_folder_sizes(&root, &AtomicBool::new(false), |_, _, _| {})
            .expect("analyze sizes");
        assert_eq!(
            entries[0].path.file_name().and_then(|name| name.to_str()),
            Some("large")
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
