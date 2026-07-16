use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::file_ops::{
    self, DeleteOutcome, FileOpProgress, OperationControl, OperationError, TransferOutcome,
    TransferRequest,
};
use crate::windows_shell;

const MAX_OPERATION_HISTORY: usize = 100;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(u64);

#[derive(Clone, Debug)]
pub enum OperationRequest {
    Transfer(TransferRequest),
    Recycle { paths: Vec<PathBuf> },
    DeletePermanent { paths: Vec<PathBuf> },
}

impl OperationRequest {
    pub fn title(&self) -> String {
        match self {
            Self::Transfer(request) => format!(
                "{} {} item(s) to {}",
                request.mode.label(),
                request.items.len(),
                request.target_dir.display()
            ),
            Self::Recycle { paths } => {
                format!("Recycle {} item(s)", paths.len())
            }
            Self::DeletePermanent { paths } => {
                format!("Delete {} item(s) permanently", paths.len())
            }
        }
    }

    pub const fn pausable(&self) -> bool {
        matches!(self, Self::Transfer(_) | Self::DeletePermanent { .. })
    }
}

#[derive(Clone, Debug)]
pub enum OperationOutcome {
    Transfer(TransferOutcome),
    Recycled(Vec<PathBuf>),
    Deleted(DeleteOutcome),
}

impl OperationOutcome {
    fn cancelled(&self) -> bool {
        match self {
            Self::Transfer(outcome) => outcome.cancelled,
            Self::Recycled(_) => false,
            Self::Deleted(outcome) => outcome.cancelled,
        }
    }

    fn has_errors(&self) -> bool {
        match self {
            Self::Transfer(outcome) => !outcome.errors.is_empty(),
            Self::Recycled(_) => false,
            Self::Deleted(outcome) => !outcome.errors.is_empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Running,
    Paused,
    Completed,
    CompletedWithErrors,
    Cancelled,
    Failed,
}

impl OperationState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::CompletedWithErrors => "Completed with errors",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
        }
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }

    pub const fn can_retry(self) -> bool {
        matches!(
            self,
            Self::CompletedWithErrors | Self::Cancelled | Self::Failed
        )
    }
}

pub struct OperationRecord {
    pub id: OperationId,
    pub title: String,
    pub state: OperationState,
    pub total_bytes: u64,
    pub processed_bytes: u64,
    pub total_items: usize,
    pub completed_items: usize,
    pub current_path: Option<PathBuf>,
    pub errors: Vec<OperationError>,
    request: OperationRequest,
    control: Arc<OperationControl>,
    started_at: Instant,
    finished_at: Option<Instant>,
}

