use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferMode {
    Copy,
    Move,
}

impl TransferMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Move => "Move",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollisionChoice {
    Replace,
    KeepBoth,
    Skip,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferCandidate {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub collision: bool,
}

#[derive(Clone, Debug)]
pub struct TransferPreparation {
    pub mode: TransferMode,
    pub target_dir: PathBuf,
    pub candidates: Vec<TransferCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferItem {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub replace_existing: bool,
}

#[derive(Clone, Debug)]
pub struct TransferRequest {
    pub mode: TransferMode,
    pub target_dir: PathBuf,
    pub items: Vec<TransferItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedTransfer {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub mode: TransferMode,
    pub replaced_existing: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct TransferOutcome {
    pub completed: Vec<CompletedTransfer>,
    pub errors: Vec<OperationError>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DeleteOutcome {
    pub deleted: Vec<PathBuf>,
    pub errors: Vec<OperationError>,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum FileOpProgress {
    Prepared {
        total_bytes: u64,
        total_items: usize,
    },
    ItemStarted {
        path: PathBuf,
    },
    Bytes(u64),
    ItemCompleted,
    ItemError(OperationError),
}

#[derive(Default)]
pub struct OperationControl {
    cancelled: AtomicBool,
    paused: AtomicBool,
}

impl OperationControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn checkpoint(&self) -> io::Result<()> {
        while self.is_paused() && !self.is_cancelled() {
            thread::sleep(Duration::from_millis(25));
        }
        if self.is_cancelled() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "operation cancelled",
            ))
        } else {
            Ok(())
        }
    }
}

pub fn prepare_transfer(
    sources: &[PathBuf],
    target_dir: &Path,
    mode: TransferMode,
) -> io::Result<TransferPreparation> {
    if !target_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("paste target is not a directory: {}", target_dir.display()),
        ));
    }

    let mut reserved = HashSet::new();
    let mut candidates = Vec::with_capacity(sources.len());
    for source in sources {
        let name = source.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("source has no file name: {}", source.display()),
            )
        })?;
        let destination = target_dir.join(name);
        if same_path(source, &destination) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("source and destination are the same: {}", source.display()),
            ));
        }
        if source.is_dir() && destination.starts_with(source) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("cannot place a folder inside itself: {}", source.display()),
            ));
        }

        let key = normalized_path_key(&destination);
        let collision = destination.exists() || !reserved.insert(key);
        candidates.push(TransferCandidate {
            source: source.clone(),
            destination,
            collision,
        });
    }

    Ok(TransferPreparation {
        mode,
        target_dir: target_dir.to_path_buf(),
        candidates,
    })
}

impl TransferPreparation {
    pub fn collision_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.collision)
            .count()
    }

    pub fn resolve(
        &self,
        choices: &HashMap<PathBuf, CollisionChoice>,
    ) -> io::Result<TransferRequest> {
        let mut reserved: HashSet<_> = self
            .candidates
            .iter()
            .map(|candidate| normalized_path_key(&candidate.destination))
            .collect();
        let mut items = Vec::with_capacity(self.candidates.len());

        for candidate in &self.candidates {
            let choice = if candidate.collision {
                *choices.get(&candidate.destination).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "missing collision decision for {}",
                            candidate.destination.display()
                        ),
                    )
                })?
            } else {
                CollisionChoice::Replace
            };

            match choice {
                CollisionChoice::Skip => {}
                CollisionChoice::Replace => {
                    reserved.insert(normalized_path_key(&candidate.destination));
                    items.push(TransferItem {
                        source: candidate.source.clone(),
                        destination: candidate.destination.clone(),
                        replace_existing: candidate.collision,
                    });
                }
                CollisionChoice::KeepBoth => {
                    let destination = keep_both_destination_reserved(
                        &candidate.destination,
                        candidate.source.is_dir(),
                        &reserved,
                    );
                    reserved.insert(normalized_path_key(&destination));
                    items.push(TransferItem {
                        source: candidate.source.clone(),
                        destination,
                        replace_existing: false,
                    });
                }
            }
        }

        Ok(TransferRequest {
            mode: self.mode,
            target_dir: self.target_dir.clone(),
            items,
        })
    }
}

#[cfg(test)]
fn keep_both_destination(path: &Path, is_directory: bool) -> PathBuf {
    keep_both_destination_reserved(path, is_directory, &HashSet::new())
}

