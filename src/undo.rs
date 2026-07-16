use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::file_ops::{self, TransferMode, TransferOutcome};

const DEFAULT_UNDO_LIMIT: usize = 50;

#[derive(Clone, Debug)]
pub struct UndoEntry {
    pub label: String,
    action: UndoAction,
}

#[derive(Clone, Debug)]
enum UndoAction {
    Rename {
        current_path: PathBuf,
        original_path: PathBuf,
    },
    RemoveCreated {
        paths: Vec<PathBuf>,
    },
    MoveBack {
        pairs: Vec<(PathBuf, PathBuf)>,
    },
}

pub struct UndoManager {
    entries: VecDeque<UndoEntry>,
    limit: usize,
}

impl UndoManager {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            limit: limit.max(1),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn next_label(&self) -> Option<&str> {
        self.entries.back().map(|entry| entry.label.as_str())
    }

    pub fn push_rename(&mut self, original_path: PathBuf, current_path: PathBuf) {
        self.push(UndoEntry {
            label: "Undo Rename".into(),
            action: UndoAction::Rename {
                current_path,
                original_path,
            },
        });
    }

    pub fn push_new_folder(&mut self, path: PathBuf) {
        self.push(UndoEntry {
            label: "Undo New Folder".into(),
            action: UndoAction::RemoveCreated { paths: vec![path] },
        });
    }

    pub fn push_transfer(&mut self, outcome: &TransferOutcome) -> bool {
        if outcome.completed.is_empty()
            || outcome.completed.iter().any(|item| item.replaced_existing)
        {
            return false;
        }

        let mode = outcome.completed[0].mode;
        if outcome.completed.iter().any(|item| item.mode != mode) {
            return false;
        }
        let count = outcome.completed.len();
        let entry = match mode {
            TransferMode::Copy => UndoEntry {
                label: format!("Undo Copy ({count} item(s))"),
                action: UndoAction::RemoveCreated {
                    paths: outcome
                        .completed
                        .iter()
                        .map(|item| item.destination.clone())
                        .collect(),
                },
            },
            TransferMode::Move => UndoEntry {
                label: format!("Undo Move ({count} item(s))"),
                action: UndoAction::MoveBack {
                    pairs: outcome
                        .completed
                        .iter()
                        .map(|item| (item.destination.clone(), item.source.clone()))
                        .collect(),
                },
            },
        };
        self.push(entry);
        true
    }

    pub fn undo_latest(&mut self) -> io::Result<String> {
        let Some(entry) = self.entries.pop_back() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "there is no file operation to undo",
            ));
        };
        let label = entry.label.clone();
        if let Err(error) = execute(&entry.action) {
            self.entries.push_back(entry);
            return Err(error);
        }
        Ok(label)
    }

    fn push(&mut self, entry: UndoEntry) {
        if self.limit == 0 {
            self.limit = DEFAULT_UNDO_LIMIT;
        }
        self.entries.push_back(entry);
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
    }
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new(DEFAULT_UNDO_LIMIT)
    }
}