impl OperationRecord {
    pub fn progress_fraction(&self) -> f32 {
        if self.total_bytes > 0 {
            (self.processed_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        } else if self.total_items > 0 {
            (self.completed_items as f64 / self.total_items as f64).clamp(0.0, 1.0) as f32
        } else if matches!(
            self.state,
            OperationState::Completed | OperationState::CompletedWithErrors
        ) {
            1.0
        } else {
            0.0
        }
    }

    pub fn speed_bytes_per_second(&self) -> Option<f64> {
        if self.processed_bytes == 0 {
            return None;
        }
        let end = self.finished_at.unwrap_or_else(Instant::now);
        let elapsed = end.duration_since(self.started_at).as_secs_f64();
        (elapsed > 0.0).then_some(self.processed_bytes as f64 / elapsed)
    }

    pub fn estimated_remaining(&self) -> Option<Duration> {
        let speed = self.speed_bytes_per_second()?;
        if speed <= 0.0 || self.total_bytes <= self.processed_bytes {
            return None;
        }
        Some(Duration::from_secs_f64(
            (self.total_bytes - self.processed_bytes) as f64 / speed,
        ))
    }

    pub fn pausable(&self) -> bool {
        self.request.pausable()
    }
}

pub struct OperationCompletion {
    pub id: OperationId,
    pub result: Result<OperationOutcome, String>,
}

enum OperationEvent {
    Prepared {
        id: OperationId,
        total_bytes: u64,
        total_items: usize,
    },
    ItemStarted {
        id: OperationId,
        path: PathBuf,
    },
    Bytes {
        id: OperationId,
        delta: u64,
    },
    ItemCompleted {
        id: OperationId,
    },
    ItemError {
        id: OperationId,
        error: OperationError,
    },
    Finished {
        id: OperationId,
        result: Result<OperationOutcome, String>,
    },
}

pub struct OperationManager {
    event_tx: mpsc::Sender<OperationEvent>,
    event_rx: mpsc::Receiver<OperationEvent>,
    records: VecDeque<OperationRecord>,
    next_id: u64,
}

impl Default for OperationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            event_tx,
            event_rx,
            records: VecDeque::new(),
            next_id: 1,
        }
    }

    pub fn start(&mut self, request: OperationRequest) -> Result<OperationId, String> {
        self.trim_history();
        let id = OperationId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let control = Arc::new(OperationControl::default());
        self.records.push_back(OperationRecord {
            id,
            title: request.title(),
            state: OperationState::Running,
            total_bytes: 0,
            processed_bytes: 0,
            total_items: 0,
            completed_items: 0,
            current_path: None,
            errors: Vec::new(),
            request: request.clone(),
            control: Arc::clone(&control),
            started_at: Instant::now(),
            finished_at: None,
        });

        let tx = self.event_tx.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("luxepix-operation-{}", id.0))
            .spawn(move || run_operation(id, request, control, tx));
        if let Err(error) = spawn_result {
            self.records.retain(|record| record.id != id);
            return Err(format!("could not start file operation: {error}"));
        }
        Ok(id)
    }

    pub fn process_events(&mut self, limit: usize) -> Vec<OperationCompletion> {
        let mut completions = Vec::new();
        for _ in 0..limit {
            let event = match self.event_rx.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            match event {
                OperationEvent::Prepared {
                    id,
                    total_bytes,
                    total_items,
                } => {
                    if let Some(record) = self.record_mut(id) {
                        record.total_bytes = total_bytes;
                        record.total_items = total_items;
                    }
                }
                OperationEvent::ItemStarted { id, path } => {
                    if let Some(record) = self.record_mut(id) {
                        record.current_path = Some(path);
                    }
                }
                OperationEvent::Bytes { id, delta } => {
                    if let Some(record) = self.record_mut(id) {
                        record.processed_bytes = record.processed_bytes.saturating_add(delta);
                    }
                }
                OperationEvent::ItemCompleted { id } => {
                    if let Some(record) = self.record_mut(id) {
                        record.completed_items = record.completed_items.saturating_add(1);
                    }
                }
                OperationEvent::ItemError { id, error } => {
                    if let Some(record) = self.record_mut(id) {
                        record.errors.push(error);
                    }
                }
                OperationEvent::Finished { id, result } => {
                    if let Some(record) = self.record_mut(id) {
                        if let Ok(outcome) = &result {
                            retain_retryable_work(record, outcome);
                        }
                        record.current_path = None;
                        record.finished_at = Some(Instant::now());
                        record.state = match &result {
                            Ok(outcome) if outcome.cancelled() => OperationState::Cancelled,
                            Ok(outcome) if outcome.has_errors() => {
                                OperationState::CompletedWithErrors
                            }
                            Ok(_) => OperationState::Completed,
                            Err(error) => {
                                record.errors.push(OperationError {
                                    path: PathBuf::new(),
                                    message: error.clone(),
                                });
                                OperationState::Failed
                            }
                        };
                    }
                    completions.push(OperationCompletion { id, result });
                }
            }
        }
        completions
    }

    pub fn pause(&mut self, id: OperationId) {
        if let Some(record) = self.record_mut(id) {
            if record.state == OperationState::Running && record.pausable() {
                record.control.pause();
                record.state = OperationState::Paused;
            }
        }
    }

    pub fn resume(&mut self, id: OperationId) {
        if let Some(record) = self.record_mut(id) {
            if record.state == OperationState::Paused {
                record.control.resume();
                record.state = OperationState::Running;
            }
        }
    }

    pub fn cancel(&mut self, id: OperationId) {
        if let Some(record) = self.record_mut(id) {
            if record.state.is_active() {
                record.control.cancel();
                if record.state == OperationState::Paused {
                    record.state = OperationState::Running;
                }
            }
        }
    }

    pub fn retry(&mut self, id: OperationId) -> Result<Option<OperationId>, String> {
        let request = self
            .records
            .iter()
            .find(|record| record.id == id && record.state.can_retry())
            .map(|record| record.request.clone());
        request.map(|request| self.start(request)).transpose()
    }

    pub fn active_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.state.is_active())
            .count()
    }

    pub fn records(&self) -> impl DoubleEndedIterator<Item = &OperationRecord> {
        self.records.iter()
    }

    pub fn clear_finished(&mut self) {
        self.records.retain(|record| record.state.is_active());
    }

    fn record_mut(&mut self, id: OperationId) -> Option<&mut OperationRecord> {
        self.records.iter_mut().find(|record| record.id == id)
    }

    fn trim_history(&mut self) {
        while self.records.len() >= MAX_OPERATION_HISTORY {
            let Some(index) = self
                .records
                .iter()
                .position(|record| !record.state.is_active())
            else {
                break;
            };
            self.records.remove(index);
        }
    }
}

