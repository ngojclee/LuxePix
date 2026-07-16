use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::model::{FileEntry, FolderEntry};
use crate::scanner::{self, ScanOptions};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaskId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind {
    ScanRoot,
    ScanFolder,
}

#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub kind: TaskKind,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum TaskEvent {
    RootScanned {
        id: TaskId,
        path: PathBuf,
        folders: Vec<FolderEntry>,
    },
    FolderScanned {
        id: TaskId,
        path: PathBuf,
        files: Vec<FileEntry>,
    },
    Failed {
        id: TaskId,
        kind: TaskKind,
        path: PathBuf,
        error: String,
    },
}

impl TaskEvent {
    fn id(&self) -> TaskId {
        match self {
            Self::RootScanned { id, .. }
            | Self::FolderScanned { id, .. }
            | Self::Failed { id, .. } => *id,
        }
    }
}

pub struct TaskManager {
    pool: rayon::ThreadPool,
    event_tx: mpsc::Sender<TaskEvent>,
    event_rx: mpsc::Receiver<TaskEvent>,
    active: HashMap<TaskId, TaskInfo>,
    next_id: u64,
}

impl TaskManager {
    pub fn new(threads: usize) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .thread_name(|index| format!("luxepix-task-{index}"))
            .build()
            .expect("build LuxePix task pool");

        Self {
            pool,
            event_tx,
            event_rx,
            active: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn scan_root(&mut self, path: &Path, options: ScanOptions) -> TaskId {
        let id = self.start(TaskKind::ScanRoot, path);
        let path = path.to_path_buf();
        let event_path = path.clone();
        let tx = self.event_tx.clone();
        self.pool.spawn(move || {
            let event = match scanner::scan_directory(&path, options) {
                Ok(folders) => TaskEvent::RootScanned {
                    id,
                    path: event_path,
                    folders,
                },
                Err(error) => TaskEvent::Failed {
                    id,
                    kind: TaskKind::ScanRoot,
                    path: event_path,
                    error: error.to_string(),
                },
            };
            let _ = tx.send(event);
        });
        id
    }

    pub fn scan_folder(&mut self, path: &Path, options: ScanOptions) -> TaskId {
        let id = self.start(TaskKind::ScanFolder, path);
        let path = path.to_path_buf();
        let event_path = path.clone();
        let tx = self.event_tx.clone();
        self.pool.spawn(move || {
            let event = match scanner::list_all_files(&path, options) {
                Ok(files) => TaskEvent::FolderScanned {
                    id,
                    path: event_path,
                    files,
                },
                Err(error) => TaskEvent::Failed {
                    id,
                    kind: TaskKind::ScanFolder,
                    path: event_path,
                    error: error.to_string(),
                },
            };
            let _ = tx.send(event);
        });
        id
    }

    pub fn drain(&mut self, limit: usize) -> Vec<TaskEvent> {
        let mut events = Vec::new();
        for _ in 0..limit {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    self.active.remove(&event.id());
                    events.push(event);
                }
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn active_tasks(&self) -> impl Iterator<Item = (&TaskId, &TaskInfo)> {
        self.active.iter()
    }

    fn start(&mut self, kind: TaskKind, path: &Path) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.active.insert(
            id,
            TaskInfo {
                kind,
                path: path.to_path_buf(),
            },
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskEvent, TaskManager};
    use crate::scanner::ScanOptions;
    use std::fs;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn root_scan_finishes_through_the_typed_event_channel() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("luxepix-task-scan-{}-{unique}", std::process::id()));
        let product = root.join("1001-product");
        fs::create_dir_all(&product).expect("create product directory");
        fs::write(product.join("preview.jpg"), b"not decoded during scan")
            .expect("write scan fixture");

        let mut tasks = TaskManager::new(1);
        let expected_id = tasks.scan_root(&root, ScanOptions::default());
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut result = None;
        while Instant::now() < deadline {
            if let Some(event) = tasks.drain(8).into_iter().next() {
                result = Some(event);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        match result.expect("background scan event") {
            TaskEvent::RootScanned { id, folders, .. } => {
                assert_eq!(id, expected_id);
                assert_eq!(folders.len(), 1);
                assert_eq!(folders[0].image_count, 1);
                assert_eq!(folders[0].id.as_path(), product);
            }
            other => panic!("unexpected task event: {other:?}"),
        }
        assert_eq!(tasks.active_count(), 0);
        fs::remove_dir_all(root).expect("remove task fixture");
    }
}