pub fn create_unique_folder(parent: &Path) -> io::Result<PathBuf> {
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("new-folder target is not a directory: {}", parent.display()),
        ));
    }

    for number in 1..10_000 {
        let name = if number == 1 {
            "New folder".to_string()
        } else {
            format!("New folder ({number})")
        };
        let candidate = parent.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not find an available New folder name",
    ))
}

pub fn execute_transfer(
    request: &TransferRequest,
    control: &OperationControl,
    mut progress: impl FnMut(FileOpProgress),
) -> TransferOutcome {
    let sizes: Vec<u64> = request
        .items
        .iter()
        .map(|item| estimate_size(&item.source, control).unwrap_or(0))
        .collect();
    progress(FileOpProgress::Prepared {
        total_bytes: sizes.iter().sum(),
        total_items: request.items.len(),
    });

    let mut outcome = TransferOutcome::default();
    for (item, item_size) in request.items.iter().zip(sizes) {
        if control.checkpoint().is_err() {
            outcome.cancelled = true;
            break;
        }
        progress(FileOpProgress::ItemStarted {
            path: item.source.clone(),
        });

        match execute_transfer_item(item, request.mode, item_size, control, &mut progress) {
            Ok(()) => {
                outcome.completed.push(CompletedTransfer {
                    source: item.source.clone(),
                    destination: item.destination.clone(),
                    mode: request.mode,
                    replaced_existing: item.replace_existing,
                });
                progress(FileOpProgress::ItemCompleted);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted && control.is_cancelled() => {
                outcome.cancelled = true;
                break;
            }
            Err(error) => {
                let operation_error = OperationError {
                    path: item.source.clone(),
                    message: error.to_string(),
                };
                progress(FileOpProgress::ItemError(operation_error.clone()));
                outcome.errors.push(operation_error);
            }
        }
    }
    outcome
}

pub fn delete_permanently(
    paths: &[PathBuf],
    control: &OperationControl,
    mut progress: impl FnMut(FileOpProgress),
) -> DeleteOutcome {
    let total_bytes = paths
        .iter()
        .map(|path| estimate_size(path, control).unwrap_or(0))
        .sum();
    progress(FileOpProgress::Prepared {
        total_bytes,
        total_items: paths.len(),
    });

    let mut outcome = DeleteOutcome::default();
    for path in paths {
        if control.checkpoint().is_err() {
            outcome.cancelled = true;
            break;
        }
        progress(FileOpProgress::ItemStarted { path: path.clone() });
        match delete_path_with_progress(path, control, &mut progress) {
            Ok(()) => {
                outcome.deleted.push(path.clone());
                progress(FileOpProgress::ItemCompleted);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted && control.is_cancelled() => {
                outcome.cancelled = true;
                break;
            }
            Err(error) => {
                let operation_error = OperationError {
                    path: path.clone(),
                    message: error.to_string(),
                };
                progress(FileOpProgress::ItemError(operation_error.clone()));
                outcome.errors.push(operation_error);
            }
        }
    }
    outcome
}

pub fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn move_path_for_undo(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("undo destination already exists: {}", destination.display()),
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let control = OperationControl::default();
            let item = TransferItem {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
                replace_existing: false,
            };
            execute_transfer_item(
                &item,
                TransferMode::Move,
                estimate_size(source, &control).unwrap_or(0),
                &control,
                &mut |_| {},
            )
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("rename failed ({rename_error}); fallback failed: {error}"),
                )
            })
        }
    }
}

fn execute_transfer_item(
    item: &TransferItem,
    mode: TransferMode,
    item_size: u64,
    control: &OperationControl,
    progress: &mut impl FnMut(FileOpProgress),
) -> io::Result<()> {
    if !item.source.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("source no longer exists: {}", item.source.display()),
        ));
    }
    if item.destination.exists() && !item.replace_existing {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", item.destination.display()),
        ));
    }
    if let Some(parent) = item.destination.parent() {
        fs::create_dir_all(parent)?;
    }

    if mode == TransferMode::Move && !item.replace_existing {
        match fs::rename(&item.source, &item.destination) {
            Ok(()) => {
                if item_size > 0 {
                    progress(FileOpProgress::Bytes(item_size));
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(error),
            Err(_) => {}
        }
    }

    let staging = temporary_destination(&item.destination);
    let copy_result = copy_path_streamed(&item.source, &staging, control, progress);
    if let Err(error) = copy_result {
        let _ = remove_path_if_exists(&staging);
        return Err(error);
    }
    if let Err(error) = control.checkpoint() {
        let _ = remove_path_if_exists(&staging);
        return Err(error);
    }

    if item.destination.exists() {
        if item.replace_existing {
            if let Err(error) = remove_path(&item.destination) {
                let _ = remove_path_if_exists(&staging);
                return Err(error);
            }
        } else {
            let _ = remove_path_if_exists(&staging);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "destination appeared during transfer: {}",
                    item.destination.display()
                ),
            ));
        }
    }

    if let Err(error) = fs::rename(&staging, &item.destination) {
        let _ = remove_path_if_exists(&staging);
        return Err(error);
    }

    if mode == TransferMode::Move {
        remove_path(&item.source).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("destination was created, but the original could not be removed: {error}"),
            )
        })?;
    }
    Ok(())
}