fn run_operation(
    id: OperationId,
    request: OperationRequest,
    control: Arc<OperationControl>,
    tx: mpsc::Sender<OperationEvent>,
) {
    let result = match request {
        OperationRequest::Transfer(request) => {
            let outcome = file_ops::execute_transfer(&request, &control, |progress| {
                send_progress(id, progress, &tx)
            });
            Ok(OperationOutcome::Transfer(outcome))
        }
        OperationRequest::Recycle { paths } => {
            let _ = tx.send(OperationEvent::Prepared {
                id,
                total_bytes: 0,
                total_items: paths.len(),
            });
            if let Some(path) = paths.first() {
                let _ = tx.send(OperationEvent::ItemStarted {
                    id,
                    path: path.clone(),
                });
            }
            if control.is_cancelled() {
                Err("Recycle Bin operation was cancelled before it started".into())
            } else {
                windows_shell::recycle_paths(&paths).map(|()| {
                    for _ in &paths {
                        let _ = tx.send(OperationEvent::ItemCompleted { id });
                    }
                    OperationOutcome::Recycled(paths)
                })
            }
        }
        OperationRequest::DeletePermanent { paths } => {
            let outcome = file_ops::delete_permanently(&paths, &control, |progress| {
                send_progress(id, progress, &tx)
            });
            Ok(OperationOutcome::Deleted(outcome))
        }
    };
    let _ = tx.send(OperationEvent::Finished { id, result });
}

fn send_progress(id: OperationId, progress: FileOpProgress, tx: &mpsc::Sender<OperationEvent>) {
    let event = match progress {
        FileOpProgress::Prepared {
            total_bytes,
            total_items,
        } => OperationEvent::Prepared {
            id,
            total_bytes,
            total_items,
        },
        FileOpProgress::ItemStarted { path } => OperationEvent::ItemStarted { id, path },
        FileOpProgress::Bytes(delta) => OperationEvent::Bytes { id, delta },
        FileOpProgress::ItemCompleted => OperationEvent::ItemCompleted { id },
        FileOpProgress::ItemError(error) => OperationEvent::ItemError { id, error },
    };
    let _ = tx.send(event);
}

fn retain_retryable_work(record: &mut OperationRecord, outcome: &OperationOutcome) {
    match (&mut record.request, outcome) {
        (OperationRequest::Transfer(request), OperationOutcome::Transfer(outcome)) => {
            request.items.retain(|item| {
                !outcome.completed.iter().any(|completed| {
                    completed.source == item.source && completed.destination == item.destination
                })
            });
        }
        (OperationRequest::DeletePermanent { paths }, OperationOutcome::Deleted(outcome)) => {
            paths.retain(|path| !outcome.deleted.contains(path));
        }
        (OperationRequest::Recycle { paths }, OperationOutcome::Recycled(recycled)) => {
            paths.retain(|path| !recycled.contains(path));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OperationCompletion, OperationManager, OperationOutcome, OperationRequest, OperationState,
    };
    use crate::file_ops::{prepare_transfer, TransferMode};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn transfer_operation_reports_completion_and_keeps_a_retryable_record() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("luxepix-operation-{}-{unique}", std::process::id()));
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
        let mut operations = OperationManager::new();
        let id = operations
            .start(OperationRequest::Transfer(request))
            .expect("start operation");
        let completion = wait_for_completion(&mut operations);

        assert_eq!(completion.id, id);
        assert!(matches!(
            completion.result,
            Ok(OperationOutcome::Transfer(_))
        ));
        let record = operations
            .records()
            .find(|record| record.id == id)
            .expect("operation record");
        assert_eq!(record.state, OperationState::Completed);
        assert_eq!(record.completed_items, 1);
        assert!(target.join("photo.jpg").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn retry_keeps_only_items_that_did_not_complete() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "luxepix-operation-retry-{}-{unique}",
            std::process::id()
        ));
        let source_root = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(&source_root).expect("create source");
        fs::create_dir_all(&target).expect("create target");
        let available = source_root.join("available.jpg");
        let missing = source_root.join("missing.jpg");
        fs::write(&available, b"available").expect("write available source");
        let preparation = prepare_transfer(
            &[available.clone(), missing.clone()],
            &target,
            TransferMode::Copy,
        )
        .expect("prepare transfer");
        let request = preparation
            .resolve(&HashMap::new())
            .expect("resolve transfer");
        let mut operations = OperationManager::new();
        let first_id = operations
            .start(OperationRequest::Transfer(request))
            .expect("start operation");
        wait_for_completion(&mut operations);

        {
            let record = operations
                .records()
                .find(|record| record.id == first_id)
                .expect("failed operation record");
            assert_eq!(record.state, OperationState::CompletedWithErrors);
            let OperationRequest::Transfer(request) = &record.request else {
                panic!("expected transfer retry request");
            };
            assert_eq!(request.items.len(), 1);
            assert_eq!(request.items[0].source, missing);
        }

        fs::write(&missing, b"now available").expect("write retry source");
        let retry_id = operations
            .retry(first_id)
            .expect("start retry")
            .expect("retryable operation");
        let completion = wait_for_completion(&mut operations);

        assert_eq!(completion.id, retry_id);
        assert!(target.join("available.jpg").exists());
        assert!(target.join("missing.jpg").exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    fn wait_for_completion(operations: &mut OperationManager) -> OperationCompletion {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(completion) = operations.process_events(64).into_iter().next() {
                return completion;
            }
            assert!(Instant::now() < deadline, "operation did not complete");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