fn execute(action: &UndoAction) -> io::Result<()> {
    match action {
        UndoAction::Rename {
            current_path,
            original_path,
        } => {
            preflight_move(current_path, original_path)?;
            file_ops::move_path_for_undo(current_path, original_path)
        }
        UndoAction::RemoveCreated { paths } => {
            let mut failures = Vec::new();
            for path in paths.iter().rev() {
                if !path.exists() {
                    continue;
                }
                if let Err(error) = file_ops::remove_path(path) {
                    failures.push(format!("{}: {error}", path.display()));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(io::Error::other(failures.join("; ")))
            }
        }
        UndoAction::MoveBack { pairs } => {
            for (current_path, original_path) in pairs {
                preflight_move(current_path, original_path)?;
            }

            let mut completed: Vec<(PathBuf, PathBuf)> = Vec::new();
            for (current_path, original_path) in pairs.iter().rev() {
                if let Err(error) = file_ops::move_path_for_undo(current_path, original_path) {
                    for (restored, prior_path) in completed.into_iter().rev() {
                        let _ = file_ops::move_path_for_undo(&restored, &prior_path);
                    }
                    return Err(error);
                }
                completed.push((original_path.to_path_buf(), current_path.to_path_buf()));
            }
            Ok(())
        }
    }
}

fn preflight_move(
    current_path: &std::path::Path,
    original_path: &std::path::Path,
) -> io::Result<()> {
    if !current_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("undo source no longer exists: {}", current_path.display()),
        ));
    }
    if original_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("undo destination is occupied: {}", original_path.display()),
        ));
    }
    if let Some(parent) = original_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::UndoManager;
    use crate::file_ops::{
        execute_transfer, prepare_transfer, CompletedTransfer, OperationControl, TransferMode,
        TransferOutcome,
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
            "luxepix-undo-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create undo fixture");
        root
    }

    #[test]
    fn rename_undo_restores_the_original_path() {
        let root = test_root("rename");
        let original = root.join("before.txt");
        let current = root.join("after.txt");
        fs::write(&current, b"content").expect("write renamed file");
        let mut undo = UndoManager::new(10);
        undo.push_rename(original.clone(), current.clone());

        undo.undo_latest().expect("undo rename");

        assert!(original.exists());
        assert!(!current.exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn copy_undo_removes_created_destinations() {
        let root = test_root("copy");
        let source = root.join("source.txt");
        let destination = root.join("copied.txt");
        fs::write(&source, b"source").expect("write source");
        fs::write(&destination, b"copy").expect("write destination");
        let outcome = TransferOutcome {
            completed: vec![CompletedTransfer {
                source,
                destination: destination.clone(),
                mode: TransferMode::Copy,
                replaced_existing: false,
            }],
            ..TransferOutcome::default()
        };
        let mut undo = UndoManager::new(10);

        assert!(undo.push_transfer(&outcome));
        undo.undo_latest().expect("undo copy");

        assert!(!destination.exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn move_undo_returns_items_to_their_sources() {
        let root = test_root("move");
        let source = root.join("source.txt");
        let destination = root.join("destination.txt");
        fs::write(&destination, b"moved").expect("write moved file");
        let outcome = TransferOutcome {
            completed: vec![CompletedTransfer {
                source: source.clone(),
                destination: destination.clone(),
                mode: TransferMode::Move,
                replaced_existing: false,
            }],
            ..TransferOutcome::default()
        };
        let mut undo = UndoManager::new(10);

        assert!(undo.push_transfer(&outcome));
        undo.undo_latest().expect("undo move");

        assert!(source.exists());
        assert!(!destination.exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn bounded_history_drops_the_oldest_entry() {
        let root = test_root("bounded");
        let mut undo = UndoManager::new(1);
        let old = root.join("old");
        let newest = root.join("newest");
        fs::create_dir(&old).expect("create old folder");
        fs::create_dir(&newest).expect("create newest folder");
        undo.push_new_folder(old.clone());
        undo.push_new_folder(newest.clone());

        undo.undo_latest().expect("undo newest");

        assert!(old.exists());
        assert!(!newest.exists());
        assert!(!undo.can_undo());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn temp_directory_copy_and_undo_complete_a_round_trip() {
        let root = test_root("copy-round-trip");
        let source_root = root.join("source");
        let target = root.join("target");
        let source = source_root.join("product");
        fs::create_dir_all(source.join("nested")).expect("create source tree");
        fs::create_dir_all(&target).expect("create target");
        fs::write(source.join("nested").join("photo.jpg"), b"content").expect("write source file");
        let preparation =
            prepare_transfer(&[source], &target, TransferMode::Copy).expect("prepare copy");
        let request = preparation.resolve(&HashMap::new()).expect("resolve copy");
        let outcome = execute_transfer(&request, &OperationControl::default(), |_| {});
        let copied = target.join("product");
        assert!(copied.join("nested").join("photo.jpg").exists());
        let mut undo = UndoManager::new(10);
        assert!(undo.push_transfer(&outcome));

        undo.undo_latest().expect("undo copied tree");

        assert!(!copied.exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