fn copy_path_streamed(
    source: &Path,
    destination: &Path,
    control: &OperationControl,
    progress: &mut impl FnMut(FileOpProgress),
) -> io::Result<()> {
    control.checkpoint()?;
    let metadata = fs::symlink_metadata(source)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("symbolic links are not copied: {}", source.display()),
        ));
    }
    if file_type.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            control.checkpoint()?;
            let entry = entry?;
            copy_path_streamed(
                &entry.path(),
                &destination.join(entry.file_name()),
                control,
                progress,
            )?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        Ok(())
    } else {
        copy_file_streamed(source, destination, &metadata, control, progress)
    }
}

fn copy_file_streamed(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
    control: &OperationControl,
    progress: &mut impl FnMut(FileOpProgress),
) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        control.checkpoint()?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        progress(FileOpProgress::Bytes(read as u64));
    }
    output.flush()?;
    fs::set_permissions(destination, metadata.permissions())?;
    Ok(())
}

fn delete_path_with_progress(
    path: &Path,
    control: &OperationControl,
    progress: &mut impl FnMut(FileOpProgress),
) -> io::Result<()> {
    control.checkpoint()?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            delete_path_with_progress(&entry.path(), control, progress)?;
        }
        fs::remove_dir(path)
    } else {
        let size = metadata.len();
        fs::remove_file(path)?;
        if size > 0 {
            progress(FileOpProgress::Bytes(size));
        }
        Ok(())
    }
}

fn estimate_size(path: &Path, control: &OperationControl) -> io::Result<u64> {
    control.checkpoint()?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        let mut total = 0_u64;
        for entry in fs::read_dir(path)? {
            total = total.saturating_add(estimate_size(&entry?.path(), control)?);
        }
        Ok(total)
    } else {
        Ok(metadata.len())
    }
}

fn keep_both_destination_reserved(
    path: &Path,
    is_directory: bool,
    reserved: &HashSet<String>,
) -> PathBuf {
    if !path.exists() && !reserved.contains(&normalized_path_key(path)) {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("item");
    let (stem, extension) = if is_directory {
        (file_name, None)
    } else {
        (
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("item"),
            path.extension().and_then(|extension| extension.to_str()),
        )
    };

    for number in 1..10_000 {
        let suffix = if number == 1 {
            " - Copy".to_string()
        } else {
            format!(" - Copy {number}")
        };
        let name = match extension {
            Some(extension) => format!("{stem}{suffix}.{extension}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() && !reserved.contains(&normalized_path_key(&candidate)) {
            return candidate;
        }
    }

    parent.join(format!(
        "{stem} - Copy {}",
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn temporary_destination(destination: &Path) -> PathBuf {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("item");
    loop {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{name}.luxepix-partial-{}-{counter}",
            std::process::id()
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
}

fn remove_path_if_exists(path: &Path) -> io::Result<()> {
    if path.exists() {
        remove_path(path)
    } else {
        Ok(())
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path_key(left) == normalized_path_key(right)
}

fn normalized_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        create_unique_folder, execute_transfer, keep_both_destination, prepare_transfer,
        CollisionChoice, FileOpProgress, OperationControl, TransferMode,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "luxepix-file-ops-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create file operation fixture");
        root
    }

    #[test]
    fn keep_both_preserves_file_extensions_and_directory_names() {
        let root = test_root("keep-both");
        let file = root.join("photo.jpg");
        let directory = root.join("product.2026");
        fs::write(&file, b"existing").expect("write existing file");
        fs::create_dir(&directory).expect("create existing directory");

        assert_eq!(
            keep_both_destination(&file, false),
            root.join("photo - Copy.jpg")
        );
        assert_eq!(
            keep_both_destination(&directory, true),
            root.join("product.2026 - Copy")
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn collision_resolution_supports_replace_keep_both_and_skip() {
        let root = test_root("collisions");
        let source_root = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&source_root).expect("create source");
        fs::create_dir_all(&target).expect("create target");
        let source = source_root.join("photo.jpg");
        fs::write(&source, b"new").expect("write source");
        fs::write(target.join("photo.jpg"), b"old").expect("write collision");

        let preparation =
            prepare_transfer(std::slice::from_ref(&source), &target, TransferMode::Copy)
                .expect("prepare");
        assert_eq!(preparation.collision_count(), 1);

        let mut choices = HashMap::new();
        choices.insert(target.join("photo.jpg"), CollisionChoice::KeepBoth);
        let request = preparation.resolve(&choices).expect("resolve keep both");
        assert_eq!(
            request.items[0].destination,
            target.join("photo - Copy.jpg")
        );
        assert!(!request.items[0].replace_existing);

        choices.insert(target.join("photo.jpg"), CollisionChoice::Replace);
        let request = preparation.resolve(&choices).expect("resolve replace");
        assert_eq!(request.items[0].destination, target.join("photo.jpg"));
        assert!(request.items[0].replace_existing);

        choices.insert(target.join("photo.jpg"), CollisionChoice::Skip);
        let request = preparation.resolve(&choices).expect("resolve skip");
        assert!(request.items.is_empty());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn keep_both_does_not_take_another_batch_items_destination() {
        let root = test_root("keep-both-batch");
        let source_root = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&source_root).expect("create source");
        fs::create_dir_all(&target).expect("create target");
        let photo = source_root.join("photo.jpg");
        let existing_copy_name = source_root.join("photo - Copy.jpg");
        fs::write(&photo, b"photo").expect("write photo");
        fs::write(&existing_copy_name, b"copy").expect("write second source");
        fs::write(target.join("photo.jpg"), b"old").expect("write collision");

        let preparation =
            prepare_transfer(&[photo, existing_copy_name], &target, TransferMode::Copy)
                .expect("prepare batch");
        let mut choices = HashMap::new();
        choices.insert(target.join("photo.jpg"), CollisionChoice::KeepBoth);

        let request = preparation.resolve(&choices).expect("resolve batch");

        assert_eq!(
            request.items[0].destination,
            target.join("photo - Copy 2.jpg")
        );
        assert_eq!(
            request.items[1].destination,
            target.join("photo - Copy.jpg")
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn streamed_copy_preserves_nested_content_and_reports_progress() {
        let root = test_root("stream-copy");
        let source_root = root.join("source");
        let source = source_root.join("product");
        let target = root.join("target");
        fs::create_dir_all(source.join("nested")).expect("create source tree");
        fs::create_dir_all(&target).expect("create target");
        fs::write(source.join("nested").join("image.jpg"), b"image bytes")
            .expect("write source file");

        let preparation =
            prepare_transfer(&[source], &target, TransferMode::Copy).expect("prepare transfer");
        let request = preparation
            .resolve(&HashMap::new())
            .expect("resolve transfer");
        let control = OperationControl::default();
        let mut bytes = 0_u64;
        let outcome = execute_transfer(&request, &control, |event| {
            if let FileOpProgress::Bytes(delta) = event {
                bytes += delta;
            }
        });

        assert!(!outcome.cancelled);
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.completed.len(), 1);
        assert_eq!(
            fs::read(target.join("product").join("nested").join("image.jpg"))
                .expect("read copied file"),
            b"image bytes"
        );
        assert_eq!(bytes, 11);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn cancellation_before_copy_does_not_create_destination() {
        let root = test_root("cancel");
        let source_root = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&source_root).expect("create source");
        fs::create_dir_all(&target).expect("create target");
        let source = source_root.join("photo.jpg");
        fs::write(&source, b"content").expect("write source");
        let preparation =
            prepare_transfer(&[source], &target, TransferMode::Copy).expect("prepare transfer");
        let request = preparation
            .resolve(&HashMap::new())
            .expect("resolve transfer");
        let control = OperationControl::default();
        control.cancel();

        let outcome = execute_transfer(&request, &control, |_| {});

        assert!(outcome.cancelled);
        assert!(!target.join("photo.jpg").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn new_folder_uses_windows_style_incrementing_names() {
        let root = test_root("new-folder");
        let first = create_unique_folder(&root).expect("create first folder");
        let second = create_unique_folder(&root).expect("create second folder");

        assert_eq!(first, root.join("New folder"));
        assert_eq!(second, root.join("New folder (2)"));
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
