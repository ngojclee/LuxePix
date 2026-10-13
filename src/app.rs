use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use eframe::egui;

use crate::commands::{CommandContext, CommandId};
use crate::config::{
    AppConfig, ProductCodeMode, ShortcutConfig, MAX_THUMBNAIL_MEMORY_BUDGET_MB,
    MIN_THUMBNAIL_MEMORY_BUDGET_MB,
};
use crate::file_ops::{self, CollisionChoice, TransferMode, TransferPreparation, TransferRequest};
use crate::index::{IndexEvent, IndexManager, IndexState};
use crate::model::{FileEntry, FolderEntry, ItemId, SortDirection, SortField, ViewMode};
use crate::navigation::NavigationHistory;
use crate::operations::{
    OperationId, OperationManager, OperationOutcome, OperationRecord, OperationRequest,
    OperationState,
};
use crate::power_tools::{
    BatchRenameRule, DuplicateGroup, FolderSizeEntry, PowerToolManager, PowerToolResult,
};
use crate::scanner::{format_size, ScanOptions};
use crate::search::{ProductSearchResult, SearchManager, SearchResponse, SearchScope};
use crate::selection::SelectionModel;
use crate::similarity::{
    ModelPack, SimilarityEngine, SimilarityManager, SimilarityQuery, SimilarityResponse,
    SimilarityResult,
};
use crate::sorting::{sort_files, sort_folders};
use crate::storage::{transactional_write, StoragePaths};
use crate::tasks::{TaskEvent, TaskId, TaskKind, TaskManager};
use crate::thumbs::ThumbManager;
use crate::undo::UndoManager;
use crate::updater::{StartupHandshake, UpdateManager, UpdateState};
use crate::windows_shell;

const FULL_IMAGE_MEMORY_BUDGET_BYTES: usize = 256 * 1024 * 1024;

mod details;
mod explorer;
mod first_run;
mod image_search;
mod quick_find;
mod sidebar;

// ── Preview state ──────────────────────────────────────────────
struct PreviewState {
    path: PathBuf,
    index: usize,
    zoom: f32,
    offset: egui::Vec2,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExplorerPane {
    #[default]
    Folders,
    Files,
}

#[derive(Clone, Default)]
struct RootViewState {
    selection: SelectionModel,
    active_folder: Option<ItemId>,
    scroll_offset: f32,
}

#[derive(Clone, Default)]
struct FolderViewState {
    selection: SelectionModel,
    scroll_offset: f32,
}

#[derive(Clone)]
struct MarqueeState {
    start: egui::Pos2,
    current: egui::Pos2,
    base: SelectionModel,
}

// ── App state ──────────────────────────────────────────────────
pub struct LuxePixApp {
    path_input: String,
    current_path: Option<PathBuf>,
    navigation: NavigationHistory,

    folders: Vec<FolderEntry>,
    selected_folder: Option<ItemId>,
    folder_selection: SelectionModel,
    left_thumb_size: f32,
    left_scroll_offset: f32,
    folder_columns: usize,
    folder_marquee: Option<MarqueeState>,

    right_files: Vec<FileEntry>,
    right_image_paths: Vec<PathBuf>,
    selected_file: Option<ItemId>,
    file_selection: SelectionModel,
    right_thumb_size: f32,
    right_scroll_offset: f32,
    file_columns: usize,
    file_marquee: Option<MarqueeState>,
    active_pane: ExplorerPane,
    pending_folder_scroll_to: Option<ItemId>,
    pending_file_scroll_to: Option<ItemId>,

    root_view_states: HashMap<PathBuf, RootViewState>,
    folder_view_states: HashMap<PathBuf, FolderViewState>,

    preview: Option<PreviewState>,

    thumb_mgr: ThumbManager,
    full_mgr: ThumbManager,
    tasks: TaskManager,
    root_scan_task: Option<TaskId>,
    folder_scan_task: Option<TaskId>,
    operations: OperationManager,
    undo: UndoManager,
    index: IndexManager,
    search: SearchManager,
    similarity: SimilarityManager,
    updates: UpdateManager,
    power_tools: PowerToolManager,

    // Rename state
    renaming: Option<RenameState>,

    // Config & history
    storage: StoragePaths,
    config: AppConfig,
    effective_theme: String,
    show_history: bool,
    show_settings: bool,
    settings_tab: SettingsTab,
    shortcut_draft: ShortcutConfig,
    shortcut_capture: Option<CommandId>,
    first_run_step: usize,
    first_run_source: String,
    first_run_source_valid: bool,
    path_editing: bool,
    clipboard: Option<FileClipboard>,
    pending_paste: Option<PendingPaste>,
    pending_delete: Option<PendingDelete>,
    internal_drag: Option<InternalDrag>,
    show_task_center: bool,
    show_quick_find: bool,
    quick_find_query: String,
    quick_find_scope: QuickFindScope,
    quick_find_results: SearchResponse,
    quick_find_selected: usize,
    quick_find_focus_requested: bool,
    quick_find_dirty_since: Option<std::time::Instant>,
    quick_find_running: bool,
    pending_product_reveal: Option<PathBuf>,
    quick_find_current_anchor: Option<PathBuf>,
    quick_find_source_anchor: Option<String>,
    pending_index_rebuild: bool,
    pending_index_reconcile: bool,
    show_image_search: bool,
    similarity_query_label: String,
    similarity_query_path: Option<PathBuf>,
    similarity_last_query: Option<SimilarityQuery>,
    similarity_results: SimilarityResponse,
    similarity_selected: usize,
    similarity_running: bool,
    model_pack_status: String,
    show_update_dialog: bool,
    startup_update_check_pending: bool,
    startup_handshake: Option<StartupHandshake>,
    first_frame_complete: bool,
    show_batch_rename: bool,
    batch_rename_paths: Vec<PathBuf>,
    batch_rename_rule: BatchRenameRule,
    show_duplicate_finder: bool,
    duplicate_root: Option<PathBuf>,
    duplicate_groups: Vec<DuplicateGroup>,
    duplicate_selected: HashSet<PathBuf>,
    show_folder_sizes: bool,
    folder_size_root: Option<PathBuf>,
    folder_size_entries: Vec<FolderSizeEntry>,

    brand_icon: Option<egui::TextureHandle>,

    status: String,
}

struct RenameState {
    original_path: PathBuf,
    new_name: String,
}

#[derive(Clone)]
struct FileClipboard {
    paths: Vec<PathBuf>,
    mode: TransferMode,
}

struct PendingPaste {
    preparation: TransferPreparation,
    choices: HashMap<PathBuf, CollisionChoice>,
    apply_to_all: bool,
}

struct InternalDrag {
    paths: Vec<PathBuf>,
    target_dir: Option<PathBuf>,
    mode: TransferMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeleteMode {
    Recycle,
    Permanent,
}

struct PendingDelete {
    paths: Vec<PathBuf>,
    mode: DeleteMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsTab {
    General,
    Appearance,
    Shortcuts,
    SearchIndex,
    FileOperations,
    Advanced,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum QuickFindScope {
    #[default]
    Current,
    CurrentSource,
    AllSources,
}

impl QuickFindScope {
    const ALL: [Self; 3] = [Self::Current, Self::CurrentSource, Self::AllSources];

    const fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::CurrentSource => "Current Source",
            Self::AllSources => "All Sources",
        }
    }
}

impl SettingsTab {
    const ALL: [Self; 6] = [
        Self::General,
        Self::Appearance,
        Self::Shortcuts,
        Self::SearchIndex,
        Self::FileOperations,
        Self::Advanced,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Shortcuts => "Shortcuts",
            Self::SearchIndex => "Search & Index",
            Self::FileOperations => "File Operations",
            Self::Advanced => "Advanced",
        }
    }
}

enum CapturedShortcut {
    Cancel,
    Value(String),
}

impl LuxePixApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let storage = StoragePaths::detect();
        let config = AppConfig::load(&storage);
        let database_path = storage.index_dir.join("luxepix.db");
        let index = IndexManager::new(database_path.clone(), config.indexing_enabled);
        let search = SearchManager::new(crate::search::SearchEngine::new(database_path.clone()));
        let mut similarity_engine = SimilarityEngine::new(database_path);
        let model_pack_result = ModelPack::discover(
            &storage.executable_dir,
            &storage.similarity_dir,
            (!config.model_pack_path.is_empty()).then(|| Path::new(&config.model_pack_path)),
        );
        let model_pack_status = match &model_pack_result {
            Ok(Some(pack)) => format!("Ready - {}", pack.manifest.pack_version),
            Ok(None) => "Not installed - using local pHash".into(),
            Err(error) => format!("Invalid - {error}"),
        };
        similarity_engine.set_model_pack(model_pack_result.ok().flatten());
        let similarity = SimilarityManager::new(similarity_engine, storage.similarity_dir.clone());
        let (startup_handshake, startup_handshake_error) =
            match crate::updater::startup_handshake_from_args() {
                Ok(handshake) => (handshake, None),
                Err(error) => (None, Some(error)),
            };
        let effective_theme = apply_theme(&cc.egui_ctx, &config.theme).to_string();
        let startup_update_check_pending = config.check_updates_on_startup;
        let thumbnail_memory_budget_bytes = config.thumbnail_memory_budget_bytes();
        let shortcut_draft = config.shortcuts.clone();
        let first_run_source = config
            .sources
            .first()
            .map(|source| source.path.clone())
            .or_else(|| config.last_path.clone())
            .unwrap_or_default();
        let first_run_source_valid = config
            .sources
            .iter()
            .any(|source| source.path.eq_ignore_ascii_case(&first_run_source));

        let mut app = Self {
            path_input: String::new(),
            current_path: None,
            navigation: NavigationHistory::default(),
            folders: Vec::new(),
            selected_folder: None,
            folder_selection: SelectionModel::default(),
            left_thumb_size: config.left_thumb_size,
            left_scroll_offset: 0.0,
            folder_columns: 1,
            folder_marquee: None,
            right_files: Vec::new(),
            right_image_paths: Vec::new(),
            selected_file: None,
            file_selection: SelectionModel::default(),
            right_thumb_size: config.right_thumb_size,
            right_scroll_offset: 0.0,
            file_columns: 1,
            file_marquee: None,
            active_pane: ExplorerPane::Folders,
            pending_folder_scroll_to: None,
            pending_file_scroll_to: None,
            root_view_states: HashMap::new(),
            folder_view_states: HashMap::new(),
            preview: None,
            thumb_mgr: ThumbManager::new(thumbnail_memory_budget_bytes, 4),
            full_mgr: ThumbManager::new(FULL_IMAGE_MEMORY_BUDGET_BYTES, 2),
            tasks: TaskManager::new(4),
            root_scan_task: None,
            folder_scan_task: None,
            operations: OperationManager::new(),
            undo: UndoManager::default(),
            index,
            search,
            similarity,
            updates: UpdateManager::new(),
            power_tools: PowerToolManager::new(),
            renaming: None,
            storage,
            config,
            effective_theme,
            show_history: false,
            show_settings: false,
            settings_tab: SettingsTab::General,
            shortcut_draft,
            shortcut_capture: None,
            first_run_step: 0,
            first_run_source,
            first_run_source_valid,
            path_editing: false,
            clipboard: None,
            pending_paste: None,
            pending_delete: None,
            internal_drag: None,
            show_task_center: false,
            show_quick_find: false,
            quick_find_query: String::new(),
            quick_find_scope: QuickFindScope::Current,
            quick_find_results: SearchResponse::default(),
            quick_find_selected: 0,
            quick_find_focus_requested: false,
            quick_find_dirty_since: None,
            quick_find_running: false,
            pending_product_reveal: None,
            quick_find_current_anchor: None,
            quick_find_source_anchor: None,
            pending_index_rebuild: false,
            pending_index_reconcile: false,
            show_image_search: false,
            similarity_query_label: String::new(),
            similarity_query_path: None,
            similarity_last_query: None,
            similarity_results: SimilarityResponse::default(),
            similarity_selected: 0,
            similarity_running: false,
            model_pack_status,
            show_update_dialog: false,
            startup_update_check_pending,
            startup_handshake,
            first_frame_complete: false,
            show_batch_rename: false,
            batch_rename_paths: Vec::new(),
            batch_rename_rule: BatchRenameRule::default(),
            show_duplicate_finder: false,
            duplicate_root: None,
            duplicate_groups: Vec::new(),
            duplicate_selected: HashSet::new(),
            show_folder_sizes: false,
            folder_size_root: None,
            folder_size_entries: Vec::new(),
            brand_icon: None,
            status: startup_handshake_error
                .unwrap_or_else(|| "Ready. Pick a folder above or choose a recent path.".into()),
        };

        if let Some(ref last) = app.config.last_path.clone() {
            if !last.trim().is_empty() {
                let p = PathBuf::from(last);
                app.navigation.restore(p.clone());
                app.load_path(p, false);
            }
        }

        app
    }

    fn navigate_to(&mut self, path: PathBuf) {
        self.navigation.visit(path.clone());
        self.load_path(path, true);
    }

    fn load_path(&mut self, path: PathBuf, save_recent: bool) {
        self.remember_current_view_state();
        let root_state = self
            .root_view_states
            .get(&path)
            .cloned()
            .unwrap_or_default();

        self.current_path = Some(path.clone());
        self.path_input = path.to_string_lossy().to_string();
        self.path_editing = false;
        self.renaming = None;
        self.show_history = false;

        if save_recent {
            self.config.add_path(&self.path_input);
        }

        self.root_scan_task = Some(
            self.tasks
                .scan_root(&path, ScanOptions::new(self.config.show_hidden_files)),
        );
        self.folder_scan_task = None;
        self.folders.clear();
        self.folder_selection = root_state.selection;
        self.selected_folder = root_state.active_folder;
        self.left_scroll_offset = root_state.scroll_offset;
        self.right_files.clear();
        self.right_image_paths.clear();
        let folder_state = self
            .selected_folder
            .as_ref()
            .and_then(|id| self.folder_view_states.get(id.as_path()))
            .cloned()
            .unwrap_or_default();
        self.file_selection = folder_state.selection;
        self.selected_file = self.file_selection.focus().cloned();
        self.right_scroll_offset = folder_state.scroll_offset;
        self.preview = None;
        self.pending_folder_scroll_to = self.selected_folder.clone();
        self.pending_file_scroll_to = self.file_selection.focus().cloned();
        self.status = format!("Scanning {}...", path.display());
    }

    fn activate_folder(&mut self, id: ItemId) {
        let Some(folder) = self.folders.iter().find(|folder| folder.id == id) else {
            return;
        };
        let path = folder.path.clone();
        let name = folder.name.clone();
        if self.selected_folder.as_ref() == Some(&id) && self.folder_scan_task.is_some() {
            return;
        }

        self.remember_current_folder_view_state();
        self.selected_folder = Some(id);
        let folder_state = self
            .folder_view_states
            .get(&path)
            .cloned()
            .unwrap_or_default();
        self.file_selection = folder_state.selection;
        self.selected_file = self.file_selection.focus().cloned();
        self.right_scroll_offset = folder_state.scroll_offset;
        self.pending_file_scroll_to = self.file_selection.focus().cloned();
        self.right_files.clear();
        self.right_image_paths.clear();
        self.preview = None;
        self.file_marquee = None;
        self.folder_scan_task = Some(
            self.tasks
                .scan_folder(&path, ScanOptions::new(self.config.show_hidden_files)),
        );
        self.status = format!("Loading {}...", name);
    }

    fn select_folder_with_modifiers(&mut self, id: ItemId, modifiers: egui::Modifiers) {
        let ordered = self.ordered_folder_ids();
        self.folder_selection
            .apply_click(id, &ordered, modifiers.ctrl, modifiers.shift);
        self.active_pane = ExplorerPane::Folders;
        if let Some(focus) = self.folder_selection.focus().cloned() {
            if self.selected_folder.as_ref() != Some(&focus) {
                self.activate_folder(focus);
            }
        } else {
            self.clear_active_folder();
        }
    }

    fn select_folder_for_context(&mut self, id: ItemId) {
        self.active_pane = ExplorerPane::Folders;
        if !self.folder_selection.contains(&id) {
            self.folder_selection.replace(id.clone());
        }
        if self.selected_folder.as_ref() != Some(&id) {
            self.activate_folder(id);
        }
    }

    fn select_file_with_modifiers(&mut self, id: ItemId, modifiers: egui::Modifiers) {
        let ordered = self.ordered_file_ids();
        self.file_selection
            .apply_click(id, &ordered, modifiers.ctrl, modifiers.shift);
        self.selected_file = self.file_selection.focus().cloned();
        self.active_pane = ExplorerPane::Files;
    }

    fn select_file_for_context(&mut self, id: ItemId) {
        self.active_pane = ExplorerPane::Files;
        if !self.file_selection.contains(&id) {
            self.file_selection.replace(id);
        }
        self.selected_file = self.file_selection.focus().cloned();
    }

    fn clear_active_folder(&mut self) {
        self.remember_current_folder_view_state();
        self.selected_folder = None;
        self.file_selection.clear();
        self.selected_file = None;
        self.right_files.clear();
        self.right_image_paths.clear();
        self.right_scroll_offset = 0.0;
        self.folder_scan_task = None;
        self.preview = None;
        self.pending_file_scroll_to = None;
    }

    fn clear_active_selection(&mut self) {
        match self.active_pane {
            ExplorerPane::Folders => {
                self.folder_selection.clear();
                self.pending_folder_scroll_to = None;
                self.clear_active_folder();
            }
            ExplorerPane::Files => {
                self.file_selection.clear();
                self.selected_file = None;
                self.pending_file_scroll_to = None;
            }
        }
    }

    fn ordered_folder_ids(&self) -> Vec<ItemId> {
        self.folders
            .iter()
            .map(|folder| folder.id.clone())
            .collect()
    }

    fn ordered_file_ids(&self) -> Vec<ItemId> {
        self.right_files
            .iter()
            .map(|file| file.id.clone())
            .collect()
    }

    fn remember_current_folder_view_state(&mut self) {
        let Some(path) = self.selected_folder.as_ref().map(ItemId::as_path) else {
            return;
        };
        self.folder_view_states.insert(
            path.to_path_buf(),
            FolderViewState {
                selection: self.file_selection.clone(),
                scroll_offset: self.right_scroll_offset,
            },
        );
    }

    fn remember_current_view_state(&mut self) {
        self.remember_current_folder_view_state();
        if let Some(path) = self.current_path.clone() {
            self.root_view_states.insert(
                path,
                RootViewState {
                    selection: self.folder_selection.clone(),
                    active_folder: self.selected_folder.clone(),
                    scroll_offset: self.left_scroll_offset,
                },
            );
        }
    }

    fn start_restored_folder_scan(&mut self) {
        let Some(selected) = self.selected_folder.clone() else {
            return;
        };
        let Some(folder) = self
            .folders
            .iter()
            .find(|folder| folder.id == selected)
            .cloned()
        else {
            return;
        };
        self.folder_scan_task = Some(self.tasks.scan_folder(
            &folder.path,
            ScanOptions::new(self.config.show_hidden_files),
        ));
        self.status = format!("Loading {}...", folder.name);
    }

    fn process_task_events(&mut self) {
        for event in self.tasks.drain(32) {
            match event {
                TaskEvent::RootScanned { id, path, folders }
                    if self.root_scan_task == Some(id)
                        && self.current_path.as_ref() == Some(&path) =>
                {
                    self.root_scan_task = None;
                    self.folders = folders;
                    sort_folders(
                        &mut self.folders,
                        self.config.sort_field,
                        self.config.sort_direction,
                    );
                    let ordered = self.ordered_folder_ids();
                    self.folder_selection.retain(&ordered);
                    if self
                        .selected_folder
                        .as_ref()
                        .is_some_and(|selected| !ordered.contains(selected))
                    {
                        self.selected_folder = self.folder_selection.focus().cloned();
                    }
                    if self.selected_folder.is_none() {
                        self.file_selection.clear();
                        self.selected_file = None;
                        self.right_files.clear();
                        self.right_image_paths.clear();
                    } else if self.folder_scan_task.is_none() {
                        self.start_restored_folder_scan();
                    }

                    let total_imgs: usize =
                        self.folders.iter().map(|folder| folder.image_count).sum();
                    let total_files: usize =
                        self.folders.iter().map(|folder| folder.total_files).sum();
                    if self.folder_scan_task.is_none() {
                        self.status = format!(
                            "Scanned {} folders, {} images and {} files.",
                            self.folders.len(),
                            total_imgs,
                            total_files
                        );
                    }
                    self.apply_pending_product_reveal(&path);
                }
                TaskEvent::FolderScanned { id, path, files }
                    if self.folder_scan_task == Some(id)
                        && self
                            .selected_folder
                            .as_ref()
                            .is_some_and(|selected| selected.as_path() == path) =>
                {
                    self.folder_scan_task = None;
                    self.right_files = files;
                    sort_files(
                        &mut self.right_files,
                        self.config.sort_field,
                        self.config.sort_direction,
                    );
                    self.right_image_paths = self
                        .right_files
                        .iter()
                        .filter(|file| file.is_image)
                        .map(|file| file.path.clone())
                        .collect();
                    let ordered = self.ordered_file_ids();
                    self.file_selection.retain(&ordered);
                    self.selected_file = self.file_selection.focus().cloned();
                    let image_count = self.right_image_paths.len();
                    let other_count = self.right_files.len().saturating_sub(image_count);
                    let folder_name = self
                        .folders
                        .iter()
                        .find(|folder| folder.path == path)
                        .map(|folder| folder.name.as_str())
                        .unwrap_or("Folder");
                    self.status = format!(
                        "{}: {} images and {} other files.",
                        folder_name, image_count, other_count
                    );
                }
                TaskEvent::Failed {
                    id,
                    kind,
                    path,
                    error,
                } => {
                    let is_current = match kind {
                        TaskKind::ScanRoot => self.root_scan_task == Some(id),
                        TaskKind::ScanFolder => self.folder_scan_task == Some(id),
                    };
                    if is_current {
                        match kind {
                            TaskKind::ScanRoot => self.root_scan_task = None,
                            TaskKind::ScanFolder => self.folder_scan_task = None,
                        }
                        self.status = format!("Could not scan {}: {}", path.display(), error);
                    }
                }
                _ => {}
            }
        }
    }

    fn process_index_events(&mut self) {
        for event in self.index.drain(64) {
            match event {
                IndexEvent::SourceAvailability {
                    source,
                    available: false,
                    error,
                    ..
                } => {
                    self.status = format!(
                        "{} is unavailable for indexing: {}",
                        source.name,
                        error.unwrap_or_else(|| "source could not be opened".into())
                    );
                }
                IndexEvent::Completed {
                    cancelled: false, ..
                } => {
                    let snapshot = self.index.snapshot();
                    self.status = format!(
                        "Search index ready: {} product folder(s), {} image(s).",
                        snapshot.products_indexed, snapshot.images_indexed
                    );
                    if !self.quick_find_query.trim().is_empty() {
                        self.queue_quick_find_search();
                    }
                    if self.similarity_last_query.is_some() {
                        self.rerun_similarity_query();
                    }
                }
                IndexEvent::Completed {
                    cancelled: true, ..
                } => {
                    self.status = "Search indexing stopped.".into();
                }
                IndexEvent::Failed { error, .. } => {
                    self.status = format!("Search index failed: {error}");
                }
                IndexEvent::SourceAvailability { .. }
                | IndexEvent::Progress { .. }
                | IndexEvent::SourceCompleted { .. } => {}
            }
        }
    }

    fn maintain_index(&mut self) {
        if !self.config.indexing_enabled {
            return;
        }
        if self.index.is_active() {
            return;
        }
        if self.pending_index_rebuild {
            match self
                .index
                .start_reconcile(self.config.sources.clone(), true)
            {
                Ok(()) => {
                    self.pending_index_rebuild = false;
                    self.pending_index_reconcile = false;
                }
                Err(error) => self.status = format!("Could not rebuild search index: {error}"),
            }
            return;
        }
        if self.pending_index_reconcile {
            match self
                .index
                .start_reconcile(self.config.sources.clone(), false)
            {
                Ok(()) => self.pending_index_reconcile = false,
                Err(error) => self.status = format!("Could not refresh search index: {error}"),
            }
            return;
        }
        if let Err(error) = self.index.reconcile_if_due(&self.config.sources) {
            if self.index.snapshot().state != IndexState::Failed {
                self.status = format!("Could not start search indexing: {error}");
            }
        }
    }

    fn request_index_rebuild(&mut self) {
        if self.config.indexing_enabled {
            self.pending_index_rebuild = true;
        }
    }

    fn request_index_reconcile(&mut self) {
        if self.config.indexing_enabled && !self.pending_index_rebuild {
            self.pending_index_reconcile = true;
        }
    }

    fn process_search_events(&mut self) {
        for completion in self.search.drain_latest(8) {
            if completion.query != self.quick_find_query.trim() {
                continue;
            }
            self.quick_find_running = false;
            match completion.result {
                Ok(results) => {
                    self.quick_find_results = results;
                    self.quick_find_selected = 0;
                    if let Some(first) = self.quick_find_results.first().cloned() {
                        self.reveal_search_result(&first);
                    }
                }
                Err(error) => {
                    self.quick_find_results = SearchResponse::default();
                    self.status = format!("Quick Find failed: {error}");
                }
            }
        }
    }

    fn refresh_model_pack(&mut self) {
        let custom_path = (!self.config.model_pack_path.is_empty())
            .then(|| Path::new(&self.config.model_pack_path));
        match ModelPack::discover(
            &self.storage.executable_dir,
            &self.storage.similarity_dir,
            custom_path,
        ) {
            Ok(Some(pack)) => {
                self.model_pack_status = format!(
                    "Ready - {} ({})",
                    pack.manifest.pack_version,
                    pack.root.display()
                );
                self.similarity.set_model_pack(Some(pack));
            }
            Ok(None) => {
                self.model_pack_status = "Not installed - using local pHash".into();
                self.similarity.set_model_pack(None);
            }
            Err(error) => {
                self.model_pack_status = format!("Invalid - {error}");
                self.similarity.set_model_pack(None);
            }
        }
    }

    fn start_similarity_query(&mut self, query: SimilarityQuery, preview_path: Option<PathBuf>) {
        if !self.config.indexing_enabled {
            self.status = "Enable Search & Index before finding products by image.".into();
            return;
        }
        let label = match &query {
            SimilarityQuery::Path(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            SimilarityQuery::Image { label, .. } => label.clone(),
        };
        self.similarity_query_label = label;
        self.similarity_query_path = preview_path;
        self.similarity_results = SimilarityResponse::default();
        self.similarity_selected = 0;
        self.similarity_running = true;
        self.similarity_last_query = Some(query.clone());
        self.similarity
            .submit(query, self.config.similarity_result_count);
        self.status = "Comparing the query image with the local product index...".into();
    }

    fn start_similarity_path(&mut self, path: PathBuf) {
        if !crate::scanner::is_image_file(&path) {
            self.status = format!("Choose a supported image file: {}", path.display());
            return;
        }
        self.start_similarity_query(SimilarityQuery::Path(path.clone()), Some(path));
    }

    fn pick_similarity_image(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff"],
            )
            .pick_file()
        else {
            return;
        };
        self.start_similarity_path(path);
    }

    fn paste_similarity_image(&mut self) {
        if let Ok(Some(payload)) = windows_shell::read_file_clipboard() {
            if let Some(path) = payload
                .paths
                .into_iter()
                .find(|path| crate::scanner::is_image_file(path))
            {
                self.start_similarity_path(path);
                return;
            }
        }
        match crate::similarity::clipboard_image() {
            Ok(image) => self.start_similarity_query(
                SimilarityQuery::Image {
                    image,
                    label: "Clipboard image".into(),
                },
                None,
            ),
            Err(error) => self.status = error,
        }
    }

    fn rerun_similarity_query(&mut self) {
        let Some(query) = self.similarity_last_query.clone() else {
            return;
        };
        let preview_path = self.similarity_query_path.clone();
        self.start_similarity_query(query, preview_path);
    }

    fn process_similarity_events(&mut self) {
        for completion in self.similarity.drain_latest(4) {
            self.similarity_running = false;
            match completion.result {
                Ok(results) => {
                    let count = results.results.len();
                    self.similarity_results = results;
                    self.similarity_selected = 0;
                    self.status = if count == 0 {
                        "No perceptual hashes are available yet. Rebuild the product index.".into()
                    } else {
                        format!(
                            "Found {count} similar product(s) for {}.",
                            completion.query_label
                        )
                    };
                }
                Err(error) => {
                    self.similarity_results = SimilarityResponse::default();
                    self.status = format!("Image Search failed: {error}");
                }
            }
        }
    }

    fn start_update_check(&mut self, manual: bool) {
        match self.updates.start_check(self.storage.mode, manual) {
            Ok(()) => {
                if manual {
                    self.status = "Checking GitHub Releases for a LuxePix update...".into();
                }
            }
            Err(error) => self.status = error,
        }
    }

    fn process_update_events(&mut self) {
        if !self.updates.process_events(64) {
            return;
        }
        match self.updates.state() {
            UpdateState::Current => {
                self.status = format!("LuxePix {} is up to date.", crate::version::display());
            }
            UpdateState::Available(offer) => {
                self.status = format!("LuxePix {} is available.", offer.version);
                self.show_update_dialog = true;
            }
            UpdateState::Ready(package) => {
                self.status = format!("LuxePix {} downloaded and verified.", package.offer.version);
                self.show_update_dialog = true;
            }
            UpdateState::Failed {
                message,
                manual: true,
            } => {
                self.status = message.clone();
            }
            UpdateState::Idle
            | UpdateState::Checking
            | UpdateState::Downloading { .. }
            | UpdateState::Failed { manual: false, .. } => {}
        }
    }

    fn start_update_download(&mut self, offer: crate::updater::UpdateOffer) {
        match self
            .updates
            .start_download(offer, &self.storage.updates_dir)
        {
            Ok(()) => {
                self.show_update_dialog = false;
                self.show_task_center = true;
                self.status = "Downloading the verified LuxePix update package...".into();
            }
            Err(error) => self.status = error,
        }
    }

    fn apply_verified_update(&mut self, ctx: &egui::Context) {
        let UpdateState::Ready(package) = self.updates.state().clone() else {
            self.status = "No verified update package is ready.".into();
            return;
        };
        match crate::updater::launch_verified_package(&package, &self.storage) {
            Ok(()) => {
                self.status = "Updater started. Closing LuxePix...".into();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(error) => {
                self.status = error;
                self.show_update_dialog = true;
            }
        }
    }

    fn open_batch_rename_tool(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = "Select one or more items before opening Batch Rename.".into();
            return;
        }
        self.batch_rename_paths = paths;
        self.batch_rename_rule = BatchRenameRule::default();
        self.show_batch_rename = true;
    }

    fn start_duplicate_finder_tool(&mut self) {
        let root = self
            .selected_folder_path()
            .or_else(|| self.current_path.clone());
        let Some(root) = root else {
            self.status = "Choose a source folder before finding duplicates.".into();
            return;
        };
        match self.power_tools.start_duplicate_scan(root.clone()) {
            Ok(()) => {
                self.duplicate_root = Some(root);
                self.duplicate_groups.clear();
                self.duplicate_selected.clear();
                self.show_duplicate_finder = false;
                self.show_task_center = true;
                self.status = "Scanning files and hashing same-size candidates...".into();
            }
            Err(error) => self.status = error,
        }
    }

    fn create_zip_from_selection(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.status = "Select one or more files or folders to create a ZIP archive.".into();
            return;
        }
        let parent = paths[0].parent().unwrap_or_else(|| Path::new("."));
        let default_name = if paths.len() == 1 {
            format!(
                "{}.zip",
                paths[0]
                    .file_stem()
                    .unwrap_or_else(|| paths[0].file_name().unwrap_or_default())
                    .to_string_lossy()
            )
        } else {
            "Archive.zip".into()
        };
        let Some(mut destination) = rfd::FileDialog::new()
            .add_filter("ZIP archive", &["zip"])
            .set_directory(parent)
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };
        if destination
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
        {
            destination.set_extension("zip");
        }
        destination = crate::power_tools::unique_destination(&destination);
        match self
            .power_tools
            .start_create_archive(paths, destination.clone())
        {
            Ok(()) => {
                self.show_task_center = true;
                self.status = format!("Creating {}...", destination.display());
            }
            Err(error) => self.status = error,
        }
    }

    fn extract_selected_zip(&mut self) {
        let selected = self.selected_paths();
        let archive = selected
            .iter()
            .find(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
            })
            .cloned()
            .or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("ZIP archive", &["zip"])
                    .pick_file()
            });
        let Some(archive) = archive else {
            return;
        };
        let parent = archive.parent().unwrap_or_else(|| Path::new("."));
        let folder_name = archive
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let destination = crate::power_tools::unique_destination(&parent.join(folder_name));
        match self
            .power_tools
            .start_extract_archive(archive.clone(), destination.clone())
        {
            Ok(()) => {
                self.show_task_center = true;
                self.status = format!("Extracting {}...", archive.display());
            }
            Err(error) => self.status = error,
        }
    }

    fn start_folder_size_tool(&mut self) {
        let Some(root) = self.current_path.clone() else {
            self.status = "Choose a source folder before analyzing folder sizes.".into();
            return;
        };
        match self.power_tools.start_folder_sizes(root.clone()) {
            Ok(()) => {
                self.folder_size_root = Some(root);
                self.folder_size_entries.clear();
                self.show_folder_sizes = false;
                self.show_task_center = true;
                self.status = "Calculating folder sizes...".into();
            }
            Err(error) => self.status = error,
        }
    }

    fn process_power_tool_events(&mut self) {
        for completion in self.power_tools.process_events(32) {
            match completion.result {
                Ok(PowerToolResult::BatchRenamed(pairs)) => {
                    let count = pairs.len();
                    self.undo.push_batch_rename(&pairs);
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = format!("Batch renamed {count} item(s).");
                }
                Ok(PowerToolResult::Duplicates { root, groups }) => {
                    let duplicate_files: usize = groups.iter().map(|group| group.paths.len()).sum();
                    self.duplicate_root = Some(root);
                    self.duplicate_groups = groups;
                    self.duplicate_selected.clear();
                    self.show_duplicate_finder = true;
                    self.status = format!(
                        "Duplicate Finder found {} group(s) across {duplicate_files} file(s).",
                        self.duplicate_groups.len()
                    );
                }
                Ok(PowerToolResult::ArchiveCreated(path)) => {
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = format!("Created ZIP archive {}.", path.display());
                }
                Ok(PowerToolResult::ArchiveExtracted(path)) => {
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = format!("Extracted ZIP archive to {}.", path.display());
                }
                Ok(PowerToolResult::FolderSizes { root, entries }) => {
                    self.folder_size_root = Some(root);
                    self.folder_size_entries = entries;
                    self.show_folder_sizes = true;
                    self.status = format!(
                        "Folder size analysis completed for {} folder(s).",
                        self.folder_size_entries.len()
                    );
                }
                Err(error) => {
                    self.status = format!("{} failed: {error}", completion.kind.label());
                }
            }
        }
    }

    fn queue_quick_find_search(&mut self) {
        self.quick_find_dirty_since = Some(std::time::Instant::now());
        self.quick_find_running = !self.quick_find_query.trim().is_empty();
        if self.quick_find_query.trim().is_empty() {
            self.quick_find_results = SearchResponse::default();
            self.quick_find_running = false;
        }
    }

    fn submit_quick_find_if_ready(&mut self) {
        let Some(dirty_since) = self.quick_find_dirty_since else {
            return;
        };
        if dirty_since.elapsed() < std::time::Duration::from_millis(120) {
            return;
        }
        self.quick_find_dirty_since = None;
        let query = self.quick_find_query.trim().to_string();
        if query.is_empty() {
            self.quick_find_results = SearchResponse::default();
            self.quick_find_running = false;
            return;
        }
        let scope = self.resolved_search_scope();
        self.search.submit(query, scope, 10);
        self.quick_find_running = true;
    }

    fn resolved_search_scope(&self) -> SearchScope {
        match self.quick_find_scope {
            QuickFindScope::Current => self
                .quick_find_current_anchor
                .clone()
                .or_else(|| self.current_path.clone())
                .map(SearchScope::Current)
                .unwrap_or(SearchScope::AllSources),
            QuickFindScope::CurrentSource => self
                .quick_find_source_anchor
                .clone()
                .or_else(|| self.current_source_path())
                .map(SearchScope::CurrentSource)
                .unwrap_or(SearchScope::AllSources),
            QuickFindScope::AllSources => SearchScope::AllSources,
        }
    }

    fn current_source_path(&self) -> Option<String> {
        let current = self.current_path.as_ref()?;
        let current_key = crate::search::normalize_path_key(current);
        self.config
            .sources
            .iter()
            .filter(|source| {
                let source_key = crate::search::normalize_path_key(Path::new(&source.path));
                current_key == source_key || current_key.starts_with(&format!("{source_key}/"))
            })
            .max_by_key(|source| crate::search::normalize_path_key(Path::new(&source.path)).len())
            .map(|source| source.path.clone())
    }

    fn reveal_search_result(&mut self, result: &ProductSearchResult) {
        self.reveal_product_folder(result.folder_path.clone());
    }

    fn reveal_similarity_result(&mut self, result: &SimilarityResult) {
        self.reveal_product_folder(result.folder_path.clone());
    }

    fn reveal_product_folder(&mut self, folder_path: PathBuf) {
        let Some(parent) = folder_path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.pending_product_reveal = Some(folder_path);
        if self.current_path.as_ref() == Some(&parent) {
            self.apply_pending_product_reveal(&parent);
        } else {
            self.navigate_to(parent);
        }
    }

    fn apply_pending_product_reveal(&mut self, parent: &Path) {
        let Some(target) = self.pending_product_reveal.clone() else {
            return;
        };
        if target.parent() != Some(parent) || self.current_path.as_deref() != Some(parent) {
            return;
        }
        let Some(id) = self
            .folders
            .iter()
            .find(|folder| folder.path == target)
            .map(|folder| folder.id.clone())
        else {
            return;
        };

        self.pending_product_reveal = None;
        self.folder_selection.replace(id.clone());
        self.pending_folder_scroll_to = Some(id.clone());
        self.active_pane = ExplorerPane::Folders;
        self.activate_folder(id);
    }

    fn navigate_back(&mut self) {
        if let Some(path) = self.navigation.back() {
            self.load_path(path, true);
        }
    }

    fn navigate_forward(&mut self) {
        if let Some(path) = self.navigation.forward() {
            self.load_path(path, true);
        }
    }

    fn navigate_up(&mut self) {
        if let Some(path) = self.navigation.up() {
            self.navigate_to(path);
        }
    }

    fn selected_folder_entry(&self) -> Option<&FolderEntry> {
        let selected = self.selected_folder.as_ref()?;
        self.folders.iter().find(|folder| &folder.id == selected)
    }

    fn selected_file_entry(&self) -> Option<&FileEntry> {
        let selected = self.selected_file.as_ref()?;
        self.right_files.iter().find(|file| &file.id == selected)
    }

    fn selected_folder_path(&self) -> Option<PathBuf> {
        self.selected_folder_entry()
            .map(|folder| folder.path.clone())
    }

    fn apply_current_sort(&mut self) {
        sort_folders(
            &mut self.folders,
            self.config.sort_field,
            self.config.sort_direction,
        );
        sort_files(
            &mut self.right_files,
            self.config.sort_field,
            self.config.sort_direction,
        );
        self.right_image_paths = self
            .right_files
            .iter()
            .filter(|file| file.is_image)
            .map(|file| file.path.clone())
            .collect();
        self.folder_selection.retain(&self.ordered_folder_ids());
        self.file_selection.retain(&self.ordered_file_ids());
        self.selected_file = self.file_selection.focus().cloned();
        match self.active_pane {
            ExplorerPane::Folders => {
                self.pending_folder_scroll_to = self.folder_selection.focus().cloned();
            }
            ExplorerPane::Files => {
                self.pending_file_scroll_to = self.file_selection.focus().cloned();
            }
        }
    }

    fn scan_status(&self) -> Option<String> {
        self.tasks.active_tasks().next().map(|(_, task)| {
            let label = match task.kind {
                TaskKind::ScanRoot => "Scanning folders",
                TaskKind::ScanFolder => "Loading files",
            };
            format!("{}: {}", label, task.path.display())
        })
    }

    /// Execute a pending rename.
    fn execute_rename(&mut self) {
        if let Some(rename) = self.renaming.take() {
            let new_name = rename.new_name.trim().to_string();
            let old_name = rename
                .original_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !new_name.is_empty() && new_name != old_name {
                let parent = rename
                    .original_path
                    .parent()
                    .unwrap_or(&rename.original_path);
                let new_path = parent.join(&new_name);
                match std::fs::rename(&rename.original_path, &new_path) {
                    Ok(_) => {
                        self.undo
                            .push_rename(rename.original_path.clone(), new_path.clone());
                        self.status = format!("Renamed \"{}\" to \"{}\".", old_name, new_name);
                        let old_id = ItemId::from(rename.original_path.as_path());
                        let new_id = ItemId::from(new_path.as_path());
                        if self.selected_folder.as_ref() == Some(&old_id) {
                            self.selected_folder = Some(new_id.clone());
                        }
                        if self.selected_file.as_ref() == Some(&old_id) {
                            self.selected_file = Some(new_id.clone());
                        }
                        self.folder_selection.replace_id(&old_id, new_id.clone());
                        self.file_selection.replace_id(&old_id, new_id);
                        self.request_index_reconcile();
                        self.refresh_current_view();
                    }
                    Err(e) => self.status = format!("Rename failed: {}", e),
                }
            }
        }
    }

    /// Open a file with the system default application.
    fn open_file_external(path: &std::path::Path) {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        }
    }

    fn open_url(url: &str) {
        #[cfg(target_os = "windows")]
        {
            if url.starts_with("https://") {
                let _ = std::process::Command::new("rundll32.exe")
                    .arg("url.dll,FileProtocolHandler")
                    .arg(url)
                    .spawn();
            }
        }
    }

    fn open_file_with(path: &Path) {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("rundll32.exe")
                .arg("shell32.dll,OpenAs_RunDLL")
                .arg(path)
                .spawn();
        }
    }

    fn reveal_in_explorer(path: &Path) {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        let selection = match self.active_pane {
            ExplorerPane::Folders => &self.folder_selection,
            ExplorerPane::Files => &self.file_selection,
        };
        if !selection.is_empty() {
            return selection
                .ids()
                .map(|id| id.as_path().to_path_buf())
                .collect();
        }
        Vec::new()
    }

    fn active_target_dir(&self) -> Option<PathBuf> {
        self.selected_folder_path()
            .or_else(|| self.current_path.clone())
    }

    fn command_context(&self) -> CommandContext {
        let selection_count = self.selected_paths().len();
        let has_single_selection = match self.active_pane {
            ExplorerPane::Folders => self.folder_selection.single().is_some(),
            ExplorerPane::Files => self.file_selection.single().is_some(),
        };
        let has_previewable_selection = self.active_pane == ExplorerPane::Files
            && self.selected_file_entry().is_some_and(|file| file.is_image);
        let has_current_path = self.current_path.is_some();

        CommandContext {
            has_selection: selection_count > 0,
            has_single_selection,
            has_clipboard: self.clipboard.is_some() || windows_shell::clipboard_has_files(),
            has_target_directory: self.active_target_dir().is_some(),
            has_current_path,
            has_items: !self.folders.is_empty() || !self.right_files.is_empty(),
            has_previewable_selection,
            can_navigate_back: self.navigation.can_back(),
            can_navigate_forward: self.navigation.can_forward(),
            can_navigate_up: self.navigation.can_up(),
            can_undo: self.undo.can_undo() && self.operations.active_count() == 0,
        }
    }

    fn command_enabled(&self, command: CommandId) -> bool {
        command.enabled(self.command_context())
    }

    fn open_settings(&mut self) {
        self.shortcut_draft = self.config.shortcuts.clone();
        self.shortcut_capture = None;
        self.show_settings = true;
    }

    fn refresh_current_view(&mut self) {
        if let Some(path) = self.current_path.clone() {
            self.remember_current_view_state();
            let options = ScanOptions::new(self.config.show_hidden_files);
            self.root_scan_task = Some(self.tasks.scan_root(&path, options));
            if let Some(folder_path) = self.selected_folder_path() {
                self.folder_scan_task = Some(self.tasks.scan_folder(&folder_path, options));
            }
            self.status = format!("Refreshing {}...", path.display());
        }
    }

    fn execute_command(&mut self, command: CommandId) {
        if !self.command_enabled(command) {
            self.status = format!("{} is not available right now.", command.short_label());
            return;
        }

        match command {
            CommandId::Copy => {
                let paths = self.selected_paths();
                let count = paths.len();
                self.clipboard = Some(FileClipboard {
                    paths: paths.clone(),
                    mode: TransferMode::Copy,
                });
                self.status = match windows_shell::write_file_clipboard(&paths, TransferMode::Copy) {
                    Ok(()) => format!("Copied {count} item(s) to the Windows clipboard."),
                    Err(error) => format!(
                        "Copied {count} item(s) to the LuxePix clipboard; Windows clipboard was unavailable: {error}"
                    ),
                };
            }
            CommandId::Cut => {
                let paths = self.selected_paths();
                let count = paths.len();
                self.clipboard = Some(FileClipboard {
                    paths: paths.clone(),
                    mode: TransferMode::Move,
                });
                self.status = match windows_shell::write_file_clipboard(&paths, TransferMode::Move)
                {
                    Ok(()) => format!("Cut {count} item(s). Paste to move them."),
                    Err(error) => format!(
                        "Cut {count} item(s) in LuxePix; Windows clipboard was unavailable: {error}"
                    ),
                };
            }
            CommandId::Paste => self.paste_clipboard(),
            CommandId::NewFolder => self.create_new_folder(),
            CommandId::Rename => {
                let target = match self.active_pane {
                    ExplorerPane::Folders => self
                        .selected_folder_entry()
                        .map(|folder| (folder.path.clone(), folder.name.clone())),
                    ExplorerPane::Files => self
                        .selected_file_entry()
                        .map(|file| (file.path.clone(), file.name.clone())),
                };
                if let Some((original_path, new_name)) = target {
                    self.renaming = Some(RenameState {
                        original_path,
                        new_name,
                    });
                }
            }
            CommandId::Delete => {
                let paths = self.selected_paths();
                if self.config.confirm_recycle_delete {
                    self.pending_delete = Some(PendingDelete {
                        paths,
                        mode: DeleteMode::Recycle,
                    });
                } else {
                    self.start_delete(paths, DeleteMode::Recycle);
                }
            }
            CommandId::DeletePermanent => {
                self.pending_delete = Some(PendingDelete {
                    paths: self.selected_paths(),
                    mode: DeleteMode::Permanent,
                });
            }
            CommandId::Open | CommandId::Preview => self.open_selected_item(),
            CommandId::OpenWith => {
                if let Some(path) = self.selected_paths().into_iter().next() {
                    Self::open_file_with(&path);
                }
            }
            CommandId::RevealInExplorer => {
                if let Some(path) = self.selected_paths().into_iter().next() {
                    Self::reveal_in_explorer(&path);
                }
            }
            CommandId::Refresh => {
                self.thumb_mgr.clear();
                self.refresh_current_view();
            }
            CommandId::Settings => self.open_settings(),
            CommandId::EditAddress => {
                if let Some(path) = &self.current_path {
                    self.path_input = path.to_string_lossy().to_string();
                }
                self.path_editing = true;
                self.show_history = false;
            }
            CommandId::Back => self.navigate_back(),
            CommandId::Forward => self.navigate_forward(),
            CommandId::Up => self.navigate_up(),
            CommandId::SelectAll => {
                match self.active_pane {
                    ExplorerPane::Folders => {
                        let ordered = self.ordered_folder_ids();
                        self.folder_selection.select_all(&ordered);
                        if let Some(focus) = self.folder_selection.focus().cloned() {
                            if self.selected_folder.as_ref() != Some(&focus) {
                                self.activate_folder(focus);
                            }
                        }
                    }
                    ExplorerPane::Files => {
                        let ordered = self.ordered_file_ids();
                        self.file_selection.select_all(&ordered);
                        self.selected_file = self.file_selection.focus().cloned();
                    }
                }
                self.status = format!("Selected {} item(s).", self.selected_paths().len());
            }
            CommandId::Undo => self.undo_latest_operation(),
            CommandId::Find => {
                if !self.show_quick_find {
                    self.quick_find_current_anchor = self.current_path.clone();
                    self.quick_find_source_anchor = self.current_source_path();
                }
                self.show_quick_find = true;
                self.quick_find_focus_requested = true;
                self.queue_quick_find_search();
            }
            CommandId::FindSimilar => {
                self.show_image_search = true;
            }
        }
    }

    fn open_selected_item(&mut self) {
        match self.active_pane {
            ExplorerPane::Files => {
                if let Some(file) = self.selected_file_entry().cloned() {
                    if file.is_image {
                        if let Some(img_idx) = self
                            .right_image_paths
                            .iter()
                            .position(|path| path == &file.path)
                        {
                            let path = self.right_image_paths[img_idx].clone();
                            self.full_mgr.request_full(&path);
                            self.preview = Some(PreviewState {
                                path,
                                index: img_idx,
                                zoom: 1.0,
                                offset: egui::Vec2::ZERO,
                            });
                        }
                    } else {
                        Self::open_file_external(&file.path);
                    }
                }
            }
            ExplorerPane::Folders => {
                if let Some(path) = self.selected_folder_path() {
                    self.navigate_to(path);
                }
            }
        }
    }

    fn paste_clipboard(&mut self) {
        let Some(target_dir) = self.active_target_dir() else {
            self.status = "No target folder selected.".into();
            return;
        };
        if !target_dir.is_dir() {
            self.status = format!("Paste target is not a folder: {}", target_dir.display());
            return;
        }

        let shell_payload = windows_shell::read_file_clipboard();
        let payload = match shell_payload {
            Ok(Some(payload)) => Some(FileClipboard {
                paths: payload.paths,
                mode: payload.mode,
            }),
            Ok(None) => self.clipboard.clone(),
            Err(error) => {
                if self.clipboard.is_some() {
                    self.status = format!(
                        "Windows clipboard was unavailable; using the LuxePix fallback: {error}"
                    );
                    self.clipboard.clone()
                } else {
                    self.status = format!("Could not read the Windows clipboard: {error}");
                    None
                }
            }
        };
        let Some(payload) = payload else {
            self.status = "The file clipboard is empty.".into();
            return;
        };
        let paths: Vec<PathBuf> = payload
            .paths
            .into_iter()
            .filter(|path| path.exists())
            .collect();
        if paths.is_empty() {
            self.status = "Clipboard files are no longer available.".into();
            return;
        }

        self.prepare_transfer(paths, target_dir, payload.mode);
    }

    fn prepare_transfer(&mut self, paths: Vec<PathBuf>, target_dir: PathBuf, mode: TransferMode) {
        match file_ops::prepare_transfer(&paths, &target_dir, mode) {
            Ok(preparation) if preparation.collision_count() > 0 => {
                self.pending_paste = Some(PendingPaste {
                    preparation,
                    choices: HashMap::new(),
                    apply_to_all: false,
                });
            }
            Ok(preparation) => match preparation.resolve(&HashMap::new()) {
                Ok(request) => self.start_transfer(request),
                Err(error) => self.status = format!("Could not prepare paste: {error}"),
            },
            Err(error) => self.status = format!("Could not prepare paste: {error}"),
        }
    }

    fn begin_folder_drag(&mut self, folder: &FolderEntry) {
        if !self.folder_selection.contains(&folder.id) {
            self.select_folder_with_modifiers(folder.id.clone(), egui::Modifiers::default());
        }
        let paths = self
            .folder_selection
            .ids()
            .map(|id| id.as_path().to_path_buf())
            .collect();
        self.internal_drag = Some(InternalDrag {
            paths,
            target_dir: None,
            mode: TransferMode::Move,
        });
    }

    fn begin_file_drag(&mut self, file: &FileEntry) {
        if !self.file_selection.contains(&file.id) {
            self.select_file_with_modifiers(file.id.clone(), egui::Modifiers::default());
        }
        let paths = self
            .file_selection
            .ids()
            .map(|id| id.as_path().to_path_buf())
            .collect();
        self.internal_drag = Some(InternalDrag {
            paths,
            target_dir: None,
            mode: TransferMode::Move,
        });
    }

    fn set_internal_drop_target(&mut self, target_dir: PathBuf, modifiers: egui::Modifiers) {
        if let Some(drag) = &mut self.internal_drag {
            drag.mode = default_drop_mode(&drag.paths, &target_dir, modifiers);
            drag.target_dir = Some(target_dir);
        }
    }

    fn internal_drop_target_is(&self, path: &Path) -> bool {
        self.internal_drag
            .as_ref()
            .and_then(|drag| drag.target_dir.as_deref())
            == Some(path)
    }

    fn finish_internal_drag(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.pointer.primary_released()) {
            return;
        }
        let Some(drag) = self.internal_drag.take() else {
            return;
        };
        if let Some(target_dir) = drag.target_dir {
            self.prepare_transfer(drag.paths, target_dir, drag.mode);
        } else {
            self.status = "Drag cancelled because no destination folder was selected.".into();
        }
    }

    fn handle_external_file_drop(&mut self, ctx: &egui::Context) -> usize {
        let (hovered_count, dropped_paths, modifiers) = ctx.input(|input| {
            (
                input.raw.hovered_files.len(),
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
                input.modifiers,
            )
        });
        if dropped_paths.is_empty() {
            return hovered_count;
        }

        if self.show_image_search {
            if let Some(path) = dropped_paths
                .into_iter()
                .find(|path| crate::scanner::is_image_file(path))
            {
                self.start_similarity_path(path);
            } else {
                self.status = "Drop a supported image file into Image Search.".into();
            }
            return 0;
        }

        let Some(target_dir) = self.active_target_dir() else {
            self.status = "Choose a destination folder before dropping files.".into();
            return 0;
        };
        let mode = default_drop_mode(&dropped_paths, &target_dir, modifiers);
        self.prepare_transfer(dropped_paths, target_dir, mode);
        0
    }

    fn render_drop_indicator(&self, ctx: &egui::Context, external_hover_count: usize) {
        let Some(pointer) = ctx.pointer_hover_pos() else {
            return;
        };
        if self.show_image_search && external_hover_count > 0 && self.internal_drag.is_none() {
            egui::Area::new(egui::Id::new("image_search_drop_indicator"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pointer + egui::vec2(16.0, 18.0))
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(egui::RichText::new("Search with this image").strong());
                        ui.label(egui::RichText::new("Processed locally").size(11.5));
                    });
                });
            return;
        }
        let (mode, count, target) = if let Some(drag) = &self.internal_drag {
            (
                drag.mode,
                drag.paths.len(),
                drag.target_dir
                    .as_ref()
                    .map(|path| path.display().to_string()),
            )
        } else if external_hover_count > 0 {
            let Some(target) = self.active_target_dir() else {
                return;
            };
            (
                TransferMode::Copy,
                external_hover_count,
                Some(target.display().to_string()),
            )
        } else {
            return;
        };

        let target_label = target
            .map(|path| elide_middle(&path, 52))
            .unwrap_or_else(|| "Choose a destination".into());
        egui::Area::new(egui::Id::new("file_drop_indicator"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pointer + egui::vec2(16.0, 18.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} {count} item(s)", mode.label())).strong(),
                    );
                    ui.label(egui::RichText::new(target_label).size(11.5));
                });
            });
    }

    fn start_transfer(&mut self, request: TransferRequest) {
        if request.items.is_empty() {
            self.status = "Nothing was selected for this paste.".into();
            return;
        }
        let title = request.mode.label();
        let count = request.items.len();
        match self.operations.start(OperationRequest::Transfer(request)) {
            Ok(_) => {
                self.status = format!("{title} started for {count} item(s).");
            }
            Err(error) => {
                self.status = error;
            }
        }
    }

    fn create_new_folder(&mut self) {
        let Some(parent) = self.current_path.clone() else {
            self.status = "Choose a root folder before creating a folder.".into();
            return;
        };
        match file_ops::create_unique_folder(&parent) {
            Ok(path) => {
                let id = ItemId::from(path.as_path());
                self.undo.push_new_folder(path.clone());
                self.folder_selection.replace(id.clone());
                self.selected_folder = Some(id.clone());
                self.pending_folder_scroll_to = Some(id);
                self.active_pane = ExplorerPane::Folders;
                self.renaming = Some(RenameState {
                    original_path: path.clone(),
                    new_name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                });
                self.status = "Created a new folder. Type a name and press Enter.".into();
                self.refresh_current_view();
            }
            Err(error) => self.status = format!("Could not create a folder: {error}"),
        }
    }

    fn undo_latest_operation(&mut self) {
        let label = self.undo.next_label().unwrap_or("Undo").to_string();
        match self.undo.undo_latest() {
            Ok(_) => {
                self.status = format!("{label} completed.");
                self.request_index_reconcile();
                self.refresh_current_view();
            }
            Err(error) => {
                self.status = format!("Undo could not be completed: {error}");
            }
        }
    }

    fn start_delete(&mut self, paths: Vec<PathBuf>, mode: DeleteMode) {
        if paths.is_empty() {
            self.status = "No items are selected for deletion.".into();
            return;
        }
        let request = match mode {
            DeleteMode::Recycle => OperationRequest::Recycle { paths },
            DeleteMode::Permanent => OperationRequest::DeletePermanent { paths },
        };
        match self.operations.start(request) {
            Ok(_) => {
                self.status = match mode {
                    DeleteMode::Recycle => "Moving selection to Recycle Bin...".into(),
                    DeleteMode::Permanent => "Permanent deletion started.".into(),
                };
            }
            Err(error) => self.status = error,
        }
    }

    fn confirm_delete(&mut self) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        self.start_delete(pending.paths, pending.mode);
    }

    fn process_operation_events(&mut self) {
        for completion in self.operations.process_events(256) {
            match completion.result {
                Ok(OperationOutcome::Transfer(outcome)) => {
                    let completed = outcome.completed.len();
                    let error_count = outcome.errors.len();
                    let undoable = self.undo.push_transfer(&outcome);
                    let moved = outcome
                        .completed
                        .first()
                        .is_some_and(|item| item.mode == TransferMode::Move);
                    if moved
                        && self.clipboard.as_ref().is_some_and(|clipboard| {
                            clipboard.mode == TransferMode::Move
                                && clipboard.paths.iter().all(|path| !path.exists())
                        })
                    {
                        self.clipboard = None;
                    }
                    let status = if outcome.cancelled {
                        format!(
                            "Operation cancelled after {completed} item(s).{}",
                            if undoable {
                                " Completed items can be undone."
                            } else {
                                ""
                            }
                        )
                    } else if error_count > 0 {
                        format!(
                            "Operation completed {completed} item(s) with {error_count} error(s)."
                        )
                    } else if undoable {
                        format!("Operation completed for {completed} item(s). Ctrl+Z is available.")
                    } else {
                        format!(
                            "Operation completed for {completed} item(s). Replaced items are not automatically undoable."
                        )
                    };
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = status;
                }
                Ok(OperationOutcome::Recycled(paths)) => {
                    let status = format!(
                        "Moved {} item(s) to Recycle Bin. Restore them from Windows Recycle Bin if needed.",
                        paths.len()
                    );
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = status;
                }
                Ok(OperationOutcome::Deleted(outcome)) => {
                    let deleted = outcome.deleted.len();
                    let status = if outcome.cancelled {
                        format!("Permanent deletion cancelled after {deleted} item(s).")
                    } else if outcome.errors.is_empty() {
                        format!("Permanently deleted {deleted} item(s).")
                    } else {
                        format!(
                            "Permanently deleted {deleted} item(s) with {} error(s).",
                            outcome.errors.len()
                        )
                    };
                    self.request_index_reconcile();
                    self.refresh_current_view();
                    self.status = status;
                }
                Err(error) => {
                    self.refresh_current_view();
                    self.status = format!("Operation {:?} failed: {error}", completion.id);
                }
            }
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input()
            || self.preview.is_some()
            || self.renaming.is_some()
            || self.show_settings
            || self.show_image_search
            || self.show_batch_rename
            || self.show_duplicate_finder
            || self.show_folder_sizes
            || self.pending_paste.is_some()
            || self.pending_delete.is_some()
            || !self.config.first_run_complete
        {
            return;
        }
        for command in CommandId::ACTIVE {
            if self.command_enabled(command)
                && shortcut_pressed(ctx, self.config.shortcuts.get(command))
            {
                self.execute_command(command);
                break;
            }
        }
    }

    fn handle_selection_navigation(&mut self, ctx: &egui::Context) {
        if self.preview.is_some()
            || self.renaming.is_some()
            || self.show_settings
            || self.show_image_search
            || self.show_batch_rename
            || self.show_duplicate_finder
            || self.show_folder_sizes
            || !self.config.first_run_complete
            || ctx.wants_keyboard_input()
        {
            return;
        }

        let mut escape = false;
        let mut delta = None;
        let mut extend = false;
        ctx.input(|input| {
            escape = input.key_pressed(egui::Key::Escape);
            if input.modifiers.ctrl || input.modifiers.alt {
                return;
            }
            extend = input.modifiers.shift;
            let columns = match self.active_pane {
                ExplorerPane::Folders => self.folder_columns.max(1),
                ExplorerPane::Files
                    if matches!(
                        self.config.file_view_mode,
                        ViewMode::Gallery | ViewMode::Grid
                    ) =>
                {
                    self.file_columns.max(1)
                }
                ExplorerPane::Files => 1,
            } as isize;
            if input.key_pressed(egui::Key::ArrowLeft) {
                delta = Some(-1);
            } else if input.key_pressed(egui::Key::ArrowRight) {
                delta = Some(1);
            } else if input.key_pressed(egui::Key::ArrowUp) {
                delta = Some(-columns);
            } else if input.key_pressed(egui::Key::ArrowDown) {
                delta = Some(columns);
            }
        });

        if escape {
            self.clear_active_selection();
            self.status = "Selection cleared.".into();
            return;
        }
        let Some(delta) = delta else {
            return;
        };

        match self.active_pane {
            ExplorerPane::Folders => {
                let ordered = self.ordered_folder_ids();
                if let Some(focus) = self.folder_selection.move_focus(&ordered, delta, extend) {
                    self.pending_folder_scroll_to = Some(focus.clone());
                    if self.selected_folder.as_ref() != Some(&focus) {
                        self.activate_folder(focus);
                    }
                }
            }
            ExplorerPane::Files => {
                let ordered = self.ordered_file_ids();
                if let Some(focus) = self.file_selection.move_focus(&ordered, delta, extend) {
                    self.selected_file = Some(focus.clone());
                    self.pending_file_scroll_to = Some(focus);
                }
            }
        }
    }
}

// ── eframe::App impl ──────────────────────────────────────────
impl eframe::App for LuxePixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let resolved_theme = resolve_theme(ctx, &self.config.theme);
        if resolved_theme != self.effective_theme {
            self.effective_theme = apply_theme(ctx, &self.config.theme).to_string();
        }
        if let Some(drag) = &mut self.internal_drag {
            drag.target_dir = None;
        }
        let palette = Palette::for_theme(&self.effective_theme);

        self.process_task_events();
        self.process_operation_events();
        self.process_index_events();
        self.maintain_index();
        self.submit_quick_find_if_ready();
        self.process_search_events();
        self.process_similarity_events();
        self.process_update_events();
        self.process_power_tool_events();
        if self.first_frame_complete && self.startup_update_check_pending {
            self.startup_update_check_pending = false;
            self.start_update_check(false);
        }

        // Process thumbnail results
        let loaded_thumbs = self.thumb_mgr.process_results(ctx);
        let loaded_full = self.full_mgr.process_results(ctx);
        if loaded_thumbs || loaded_full {
            ctx.request_repaint();
        }

        self.handle_global_shortcuts(ctx);
        self.handle_selection_navigation(ctx);

        // ── Top command bar ──
        egui::TopBottomPanel::top("address_bar")
            .exact_height(56.0)
            .frame(
                egui::Frame::none()
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, palette.border_soft))
                    .inner_margin(egui::Margin::symmetric(16.0, 0.0)),
            )
            .show(ctx, |ui| {
                self.render_address_bar(ui);
            });

        if self.show_history {
            self.render_history_popover(ctx);
        }

        // ── Bottom status bar ──
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(28.0)
            .frame(
                egui::Frame::none()
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, palette.border_soft))
                    .inner_margin(egui::Margin::symmetric(16.0, 0.0)),
            )
            .show(ctx, |ui| self.render_status_bar(ui));

        let available_width = ctx.available_rect().width();
        let compact_sidebar = available_width < 1_040.0;
        let sidebar_width = if self.config.sidebar_collapsed || compact_sidebar {
            52.0
        } else {
            190.0
        };

        egui::SidePanel::left("navigation_sidebar")
            .exact_width(sidebar_width)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, palette.border_soft))
                    .inner_margin(egui::Margin::symmetric(12.0, 14.0)),
            )
            .show(ctx, |ui| {
                self.render_navigation_sidebar(ui, compact_sidebar);
            });

        // ── Left pane (folder grid) ──
        let panel_width = self.config.left_panel_width;
        let left_min_width = if available_width < 1_040.0 {
            240.0
        } else {
            280.0
        };
        let details_reserve = if self.config.details_panel_open {
            220.0
        } else {
            0.0
        };
        let left_max_width = (available_width - sidebar_width - details_reserve - 260.0)
            .clamp(left_min_width, 760.0);
        let left_resp = egui::SidePanel::left("left_pane")
            .default_width(panel_width.min(left_max_width))
            .min_width(left_min_width)
            .max_width(left_max_width)
            .resizable(true)
            .frame(egui::Frame::none().fill(palette.surface))
            .show(ctx, |ui| {
                self.render_left_pane(ui);
            });
        // Track panel width for config save
        let new_panel_w = left_resp.response.rect.width();

        if self.config.details_panel_open {
            egui::SidePanel::right("details_panel")
                .default_width(270.0)
                .min_width(220.0)
                .max_width(340.0)
                .resizable(true)
                .frame(
                    egui::Frame::none()
                        .fill(palette.surface)
                        .stroke(egui::Stroke::new(1.0, palette.border_soft))
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0)),
                )
                .show(ctx, |ui| {
                    self.render_details_panel(ui);
                });
        }

        // ── Right pane (all files grid) ──
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(palette.canvas))
            .show(ctx, |ui| {
                if self.selected_folder.is_some() {
                    self.render_right_pane(ui, ctx);
                } else if self.current_path.is_none() {
                    self.render_initial_state(ui);
                } else {
                    self.render_select_folder_state(ui);
                }
            });

        let external_hover_count = self.handle_external_file_drop(ctx);
        self.render_drop_indicator(ctx, external_hover_count);
        self.finish_internal_drag(ctx);

        // ── Preview overlay ──
        if self.preview.is_some() {
            self.render_preview(ctx);
        }

        if self.show_settings {
            self.render_settings_window(ctx);
        }

        if self.show_quick_find {
            self.render_quick_find_window(ctx);
        }

        if self.show_image_search {
            self.render_image_search_window(ctx);
        }

        if self.pending_delete.is_some() {
            self.render_delete_confirmation(ctx);
        }

        if self.pending_paste.is_some() {
            self.render_collision_dialog(ctx);
        }

        if self.show_task_center {
            self.render_task_center(ctx);
        }

        if self.show_update_dialog {
            self.render_update_dialog(ctx);
        }

        if self.show_batch_rename {
            self.render_batch_rename_dialog(ctx);
        }

        if self.show_duplicate_finder {
            self.render_duplicate_finder_dialog(ctx);
        }

        if self.show_folder_sizes {
            self.render_folder_sizes_dialog(ctx);
        }

        self.render_first_run(ctx);

        // (Rename is now inline in panes — no separate dialog needed)

        // ── Save UI state periodically ──
        self.config
            .save_ui_state(self.left_thumb_size, self.right_thumb_size, new_panel_w);

        // Request repaint if there are pending thumbnails
        if !self.thumb_mgr.is_idle() || !self.full_mgr.is_idle() {
            ctx.request_repaint();
        }
        if self.tasks.active_count() > 0
            || self.operations.active_count() > 0
            || self.index.is_active()
            || self.quick_find_running
            || self.quick_find_dirty_since.is_some()
            || self.similarity_running
            || self.updates.is_active()
            || self.power_tools.active_count() > 0
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        if !self.first_frame_complete {
            self.first_frame_complete = true;
            if let Some(handshake) = self.startup_handshake.take() {
                if let Err(error) = handshake.complete() {
                    self.status = format!("Could not complete update startup handshake: {error}");
                }
            }
            ctx.request_repaint();
        }
    }
}

// ── UI Rendering ──────────────────────────────────────────────
impl LuxePixApp {
    fn render_address_bar(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

            let mark_rect = ui
                .allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover())
                .0;
            paint_brand_icon(ui, &mut self.brand_icon, mark_rect);
            ui.label(
                egui::RichText::new("LuxePix")
                    .size(14.0)
                    .strong()
                    .color(palette.text_primary),
            );

            ui.add_space(6.0);
            let sep = ui
                .allocate_exact_size(egui::vec2(1.0, 22.0), egui::Sense::hover())
                .0;
            ui.painter().rect_filled(sep, 0.0, palette.border_soft);
            ui.add_space(4.0);

            for (command, icon, tooltip) in [
                (CommandId::Back, IconKind::ChevronLeft, "Back"),
                (CommandId::Forward, IconKind::ChevronRight, "Forward"),
                (CommandId::Up, IconKind::ChevronUp, "Up"),
            ] {
                if ui
                    .add_enabled_ui(self.command_enabled(command), |ui| {
                        icon_button(ui, icon, None, tooltip, false, &palette)
                    })
                    .inner
                    .clicked()
                {
                    self.execute_command(command);
                }
            }

            let path_width = (ui.available_width() - 250.0).clamp(260.0, 620.0);
            let path_frame = egui::Frame::none()
                .fill(palette.surface_inset)
                .stroke(egui::Stroke::new(1.0, palette.border_soft))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 0.0));

            let segments = self
                .current_path
                .as_deref()
                .map(breadcrumb_segments)
                .unwrap_or_default();
            let mut breadcrumb_target = None;
            let mut edit_address = false;
            let path_edit_response = path_frame
                .show(ui, |ui| {
                    ui.set_width(path_width);
                    ui.set_height(34.0);
                    ui.horizontal_centered(|ui| {
                        let icon_rect = ui
                            .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover())
                            .0;
                        draw_icon(
                            ui.painter(),
                            IconKind::FolderOpen,
                            icon_rect,
                            palette.text_tertiary,
                            1.6,
                        );
                        ui.add_space(2.0);
                        if self.path_editing {
                            Some(
                                ui.add_sized(
                                    [ui.available_width(), 26.0],
                                    egui::TextEdit::singleline(&mut self.path_input)
                                        .hint_text("Paste or type a folder path...")
                                        .font(egui::TextStyle::Body)
                                        .frame(false)
                                        .text_color(palette.text_primary),
                                ),
                            )
                        } else {
                            let breadcrumb_width = (ui.available_width() - 42.0).max(80.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(breadcrumb_width, 28.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    egui::ScrollArea::horizontal()
                                        .id_salt("address_breadcrumbs")
                                        .max_width(breadcrumb_width)
                                        .stick_to_right(true)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            ui.horizontal_centered(|ui| {
                                                ui.spacing_mut().item_spacing.x = 2.0;
                                                if segments.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new("Choose a folder")
                                                            .color(palette.text_tertiary),
                                                    );
                                                }
                                                for (index, (label, target)) in
                                                    segments.iter().enumerate()
                                                {
                                                    if index > 0 {
                                                        let chevron = ui
                                                            .allocate_exact_size(
                                                                egui::vec2(12.0, 16.0),
                                                                egui::Sense::hover(),
                                                            )
                                                            .0;
                                                        draw_icon(
                                                            ui.painter(),
                                                            IconKind::ChevronRight,
                                                            chevron.shrink(2.0),
                                                            palette.text_tertiary,
                                                            1.4,
                                                        );
                                                    }
                                                    let text = egui::RichText::new(label)
                                                        .size(12.5)
                                                        .color(if index + 1 == segments.len() {
                                                            palette.text_primary
                                                        } else {
                                                            palette.text_secondary
                                                        });
                                                    if ui
                                                        .add(
                                                            egui::Button::new(text)
                                                                .frame(false)
                                                                .min_size(egui::vec2(0.0, 24.0)),
                                                        )
                                                        .on_hover_text(target.display().to_string())
                                                        .clicked()
                                                    {
                                                        breadcrumb_target = Some(target.clone());
                                                    }
                                                }
                                            });
                                        });
                                },
                            );
                            edit_address = icon_button(
                                ui,
                                IconKind::Edit,
                                None,
                                "Edit address (Ctrl+L)",
                                false,
                                &palette,
                            )
                            .clicked();
                            None
                        }
                    })
                    .inner
                })
                .inner;

            if let Some(path_resp) = path_edit_response {
                if !path_resp.has_focus() && !path_resp.lost_focus() {
                    path_resp.request_focus();
                }
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if enter {
                    let path = PathBuf::from(self.path_input.trim());
                    if path.is_dir() {
                        self.navigate_to(path);
                    } else {
                        self.status =
                            format!("Path not found or not accessible: {}", self.path_input);
                    }
                } else if escape || path_resp.lost_focus() {
                    self.path_input = self
                        .current_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.path_editing = false;
                }
            }
            if edit_address {
                self.execute_command(CommandId::EditAddress);
            }
            if let Some(target) = breadcrumb_target {
                if self.current_path.as_ref() != Some(&target) {
                    self.navigate_to(target);
                }
            }

            if icon_button(
                ui,
                IconKind::Search,
                Some("Find"),
                "Quick Find (Ctrl+F)",
                self.show_quick_find,
                &palette,
            )
            .clicked()
            {
                self.execute_command(CommandId::Find);
            }

            if icon_button(
                ui,
                IconKind::Image,
                None,
                "Find Product by Image (Ctrl+Shift+F)",
                self.show_image_search,
                &palette,
            )
            .clicked()
            {
                self.execute_command(CommandId::FindSimilar);
            }

            if icon_button(
                ui,
                IconKind::FolderOpen,
                Some("Browse"),
                "Browse for folder",
                false,
                &palette,
            )
            .clicked()
            {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.navigate_to(p);
                }
            }

            if icon_button(
                ui,
                IconKind::History,
                None,
                "Recent paths",
                self.show_history,
                &palette,
            )
            .clicked()
            {
                self.show_history = !self.show_history;
            }

            if ui
                .add_enabled_ui(self.command_enabled(CommandId::Refresh), |ui| {
                    icon_button(ui, IconKind::Refresh, None, "Refresh", false, &palette)
                })
                .inner
                .clicked()
            {
                self.execute_command(CommandId::Refresh);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let theme_icon = if self.effective_theme == "dark" {
                    IconKind::Sun
                } else {
                    IconKind::Moon
                };
                let tip = match self.config.theme.as_str() {
                    "light" => "Theme: Light. Click for Dark.",
                    "dark" => "Theme: Dark. Click for System.",
                    _ => "Theme: System. Click for Light.",
                };
                if icon_button(ui, theme_icon, None, tip, false, &palette).clicked() {
                    self.config.toggle_theme();
                    self.effective_theme = apply_theme(ui.ctx(), &self.config.theme).to_string();
                    self.status = format!("Switched to {} theme.", self.config.theme);
                }
                if icon_button(
                    ui,
                    IconKind::Settings,
                    None,
                    "Settings",
                    self.show_settings,
                    &palette,
                )
                .clicked()
                {
                    self.execute_command(CommandId::Settings);
                }
                if icon_button(
                    ui,
                    IconKind::Info,
                    None,
                    if self.config.details_panel_open {
                        "Hide details"
                    } else {
                        "Show details"
                    },
                    self.config.details_panel_open,
                    &palette,
                )
                .clicked()
                {
                    self.config.details_panel_open = !self.config.details_panel_open;
                    let _ = self.config.save();
                }
            });
        });
    }

    fn render_history_popover(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let mut nav_to: Option<PathBuf> = None;
        let mut remove_path: Option<String> = None;

        egui::Area::new(egui::Id::new("history_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(200.0, 50.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, palette.border_standard))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(0.0))
                    .show(ui, |ui| {
                        ui.set_width(390.0);
                        ui.vertical(|ui| {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                ui.label(
                                    egui::RichText::new("RECENT PATHS")
                                        .size(11.0)
                                        .strong()
                                        .color(palette.text_tertiary),
                                );
                            });
                            ui.add_space(8.0);
                            let sep = ui.available_rect_before_wrap();
                            ui.painter().line_segment(
                                [
                                    egui::pos2(sep.left(), sep.top()),
                                    egui::pos2(sep.right(), sep.top()),
                                ],
                                egui::Stroke::new(1.0, palette.border_soft),
                            );
                            ui.add_space(1.0);

                            if self.config.path_history.is_empty() {
                                ui.add_space(16.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        egui::RichText::new("No recent paths yet")
                                            .size(12.0)
                                            .color(palette.text_tertiary),
                                    );
                                });
                                ui.add_space(16.0);
                            } else {
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| {
                                        for path_str in self.config.path_history.iter() {
                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(ui.available_width(), 40.0),
                                                egui::Sense::click(),
                                            );
                                            if resp.hovered() {
                                                ui.painter().rect_filled(
                                                    rect,
                                                    0.0,
                                                    palette.surface_inset,
                                                );
                                            }
                                            let icon_rect = egui::Rect::from_min_size(
                                                rect.min + egui::vec2(14.0, 13.0),
                                                egui::vec2(14.0, 14.0),
                                            );
                                            draw_icon(
                                                ui.painter(),
                                                IconKind::Folder,
                                                icon_rect,
                                                palette.text_tertiary,
                                                1.5,
                                            );
                                            let text_rect = egui::Rect::from_min_max(
                                                rect.min + egui::vec2(38.0, 0.0),
                                                rect.max - egui::vec2(42.0, 0.0),
                                            );
                                            ui.painter().text(
                                                text_rect.left_center(),
                                                egui::Align2::LEFT_CENTER,
                                                elide_middle(path_str, 48),
                                                egui::FontId::proportional(12.5),
                                                palette.text_primary,
                                            );

                                            let remove_rect = egui::Rect::from_center_size(
                                                rect.right_center() - egui::vec2(20.0, 0.0),
                                                egui::vec2(24.0, 24.0),
                                            );
                                            let rem_resp = ui.interact(
                                                remove_rect,
                                                ui.id().with(("hist_rm", path_str)),
                                                egui::Sense::click(),
                                            );
                                            if rem_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    remove_rect,
                                                    4.0,
                                                    palette.surface_inset,
                                                );
                                                draw_icon(
                                                    ui.painter(),
                                                    IconKind::Close,
                                                    remove_rect.shrink(7.0),
                                                    palette.error,
                                                    1.6,
                                                );
                                            }
                                            if rem_resp.clicked() {
                                                remove_path = Some(path_str.clone());
                                            } else if resp.clicked() {
                                                let p = PathBuf::from(path_str);
                                                if p.is_dir() {
                                                    nav_to = Some(p);
                                                } else {
                                                    self.status = format!(
                                                        "Path not found or not accessible: {}",
                                                        path_str
                                                    );
                                                }
                                            }
                                        }
                                    });
                            }
                        });
                    });
            });

        if let Some(path) = remove_path {
            self.config.remove_path(&path);
        }
        if let Some(path) = nav_to {
            self.navigate_to(path);
        }
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        let visible_status = self.scan_status().unwrap_or_else(|| self.status.clone());
        ui.horizontal_centered(|ui| {
            let dot_color = status_color(&visible_status, &palette);
            let dot_rect = ui
                .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                .0;
            ui.painter()
                .circle_filled(dot_rect.center(), 3.0, dot_color);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&visible_status)
                    .size(11.5)
                    .color(palette.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let active_tasks = self.operations.active_count()
                    + self.updates.active_count()
                    + self.power_tools.active_count();
                let task_label = if active_tasks > 0 {
                    format!("Tasks ({active_tasks})")
                } else {
                    "Tasks".into()
                };
                if ui
                    .selectable_label(self.show_task_center, task_label)
                    .on_hover_text("Open Task Center")
                    .clicked()
                {
                    self.show_task_center = !self.show_task_center;
                }
                if let Some(path) = &self.current_path {
                    ui.label(
                        egui::RichText::new(elide_middle(&path.to_string_lossy(), 64))
                            .size(11.5)
                            .color(palette.text_tertiary),
                    );
                }
            });
        });
    }

    fn render_settings_window(&mut self, ctx: &egui::Context) {
        if let Some(command) = self.shortcut_capture {
            if let Some(captured) = capture_shortcut(ctx) {
                match captured {
                    CapturedShortcut::Cancel => {
                        self.shortcut_capture = None;
                    }
                    CapturedShortcut::Value(value) => {
                        *self.shortcut_draft.get_mut(command) = value;
                        self.shortcut_capture = None;
                    }
                }
            }
        }

        let palette = Palette::for_theme(&self.effective_theme);
        let mut open = self.show_settings;
        let mut save_shortcuts = false;
        let mut reset_shortcuts = false;
        let mut import_shortcuts = false;
        let mut export_shortcuts = false;
        let mut config_changed = false;
        let mut hidden_files_changed = false;
        let mut index_enabled_changed = false;
        let mut index_rules_changed = false;
        let mut thumbnail_budget_changed = false;
        let mut rebuild_index = false;
        let mut pause_index = false;
        let mut resume_index = false;
        let mut cancel_index = false;
        let mut choose_model_pack = false;
        let mut clear_model_pack = false;
        let mut refresh_model_pack = false;
        let mut check_updates_now = false;
        let mut close_window = false;
        let index_snapshot = self.index.snapshot().clone();
        let update_status = self.updates.state().status_text();
        let update_active = self.updates.is_active();

        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size(fit_window_size(ctx, 660.0, 520.0))
            .min_width(560.0)
            .min_height(440.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for tab in SettingsTab::ALL {
                        if ui
                            .selectable_label(self.settings_tab == tab, tab.label())
                            .clicked()
                        {
                            self.settings_tab = tab;
                            self.shortcut_capture = None;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_tab {
                        SettingsTab::General => {
                            settings_heading(ui, "General", &palette);
                            settings_value_row(
                                ui,
                                "Version",
                                &crate::version::display(),
                                &palette,
                            );
                            settings_value_row(
                                ui,
                                "Release source",
                                &crate::updater::repository_label(),
                                &palette,
                            );
                            settings_value_row(ui, "Updates", &update_status, &palette);
                            if ui
                                .add_enabled(
                                    !update_active,
                                    egui::Button::new("Check for Updates"),
                                )
                                .clicked()
                            {
                                check_updates_now = true;
                            }
                            ui.add_space(12.0);
                            config_changed |= ui
                                .checkbox(
                                    &mut self.config.check_updates_on_startup,
                                    "Check for updates on startup",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut self.config.show_selection_checkboxes,
                                    "Show selection checkboxes",
                                )
                                .changed();
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} recent paths saved",
                                    self.config.path_history.len()
                                ))
                                .color(palette.text_secondary),
                            );
                        }
                        SettingsTab::Appearance => {
                            settings_heading(ui, "Appearance", &palette);
                            ui.horizontal(|ui| {
                                ui.label("Theme");
                                for (value, label) in
                                    [("light", "Light"), ("dark", "Dark"), ("system", "System")]
                                {
                                    config_changed |= ui
                                        .selectable_value(
                                            &mut self.config.theme,
                                            value.to_string(),
                                            label,
                                        )
                                        .changed();
                                }
                            });
                            ui.add_space(14.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Active palette: {}",
                                    capitalize(&self.effective_theme)
                                ))
                                .color(palette.text_secondary),
                            );
                            ui.add_space(18.0);
                            settings_heading(ui, "Thumbnail Memory", &palette);
                            let budget_response = ui.add(
                                egui::Slider::new(
                                    &mut self.config.thumbnail_memory_budget_mb,
                                    MIN_THUMBNAIL_MEMORY_BUDGET_MB
                                        ..=MAX_THUMBNAIL_MEMORY_BUDGET_MB,
                                )
                                .suffix(" MB")
                                .show_value(true),
                            );
                            thumbnail_budget_changed |= budget_response.changed();
                            config_changed |= budget_response.changed();
                            settings_value_row(
                                ui,
                                "In use",
                                &format!(
                                    "{} / {}",
                                    format_size(self.thumb_mgr.cached_bytes() as u64),
                                    format_size(self.thumb_mgr.byte_budget() as u64)
                                ),
                                &palette,
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Least-recently-used thumbnails are released automatically when this limit is reached.",
                                )
                                .size(11.5)
                                .color(palette.text_secondary),
                            );
                        }
                        SettingsTab::Shortcuts => {
                            settings_heading(ui, "Keyboard Shortcuts", &palette);

                            if let Some(error) = self.shortcut_draft.validation_error() {
                                ui.colored_label(palette.error, error);
                                ui.add_space(8.0);
                            }

                            for command in CommandId::CONFIGURABLE {
                                shortcut_capture_row(
                                    ui,
                                    command,
                                    &mut self.shortcut_draft,
                                    &mut self.shortcut_capture,
                                    &palette,
                                );
                            }

                            ui.add_space(10.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Save").clicked() {
                                    save_shortcuts = true;
                                }
                                if ui.button("Reset Defaults").clicked() {
                                    reset_shortcuts = true;
                                }
                                if ui.button("Import Profile").clicked() {
                                    import_shortcuts = true;
                                }
                                if ui.button("Export Profile").clicked() {
                                    export_shortcuts = true;
                                }
                            });
                        }
                        SettingsTab::SearchIndex => {
                            settings_heading(ui, "Search & Index", &palette);
                            let enabled_response = ui.checkbox(
                                &mut self.config.indexing_enabled,
                                "Build and maintain a local product index",
                            );
                            index_enabled_changed |= enabled_response.changed();
                            config_changed |= enabled_response.changed();
                            ui.add_space(10.0);
                            settings_value_row(
                                ui,
                                "Status",
                                index_snapshot.state.label(),
                                &palette,
                            );
                            settings_value_row(
                                ui,
                                "Products",
                                &index_snapshot.products_indexed.to_string(),
                                &palette,
                            );
                            settings_value_row(
                                ui,
                                "Images",
                                &index_snapshot.images_indexed.to_string(),
                                &palette,
                            );
                            if let Some(source) = &index_snapshot.current_source {
                                settings_value_row(ui, "Current source", source, &palette);
                            }
                            if let Some(error) = &index_snapshot.last_error {
                                ui.colored_label(palette.error, error);
                            }
                            if index_snapshot.recovered_corrupt_database {
                                ui.label(
                                    egui::RichText::new(
                                        "A damaged index was preserved and a clean database was created.",
                                    )
                                    .size(11.5)
                                    .color(palette.warning),
                                );
                            }
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                match index_snapshot.state {
                                    IndexState::Indexing => {
                                        if ui.button("Pause").clicked() {
                                            pause_index = true;
                                        }
                                        if ui.button("Stop").clicked() {
                                            cancel_index = true;
                                        }
                                    }
                                    IndexState::Paused => {
                                        if ui.button("Resume").clicked() {
                                            resume_index = true;
                                        }
                                        if ui.button("Stop").clicked() {
                                            cancel_index = true;
                                        }
                                    }
                                    _ => {}
                                }
                                if ui
                                    .add_enabled(
                                        self.config.indexing_enabled
                                            && !matches!(
                                                index_snapshot.state,
                                                IndexState::Indexing | IndexState::Paused
                                            ),
                                        egui::Button::new("Rebuild Index"),
                                    )
                                    .clicked()
                                {
                                    rebuild_index = true;
                                }
                            });
                            ui.add_space(16.0);
                            settings_heading(ui, "Source Rules", &palette);
                            if self.config.sources.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "Add a source from the navigation sidebar.",
                                    )
                                    .color(palette.text_secondary),
                                );
                            }
                            for (index, source) in self.config.sources.iter_mut().enumerate() {
                                let source_key =
                                    crate::search::normalize_path_key(Path::new(&source.path));
                                let availability = index_snapshot
                                    .sources
                                    .get(&source_key)
                                    .map(|status| {
                                        if status.available {
                                            format!(
                                                "Available - {} products / {} images",
                                                status.products_indexed, status.images_indexed
                                            )
                                        } else {
                                            status
                                                .last_error
                                                .clone()
                                                .unwrap_or_else(|| "Unavailable".into())
                                        }
                                    })
                                    .unwrap_or_else(|| {
                                        if Path::new(&source.path).is_dir() {
                                            "Waiting for index".into()
                                        } else {
                                            "Unavailable".into()
                                        }
                                    });
                                egui::CollapsingHeader::new(&source.name)
                                    .id_salt(("source_rule", index))
                                    .default_open(index == 0)
                                    .show(ui, |ui| {
                                        settings_path_row(
                                            ui,
                                            "Folder",
                                            Path::new(&source.path),
                                            &palette,
                                        );
                                        settings_value_row(
                                            ui,
                                            "Availability",
                                            &availability,
                                            &palette,
                                        );
                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Product code");
                                            egui::ComboBox::from_id_salt((
                                                "source_code_mode",
                                                index,
                                            ))
                                            .selected_text(source.code_mode.label())
                                            .show_ui(ui, |ui| {
                                                for mode in ProductCodeMode::ALL {
                                                    index_rules_changed |= ui
                                                        .selectable_value(
                                                            &mut source.code_mode,
                                                            mode,
                                                            mode.label(),
                                                        )
                                                        .changed();
                                                }
                                            });
                                        });
                                        if source.code_mode != ProductCodeMode::FolderName {
                                            let hint = if source.code_mode
                                                == ProductCodeMode::Prefix
                                            {
                                                "Prefix to remove, for example SKU"
                                            } else {
                                                "Regex; capture group 1 becomes the product code"
                                            };
                                            index_rules_changed |= ui
                                                .add(
                                                    egui::TextEdit::singleline(
                                                        &mut source.code_pattern,
                                                    )
                                                    .hint_text(hint)
                                                    .desired_width(ui.available_width()),
                                                )
                                                .changed();
                                        }
                                        if let Err(error) =
                                            crate::search::validate_extraction_rule(source)
                                        {
                                            ui.colored_label(palette.error, error);
                                        }
                                        ui.horizontal(|ui| {
                                            ui.label("Reconcile every");
                                            index_rules_changed |= ui
                                                .add(
                                                    egui::DragValue::new(
                                                        &mut source.reconciliation_minutes,
                                                    )
                                                    .range(1..=1_440)
                                                    .suffix(" min"),
                                                )
                                                .changed();
                                        });
                                    });
                                ui.add_space(6.0);
                            }
                            config_changed |= index_rules_changed;
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.label("Similarity results");
                                config_changed |= ui
                                    .add(
                                        egui::Slider::new(
                                            &mut self.config.similarity_result_count,
                                            5..=50,
                                        )
                                        .show_value(true),
                                    )
                                    .changed();
                            });
                            ui.add_space(14.0);
                            settings_heading(ui, "AI Model Pack", &palette);
                            settings_value_row(
                                ui,
                                "Status",
                                &self.model_pack_status,
                                &palette,
                            );
                            if self.config.model_pack_path.is_empty() {
                                settings_value_row(
                                    ui,
                                    "Location",
                                    "Auto-detect beside LuxePix",
                                    &palette,
                                );
                            } else {
                                settings_path_row(
                                    ui,
                                    "Location",
                                    Path::new(&self.config.model_pack_path),
                                    &palette,
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Choose Folder").clicked() {
                                    choose_model_pack = true;
                                }
                                if ui
                                    .add_enabled(
                                        !self.config.model_pack_path.is_empty(),
                                        egui::Button::new("Use Auto-detect"),
                                    )
                                    .clicked()
                                {
                                    clear_model_pack = true;
                                }
                                if ui.button("Recheck").clicked() {
                                    refresh_model_pack = true;
                                }
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Image matching always falls back to local pHash if the optional helper or model is unavailable.",
                                )
                                .size(11.5)
                                .color(palette.text_secondary),
                            );
                            ui.add_space(12.0);
                            settings_path_row(ui, "Index", &self.storage.index_dir, &palette);
                            settings_path_row(
                                ui,
                                "Similarity data",
                                &self.storage.similarity_dir,
                                &palette,
                            );
                        }
                        SettingsTab::FileOperations => {
                            settings_heading(ui, "File Operations", &palette);
                            hidden_files_changed |= ui
                                .checkbox(&mut self.config.show_hidden_files, "Show hidden files")
                                .changed();
                            config_changed |= hidden_files_changed;
                            config_changed |= ui
                                .checkbox(
                                    &mut self.config.confirm_recycle_delete,
                                    "Confirm before moving items to Recycle Bin",
                                )
                                .changed();
                            let mut permanent_confirmation = true;
                            ui.add_enabled(
                                false,
                                egui::Checkbox::new(
                                    &mut permanent_confirmation,
                                    "Always confirm permanent deletion",
                                ),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Replace collisions always require an explicit choice.",
                                )
                                .size(12.0)
                                .color(palette.text_secondary),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(
                                    if self.clipboard.is_some()
                                        || windows_shell::clipboard_has_files()
                                    {
                                        "The file clipboard contains selected items."
                                    } else {
                                        "The file clipboard is empty."
                                    },
                                )
                                .color(palette.text_secondary),
                            );
                        }
                        SettingsTab::Advanced => {
                            settings_heading(ui, "Advanced", &palette);
                            settings_value_row(
                                ui,
                                "Storage mode",
                                self.storage.mode_label(),
                                &palette,
                            );
                            settings_path_row(ui, "Data root", &self.storage.root, &palette);
                            settings_path_row(ui, "Config", &self.storage.config, &palette);
                            settings_path_row(ui, "Session state", &self.storage.state, &palette);
                            settings_path_row(
                                ui,
                                "Thumbnail cache",
                                &self.storage.thumbnails_dir,
                                &palette,
                            );
                            settings_path_row(
                                ui,
                                "Update staging",
                                &self.storage.updates_dir,
                                &palette,
                            );
                            settings_path_row(
                                ui,
                                "Install folder",
                                &self.storage.executable_dir,
                                &palette,
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.button("Open Data Folder").clicked() {
                                    Self::open_file_external(&self.storage.root);
                                }
                                if self.storage.mode == crate::storage::StorageMode::Installed
                                    && ui.button("Open Install Folder").clicked()
                                {
                                    Self::open_file_external(&self.storage.executable_dir);
                                }
                            });
                        }
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close_window = true;
                    }
                });
            });

        if reset_shortcuts {
            self.shortcut_draft = ShortcutConfig::default();
            self.shortcut_capture = None;
            self.status = "Shortcut defaults loaded. Save to apply them.".into();
        }

        if save_shortcuts {
            if let Some(error) = self.shortcut_draft.validation_error() {
                self.status = format!("Shortcuts were not saved: {error}");
            } else {
                self.config.shortcuts = self.shortcut_draft.clone();
                match self.config.save() {
                    Ok(()) => self.status = "Shortcut settings saved.".into(),
                    Err(error) => self.status = format!("Could not save shortcuts: {error}"),
                }
            }
        }

        if import_shortcuts {
            self.import_shortcut_profile();
        }
        if export_shortcuts {
            self.export_shortcut_profile();
        }

        if choose_model_pack {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                self.config.model_pack_path = path.to_string_lossy().to_string();
                config_changed = true;
                refresh_model_pack = true;
            }
        }
        if clear_model_pack {
            self.config.model_pack_path.clear();
            config_changed = true;
            refresh_model_pack = true;
        }

        if config_changed {
            self.effective_theme = apply_theme(ctx, &self.config.theme).to_string();
            if let Err(error) = self.config.save() {
                self.status = format!("Could not save settings: {error}");
            }
        }
        if thumbnail_budget_changed {
            self.thumb_mgr
                .set_byte_budget(self.config.thumbnail_memory_budget_bytes());
        }
        if refresh_model_pack {
            self.refresh_model_pack();
        }
        if hidden_files_changed {
            self.refresh_current_view();
        }
        if index_enabled_changed {
            match self.index.set_enabled(self.config.indexing_enabled) {
                Ok(()) if self.config.indexing_enabled => self.request_index_rebuild(),
                Ok(()) => {
                    self.pending_index_rebuild = false;
                    self.pending_index_reconcile = false;
                }
                Err(error) => {
                    self.config.indexing_enabled = false;
                    let _ = self.config.save();
                    self.status = format!("Could not enable indexing: {error}");
                }
            }
        }
        if index_rules_changed || rebuild_index {
            if let Some(error) = self
                .config
                .sources
                .iter()
                .find_map(|source| crate::search::validate_extraction_rule(source).err())
            {
                self.status = format!("Search index was not rebuilt: {error}");
            } else {
                self.request_index_rebuild();
            }
        }
        if pause_index {
            self.index.pause();
        }
        if resume_index {
            self.index.resume();
        }
        if cancel_index {
            self.index.cancel();
        }
        if check_updates_now {
            self.start_update_check(true);
        }

        if close_window {
            open = false;
        }
        if !open {
            self.shortcut_capture = None;
            self.shortcut_draft = self.config.shortcuts.clone();
        }
        self.show_settings = open;
    }

    fn import_shortcut_profile(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("LuxePix shortcut profile", &["json"])
            .pick_file()
        else {
            return;
        };

        match fs::read_to_string(&path) {
            Ok(json) => match self.config.import_shortcut_profile(&json) {
                Ok(()) => {
                    self.shortcut_draft = self.config.shortcuts.clone();
                    self.shortcut_capture = None;
                    self.status = format!(
                        "Imported shortcut profile from {}.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                Err(error) => {
                    self.status = format!("Shortcut profile was not imported: {error}");
                }
            },
            Err(error) => {
                self.status = format!("Could not read shortcut profile: {error}");
            }
        }
    }

    fn export_shortcut_profile(&mut self) {
        if let Some(error) = self.shortcut_draft.validation_error() {
            self.status = format!("Shortcut profile was not exported: {error}");
            return;
        }

        let Some(path) = rfd::FileDialog::new()
            .add_filter("LuxePix shortcut profile", &["json"])
            .set_file_name("LuxePix-shortcuts.json")
            .save_file()
        else {
            return;
        };

        let mut export_config = self.config.clone();
        export_config.shortcuts = self.shortcut_draft.clone();
        match export_config.export_shortcut_profile() {
            Ok(json) => match transactional_write(&path, json.as_bytes()) {
                Ok(()) => {
                    self.status = format!("Exported shortcut profile to {}.", path.display());
                }
                Err(error) => {
                    self.status = format!("Could not write shortcut profile: {error}");
                }
            },
            Err(error) => {
                self.status = format!("Could not encode shortcut profile: {error}");
            }
        }
    }

    fn render_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let count = pending.paths.len();
        let sample = pending
            .paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let permanent = pending.mode == DeleteMode::Permanent;
        let title = if permanent {
            "Delete Permanently"
        } else {
            "Move to Recycle Bin"
        };
        let question = if permanent {
            format!("Permanently delete {count} selected item(s)?")
        } else {
            format!("Move {count} selected item(s) to Recycle Bin?")
        };
        let explanation = if permanent {
            "This cannot be undone by LuxePix or Windows Recycle Bin."
        } else {
            "You can restore these items later from Windows Recycle Bin."
        };
        let confirm_label = if permanent {
            "Delete Permanently"
        } else {
            "Move to Recycle Bin"
        };
        let mut confirm = false;
        let mut cancel = false;
        let palette = Palette::for_theme(&self.effective_theme);

        egui::Window::new(title)
            .resizable(false)
            .collapsible(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(question);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(elide_middle(&sample, 72))
                        .size(12.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(explanation)
                        .size(12.0)
                        .color(if permanent {
                            palette.error
                        } else {
                            palette.text_secondary
                        }),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new(confirm_label).color(if permanent {
                            palette.error
                        } else {
                            palette.text_primary
                        }))
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if confirm {
            self.confirm_delete();
        } else if cancel {
            self.pending_delete = None;
        }
    }

    fn render_collision_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_paste else {
            return;
        };
        let collisions: Vec<_> = pending
            .preparation
            .candidates
            .iter()
            .filter(|candidate| candidate.collision)
            .collect();
        let Some(current) = collisions
            .iter()
            .find(|candidate| !pending.choices.contains_key(&candidate.destination))
            .copied()
        else {
            return;
        };
        let current_number = collisions
            .iter()
            .position(|candidate| candidate.destination == current.destination)
            .unwrap_or(0)
            + 1;
        let total = collisions.len();
        let source = current.source.clone();
        let destination = current.destination.clone();
        let mut apply_to_all = pending.apply_to_all;
        let mut choice = None;
        let mut cancel = false;
        let palette = Palette::for_theme(&self.effective_theme);

        egui::Window::new("File Name Collision")
            .resizable(false)
            .collapsible(false)
            .default_width(500.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("Item {current_number} of {total}"))
                        .size(11.5)
                        .color(palette.text_tertiary),
                );
                ui.add_space(4.0);
                ui.label("An item with the same name already exists.");
                ui.add_space(10.0);
                settings_path_row(ui, "From", &source, &palette);
                settings_path_row(ui, "Existing", &destination, &palette);
                ui.add_space(10.0);
                ui.checkbox(
                    &mut apply_to_all,
                    "Apply this choice to all remaining collisions",
                );
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(egui::RichText::new("Replace").color(palette.error))
                        .on_hover_text("Replace the existing item")
                        .clicked()
                    {
                        choice = Some(CollisionChoice::Replace);
                    }
                    if ui
                        .button("Keep Both")
                        .on_hover_text("Create a new name and preserve both items")
                        .clicked()
                    {
                        choice = Some(CollisionChoice::KeepBoth);
                    }
                    if ui
                        .button("Skip")
                        .on_hover_text("Do not copy or move this item")
                        .clicked()
                    {
                        choice = Some(CollisionChoice::Skip);
                    }
                    if ui.button("Cancel Paste").clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.pending_paste = None;
            self.status = "Paste cancelled before files were changed.".into();
            return;
        }
        let Some(choice) = choice else {
            if let Some(pending) = &mut self.pending_paste {
                pending.apply_to_all = apply_to_all;
            }
            return;
        };

        if let Some(pending) = &mut self.pending_paste {
            pending.apply_to_all = apply_to_all;
            pending.choices.insert(destination, choice);
            if apply_to_all {
                for candidate in &pending.preparation.candidates {
                    if candidate.collision {
                        pending
                            .choices
                            .entry(candidate.destination.clone())
                            .or_insert(choice);
                    }
                }
            }
        }

        let ready = self.pending_paste.as_ref().is_some_and(|pending| {
            pending
                .preparation
                .candidates
                .iter()
                .filter(|candidate| candidate.collision)
                .all(|candidate| pending.choices.contains_key(&candidate.destination))
        });
        if ready {
            let pending = self.pending_paste.take().expect("pending paste");
            match pending.preparation.resolve(&pending.choices) {
                Ok(request) => self.start_transfer(request),
                Err(error) => self.status = format!("Could not resolve paste collisions: {error}"),
            }
        }
    }

    fn render_task_center(&mut self, ctx: &egui::Context) {
        enum TaskAction {
            Pause(OperationId),
            Resume(OperationId),
            Cancel(OperationId),
            Retry(OperationId),
            CancelUpdate,
            ApplyUpdate,
            CancelPowerTool,
        }

        let palette = Palette::for_theme(&self.effective_theme);
        let mut open = self.show_task_center;
        let mut actions = Vec::new();
        let mut clear_finished = false;
        let update_state = self.updates.state().clone();
        let power_task = self.power_tools.active().cloned();

        egui::Window::new("Task Center")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size(fit_window_size(ctx, 620.0, 420.0))
            .min_width(500.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let active = self.operations.active_count()
                        + self.updates.active_count()
                        + self.power_tools.active_count();
                    ui.label(
                        egui::RichText::new(format!("{active} active"))
                            .color(palette.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Clear Finished").clicked() {
                            clear_finished = true;
                        }
                    });
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut any = false;
                        match &update_state {
                            UpdateState::Checking => {
                                any = true;
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("Check for LuxePix updates")
                                            .strong()
                                            .color(palette.text_primary),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.spinner();
                                            ui.label(
                                                egui::RichText::new("Running")
                                                    .size(11.5)
                                                    .color(palette.accent),
                                            );
                                        },
                                    );
                                });
                                ui.label(
                                    egui::RichText::new(crate::updater::repository_label())
                                        .size(11.5)
                                        .color(palette.text_secondary),
                                );
                                ui.add_space(7.0);
                                ui.separator();
                            }
                            UpdateState::Downloading { offer, downloaded } => {
                                any = true;
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Download LuxePix {}",
                                            offer.version
                                        ))
                                        .strong()
                                        .color(palette.text_primary),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new("Running")
                                                    .size(11.5)
                                                    .color(palette.accent),
                                            );
                                        },
                                    );
                                });
                                let fraction = if offer.asset.byte_length == 0 {
                                    0.0
                                } else {
                                    (*downloaded as f64 / offer.asset.byte_length as f64)
                                        .clamp(0.0, 1.0) as f32
                                };
                                ui.add(
                                    egui::ProgressBar::new(fraction)
                                        .desired_width(ui.available_width())
                                        .show_percentage(),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} of {}",
                                        crate::updater::format_bytes(*downloaded),
                                        crate::updater::format_bytes(offer.asset.byte_length)
                                    ))
                                    .size(11.5)
                                    .color(palette.text_secondary),
                                );
                                if ui.small_button("Cancel").clicked() {
                                    actions.push(TaskAction::CancelUpdate);
                                }
                                ui.add_space(7.0);
                                ui.separator();
                            }
                            UpdateState::Ready(package) => {
                                any = true;
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "LuxePix {} update",
                                            package.offer.version
                                        ))
                                        .strong()
                                        .color(palette.text_primary),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new("Verified")
                                                    .size(11.5)
                                                    .color(palette.success),
                                            );
                                        },
                                    );
                                });
                                ui.add(
                                    egui::ProgressBar::new(1.0)
                                        .desired_width(ui.available_width())
                                        .show_percentage(),
                                );
                                if ui.small_button("Install and Restart").clicked() {
                                    actions.push(TaskAction::ApplyUpdate);
                                }
                                ui.add_space(7.0);
                                ui.separator();
                            }
                            UpdateState::Failed {
                                message,
                                manual: true,
                            } => {
                                any = true;
                                ui.add_space(7.0);
                                ui.label(
                                    egui::RichText::new("LuxePix update")
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.label(
                                    egui::RichText::new(message).size(11.5).color(palette.error),
                                );
                                ui.add_space(7.0);
                                ui.separator();
                            }
                            UpdateState::Idle
                            | UpdateState::Current
                            | UpdateState::Available(_)
                            | UpdateState::Failed { manual: false, .. } => {}
                        }
                        if let Some(task) = &power_task {
                            any = true;
                            ui.add_space(7.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&task.title)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(task.kind.label())
                                                .size(11.5)
                                                .color(palette.accent),
                                        );
                                    },
                                );
                            });
                            if task.total > 0 {
                                ui.add(
                                    egui::ProgressBar::new(task.progress_fraction())
                                        .desired_width(ui.available_width())
                                        .show_percentage(),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} of {} item(s)",
                                        task.completed, task.total
                                    ))
                                    .size(11.5)
                                    .color(palette.text_secondary),
                                );
                            } else {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        egui::RichText::new("Preparing...")
                                            .size(11.5)
                                            .color(palette.text_secondary),
                                    );
                                });
                            }
                            if let Some(path) = &task.current_path {
                                ui.label(
                                    egui::RichText::new(elide_middle(&path.to_string_lossy(), 78))
                                        .size(11.5)
                                        .color(palette.text_tertiary),
                                );
                            }
                            if ui.small_button("Cancel").clicked() {
                                actions.push(TaskAction::CancelPowerTool);
                            }
                            ui.add_space(7.0);
                            ui.separator();
                        }
                        for record in self.operations.records().rev() {
                            any = true;
                            ui.add_space(7.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&record.title)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(record.state.label())
                                                .size(11.5)
                                                .color(operation_state_color(
                                                    record.state,
                                                    &palette,
                                                )),
                                        );
                                    },
                                );
                            });
                            ui.add(
                                egui::ProgressBar::new(record.progress_fraction())
                                    .desired_width(ui.available_width())
                                    .show_percentage(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(operation_metrics(record))
                                        .size(11.5)
                                        .color(palette.text_secondary),
                                );
                                if let Some(path) = &record.current_path {
                                    ui.label(
                                        egui::RichText::new(elide_middle(
                                            &path.to_string_lossy(),
                                            68,
                                        ))
                                        .size(11.5)
                                        .color(palette.text_tertiary),
                                    );
                                }
                            });
                            ui.horizontal(|ui| {
                                match record.state {
                                    OperationState::Running if record.pausable() => {
                                        if ui.small_button("Pause").clicked() {
                                            actions.push(TaskAction::Pause(record.id));
                                        }
                                    }
                                    OperationState::Paused => {
                                        if ui.small_button("Resume").clicked() {
                                            actions.push(TaskAction::Resume(record.id));
                                        }
                                    }
                                    _ => {}
                                }
                                if record.state.is_active() && ui.small_button("Cancel").clicked() {
                                    actions.push(TaskAction::Cancel(record.id));
                                }
                                if record.state.can_retry() && ui.small_button("Retry").clicked() {
                                    actions.push(TaskAction::Retry(record.id));
                                }
                            });
                            if !record.errors.is_empty() {
                                egui::CollapsingHeader::new(format!(
                                    "{} error(s)",
                                    record.errors.len()
                                ))
                                .show(ui, |ui| {
                                    for error in &record.errors {
                                        let path = if error.path.as_os_str().is_empty() {
                                            String::new()
                                        } else {
                                            format!("{}: ", error.path.display())
                                        };
                                        ui.label(
                                            egui::RichText::new(format!("{path}{}", error.message))
                                                .size(11.5)
                                                .color(palette.error),
                                        );
                                    }
                                });
                            }
                            ui.add_space(7.0);
                            ui.separator();
                        }
                        if !any {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    egui::RichText::new("No file operations yet.")
                                        .color(palette.text_secondary),
                                );
                            });
                        }
                    });
            });

        for action in actions {
            match action {
                TaskAction::Pause(id) => self.operations.pause(id),
                TaskAction::Resume(id) => self.operations.resume(id),
                TaskAction::Cancel(id) => self.operations.cancel(id),
                TaskAction::Retry(id) => match self.operations.retry(id) {
                    Ok(Some(_)) => self.status = "Operation retry started.".into(),
                    Ok(None) => {}
                    Err(error) => self.status = error,
                },
                TaskAction::CancelUpdate => {
                    self.updates.cancel_download();
                    self.status = "Cancelling update download...".into();
                }
                TaskAction::ApplyUpdate => self.apply_verified_update(ctx),
                TaskAction::CancelPowerTool => {
                    self.power_tools.cancel();
                    self.status = "Cancelling power tool...".into();
                }
            }
        }
        if clear_finished {
            self.operations.clear_finished();
        }
        self.show_task_center = open;
    }

    fn render_update_dialog(&mut self, ctx: &egui::Context) {
        enum UpdateAction {
            Download(crate::updater::UpdateOffer),
            Apply,
            CancelDownload,
            Later,
            Retry,
            OpenRelease(String),
        }

        let palette = Palette::for_theme(&self.effective_theme);
        let state = self.updates.state().clone();
        let mut open = self.show_update_dialog;
        let mut action = None;

        egui::Window::new("LuxePix Update")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(fit_window_size(ctx, 560.0, 390.0))
            .min_width(480.0)
            .show(ctx, |ui| match &state {
                UpdateState::Available(offer) => {
                    ui.label(
                        egui::RichText::new(format!("LuxePix {} is available", offer.version))
                            .size(19.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(8.0);
                    settings_value_row(ui, "Installed", &crate::version::display(), &palette);
                    settings_value_row(ui, "Available", &offer.version.to_string(), &palette);
                    settings_value_row(ui, "Published", &offer.published_at, &palette);
                    settings_value_row(ui, "Package", offer.asset.mode.label(), &palette);
                    settings_value_row(
                        ui,
                        "Download",
                        &crate::updater::format_bytes(offer.asset.byte_length),
                        &palette,
                    );
                    ui.add_space(12.0);
                    settings_heading(ui, "Release Notes", &palette);
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(if offer.notes.is_empty() {
                                    "No release notes were supplied."
                                } else {
                                    &offer.notes
                                })
                                .color(palette.text_secondary),
                            );
                        });
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Download and Update").clicked() {
                            action = Some(UpdateAction::Download(offer.clone()));
                        }
                        if ui.button("Later").clicked() {
                            action = Some(UpdateAction::Later);
                        }
                        if ui.button("Open Release Page").clicked() {
                            action = Some(UpdateAction::OpenRelease(
                                offer.release_url.clone(),
                            ));
                        }
                    });
                }
                UpdateState::Downloading { offer, downloaded } => {
                    ui.label(
                        egui::RichText::new(format!("Downloading LuxePix {}", offer.version))
                            .size(19.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(14.0);
                    let fraction = if offer.asset.byte_length == 0 {
                        0.0
                    } else {
                        (*downloaded as f64 / offer.asset.byte_length as f64).clamp(0.0, 1.0)
                            as f32
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(ui.available_width())
                            .show_percentage(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{} of {}",
                            crate::updater::format_bytes(*downloaded),
                            crate::updater::format_bytes(offer.asset.byte_length)
                        ))
                        .color(palette.text_secondary),
                    );
                    ui.add_space(12.0);
                    if ui.button("Cancel Download").clicked() {
                        action = Some(UpdateAction::CancelDownload);
                    }
                }
                UpdateState::Ready(package) => {
                    ui.label(
                        egui::RichText::new(format!(
                            "LuxePix {} is verified",
                            package.offer.version
                        ))
                        .size(19.0)
                        .strong()
                        .color(palette.text_primary),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            "The package byte length and SHA-256 match the signed release manifest.",
                        )
                        .color(palette.success),
                    );
                    ui.add_space(10.0);
                    settings_path_row(ui, "Package", &package.path, &palette);
                    settings_value_row(
                        ui,
                        "Mode",
                        package.offer.asset.mode.label(),
                        &palette,
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Install and Restart").clicked() {
                            action = Some(UpdateAction::Apply);
                        }
                        if ui.button("Later").clicked() {
                            action = Some(UpdateAction::Later);
                        }
                    });
                }
                UpdateState::Failed { message, .. } => {
                    ui.label(
                        egui::RichText::new("Update could not continue")
                            .size(19.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(message).color(palette.error));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Try Again").clicked() {
                            action = Some(UpdateAction::Retry);
                        }
                        if ui.button("Close").clicked() {
                            action = Some(UpdateAction::Later);
                        }
                    });
                }
                UpdateState::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking GitHub Releases...");
                    });
                }
                UpdateState::Current => {
                    ui.label(
                        egui::RichText::new("LuxePix is up to date")
                            .size(19.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    if ui.button("Close").clicked() {
                        action = Some(UpdateAction::Later);
                    }
                }
                UpdateState::Idle => {
                    ui.label("No update is currently selected.");
                    if ui.button("Close").clicked() {
                        action = Some(UpdateAction::Later);
                    }
                }
            });

        match action {
            Some(UpdateAction::Download(offer)) => self.start_update_download(offer),
            Some(UpdateAction::Apply) => self.apply_verified_update(ctx),
            Some(UpdateAction::CancelDownload) => {
                self.updates.cancel_download();
                self.status = "Cancelling update download...".into();
            }
            Some(UpdateAction::Later) => {
                self.updates.dismiss_offer();
                open = false;
            }
            Some(UpdateAction::Retry) => {
                open = false;
                self.start_update_check(true);
            }
            Some(UpdateAction::OpenRelease(url)) => Self::open_url(&url),
            None => {}
        }
        self.show_update_dialog = open;
    }

    fn render_batch_rename_dialog(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let paths = self.batch_rename_paths.clone();
        let mut rule = self.batch_rename_rule.clone();
        let mut open = self.show_batch_rename;
        let mut apply = false;
        let mut close = false;

        egui::Window::new("Batch Rename")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(fit_window_size(ctx, 720.0, 560.0))
            .min_width(620.0)
            .show(ctx, |ui| {
                settings_heading(ui, "Rename Rules", &palette);
                egui::Grid::new("batch_rename_rules")
                    .num_columns(2)
                    .spacing(egui::vec2(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.label("Find");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.find)
                                .desired_width(ui.available_width())
                                .hint_text("Text to replace"),
                        );
                        ui.end_row();

                        ui.label("Replace with");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.replace)
                                .desired_width(ui.available_width()),
                        );
                        ui.end_row();

                        ui.label("Prefix");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.prefix)
                                .desired_width(ui.available_width()),
                        );
                        ui.end_row();

                        ui.label("Suffix");
                        ui.add(
                            egui::TextEdit::singleline(&mut rule.suffix)
                                .desired_width(ui.available_width()),
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut rule.sequence_enabled, "Append sequence");
                    ui.add_enabled_ui(rule.sequence_enabled, |ui| {
                        ui.label("Start");
                        ui.add(egui::DragValue::new(&mut rule.sequence_start).range(0..=999_999));
                        ui.label("Digits");
                        ui.add(egui::DragValue::new(&mut rule.sequence_padding).range(1..=12));
                    });
                });

                let preview = crate::power_tools::preview_batch_rename(&paths, &rule);
                let error_count = preview.iter().filter(|item| item.error.is_some()).count();
                let changed_count = preview.iter().filter(|item| item.changed).count();
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    settings_heading(ui, "Preview", &palette);
                    ui.label(
                        egui::RichText::new(format!(
                            "{changed_count} change(s), {error_count} error(s)"
                        ))
                        .size(11.5)
                        .color(if error_count == 0 {
                            palette.text_secondary
                        } else {
                            palette.error
                        }),
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &preview {
                            let old_name = item
                                .source
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let new_name = item
                                .destination
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(elide_middle(&old_name, 36))
                                        .color(palette.text_secondary),
                                );
                                ui.label(egui::RichText::new("->").color(palette.text_tertiary));
                                ui.label(egui::RichText::new(elide_middle(&new_name, 36)).color(
                                    if item.error.is_some() {
                                        palette.error
                                    } else if item.changed {
                                        palette.text_primary
                                    } else {
                                        palette.text_tertiary
                                    },
                                ));
                            });
                            if let Some(error) = &item.error {
                                ui.label(
                                    egui::RichText::new(error).size(11.0).color(palette.error),
                                );
                            }
                            ui.separator();
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            error_count == 0
                                && changed_count > 0
                                && self.power_tools.active_count() == 0,
                            egui::Button::new(format!("Rename {changed_count} Item(s)")),
                        )
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        self.batch_rename_rule = rule;
        if close {
            open = false;
        }
        if apply {
            let preview = crate::power_tools::preview_batch_rename(
                &self.batch_rename_paths,
                &self.batch_rename_rule,
            );
            match self.power_tools.start_batch_rename(preview) {
                Ok(()) => {
                    open = false;
                    self.show_task_center = true;
                    self.status = "Applying batch rename...".into();
                }
                Err(error) => self.status = error,
            }
        }
        self.show_batch_rename = open;
    }

    fn render_duplicate_finder_dialog(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let groups = self.duplicate_groups.clone();
        let root = self.duplicate_root.clone();
        let all_paths: HashSet<PathBuf> = groups
            .iter()
            .flat_map(|group| group.paths.iter().cloned())
            .collect();
        let mut selected = self.duplicate_selected.clone();
        selected.retain(|path| all_paths.contains(path));
        let mut open = self.show_duplicate_finder;
        let mut reveal = None;
        let mut delete_selected = false;
        let mut rescan = false;
        let mut close = false;

        egui::Window::new("Duplicate Finder")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(fit_window_size(ctx, 760.0, 590.0))
            .min_width(640.0)
            .show(ctx, |ui| {
                let duplicate_files: usize = groups.iter().map(|group| group.paths.len()).sum();
                let wasted_bytes: u64 = groups.iter().map(DuplicateGroup::wasted_bytes).sum();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} duplicate group(s)", groups.len()))
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{duplicate_files} files, {} recoverable",
                            format_size(wasted_bytes)
                        ))
                        .color(palette.text_secondary),
                    );
                });
                if let Some(root) = &root {
                    settings_path_row(ui, "Scope", root, &palette);
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(!groups.is_empty(), egui::Button::new("Select Copies"))
                        .clicked()
                    {
                        selected.clear();
                        for group in &groups {
                            selected.extend(group.paths.iter().skip(1).cloned());
                        }
                    }
                    if ui
                        .add_enabled(!selected.is_empty(), egui::Button::new("Clear Selection"))
                        .clicked()
                    {
                        selected.clear();
                    }
                    if ui
                        .add_enabled(
                            self.power_tools.active_count() == 0,
                            egui::Button::new("Scan Again"),
                        )
                        .clicked()
                    {
                        rescan = true;
                    }
                });
                ui.separator();

                if groups.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("No duplicate file contents were found.")
                                .color(palette.text_secondary),
                        );
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (group_index, group) in groups.iter().enumerate() {
                                let title = format!(
                                    "Group {} - {} each - {} wasted",
                                    group_index + 1,
                                    format_size(group.size_bytes),
                                    format_size(group.wasted_bytes())
                                );
                                egui::CollapsingHeader::new(title)
                                    .id_salt(("duplicate_group", group_index))
                                    .default_open(group_index < 3)
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "SHA-256 {}...",
                                                &group.sha256[..12.min(group.sha256.len())]
                                            ))
                                            .monospace()
                                            .size(10.5)
                                            .color(palette.text_tertiary),
                                        );
                                        for path in &group.paths {
                                            ui.horizontal(|ui| {
                                                let mut checked = selected.contains(path);
                                                if ui.checkbox(&mut checked, "").changed() {
                                                    if checked {
                                                        selected.insert(path.clone());
                                                    } else {
                                                        selected.remove(path);
                                                    }
                                                }
                                                ui.label(
                                                    egui::RichText::new(elide_middle(
                                                        &path.to_string_lossy(),
                                                        82,
                                                    ))
                                                    .color(palette.text_secondary),
                                                );
                                                if ui.small_button("Reveal").clicked() {
                                                    reveal = Some(path.clone());
                                                }
                                            });
                                        }
                                    });
                                ui.separator();
                            }
                        });
                }

                let removes_entire_group = groups.iter().any(|group| {
                    !group.paths.is_empty()
                        && group.paths.iter().all(|path| selected.contains(path))
                });
                if removes_entire_group {
                    ui.colored_label(
                        palette.error,
                        "Keep at least one file in every duplicate group.",
                    );
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !selected.is_empty() && !removes_entire_group,
                            egui::Button::new(format!(
                                "Move {} Selected to Recycle Bin",
                                selected.len()
                            )),
                        )
                        .clicked()
                    {
                        delete_selected = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        self.duplicate_selected = selected.clone();
        if close {
            open = false;
        }
        if let Some(path) = reveal {
            Self::reveal_in_explorer(&path);
        }
        if delete_selected {
            self.pending_delete = Some(PendingDelete {
                paths: selected.into_iter().collect(),
                mode: DeleteMode::Recycle,
            });
            open = false;
        }
        if rescan {
            if let Some(root) = root {
                match self.power_tools.start_duplicate_scan(root) {
                    Ok(()) => {
                        self.duplicate_groups.clear();
                        self.duplicate_selected.clear();
                        self.show_task_center = true;
                        open = false;
                    }
                    Err(error) => self.status = error,
                }
            }
        }
        self.show_duplicate_finder = open;
    }

    fn render_folder_sizes_dialog(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let entries = self.folder_size_entries.clone();
        let root = self.folder_size_root.clone();
        let mut open = self.show_folder_sizes;
        let mut open_folder = None;
        let mut recalculate = false;
        let mut close = false;

        egui::Window::new("Folder Sizes")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(fit_window_size(ctx, 680.0, 540.0))
            .min_width(560.0)
            .show(ctx, |ui| {
                if let Some(root) = &root {
                    settings_path_row(ui, "Folder", root, &palette);
                }
                let total_bytes: u64 = entries.iter().map(|entry| entry.size_bytes).sum();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} child folder(s)", entries.len()))
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} total", format_size(total_bytes)))
                            .color(palette.text_secondary),
                    );
                });
                ui.add_space(8.0);
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("folder_size_results")
                            .num_columns(4)
                            .striped(true)
                            .spacing(egui::vec2(14.0, 8.0))
                            .show(ui, |ui| {
                                ui.strong("Folder");
                                ui.strong("Files");
                                ui.strong("Size");
                                ui.label("");
                                ui.end_row();
                                for entry in &entries {
                                    ui.label(
                                        egui::RichText::new(
                                            entry
                                                .path
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy(),
                                        )
                                        .color(palette.text_primary),
                                    );
                                    ui.label(entry.file_count.to_string());
                                    ui.label(format_size(entry.size_bytes));
                                    if ui.small_button("Open").clicked() {
                                        open_folder = Some(entry.path.clone());
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.power_tools.active_count() == 0,
                            egui::Button::new("Recalculate"),
                        )
                        .clicked()
                    {
                        recalculate = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if close {
            open = false;
        }
        if let Some(path) = open_folder {
            self.navigate_to(path);
            open = false;
        }
        if recalculate {
            if let Some(root) = root {
                match self.power_tools.start_folder_sizes(root) {
                    Ok(()) => {
                        self.folder_size_entries.clear();
                        self.show_task_center = true;
                        open = false;
                    }
                    Err(error) => self.status = error,
                }
            }
        }
        self.show_folder_sizes = open;
    }

    fn render_initial_state(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        let content_height = 96.0 + 22.0 + 21.0 + 6.0 + 18.0 + 18.0 + 30.0;
        ui.add_space(((ui.available_height() - content_height) * 0.45).max(0.0));
        ui.vertical(|ui| {
            ui.vertical_centered(|ui| {
                let art = ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::hover()).0;
                paint_brand_icon(ui, &mut self.brand_icon, art);
                ui.add_space(22.0);
                ui.label(egui::RichText::new("Pick a folder to start browsing").size(16.0).strong().color(palette.text_primary));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("LuxePix scans local folders and shows images in a clean contact-sheet layout.").size(13.0).color(palette.text_secondary));
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, IconKind::FolderOpen, "Browse for folder", &palette).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.navigate_to(p);
                        }
                    }
                    if icon_button(ui, IconKind::History, Some("Recent paths"), "Recent paths", self.show_history, &palette).clicked() {
                        self.show_history = !self.show_history;
                    }
                });
            });
        });
    }

    fn render_select_folder_state(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        let content_height = 76.0 + 16.0 + 20.0 + 6.0 + 18.0;
        ui.add_space(((ui.available_height() - content_height) * 0.45).max(0.0));
        ui.vertical(|ui| {
            ui.vertical_centered(|ui| {
                let art = ui
                    .allocate_exact_size(egui::vec2(76.0, 76.0), egui::Sense::hover())
                    .0;
                ui.painter().rect_filled(art, 16.0, palette.surface_inset);
                ui.painter()
                    .rect_stroke(art, 16.0, egui::Stroke::new(1.0, palette.border_soft));
                draw_icon(
                    ui.painter(),
                    IconKind::Image,
                    art.shrink(22.0),
                    palette.text_tertiary,
                    1.7,
                );
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Select a folder to view its files")
                        .size(15.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Use the folder tray on the left, or arrow keys and Enter.",
                    )
                    .size(12.5)
                    .color(palette.text_secondary),
                );
            });
        });
    }

    fn render_file_action_bar(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        if (self.active_pane != ExplorerPane::Files || self.file_selection.is_empty())
            && self.clipboard.is_none()
        {
            return;
        }

        egui::Frame::none()
            .fill(palette.canvas)
            .inner_margin(egui::Margin::symmetric(18.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let selected_label = match self.file_selection.len() {
                        0 => "Clipboard ready".into(),
                        1 => self
                            .selected_file_entry()
                            .map(|file| truncate_str(&file.name, 34))
                            .unwrap_or_else(|| "1 item selected".into()),
                        count => format!("{count} items selected"),
                    };
                    ui.label(
                        egui::RichText::new(selected_label)
                            .size(12.0)
                            .color(palette.text_secondary),
                    );
                    ui.add_space(8.0);
                    for command in [
                        CommandId::Open,
                        CommandId::Copy,
                        CommandId::Cut,
                        CommandId::Paste,
                        CommandId::Rename,
                        CommandId::Delete,
                    ] {
                        if ui
                            .add_enabled(
                                self.command_enabled(command),
                                egui::Button::new(command.short_label()),
                            )
                            .on_hover_text(format!(
                                "{} ({})",
                                command.label(),
                                self.config.shortcuts.get(command)
                            ))
                            .clicked()
                        {
                            self.execute_command(command);
                        }
                    }
                    ui.menu_button("More", |ui| {
                        for command in [
                            CommandId::OpenWith,
                            CommandId::RevealInExplorer,
                            CommandId::DeletePermanent,
                        ] {
                            if ui
                                .add_enabled(
                                    self.command_enabled(command),
                                    egui::Button::new(command.short_label()),
                                )
                                .clicked()
                            {
                                self.execute_command(command);
                                ui.close_menu();
                            }
                        }
                    });
                });
            });
    }

    fn render_left_pane(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        let total_images: usize = self.folders.iter().map(|f| f.image_count).sum();

        render_pane_header(
            ui,
            &palette,
            "Folders",
            &format!("{} folders  -  {} images", self.folders.len(), total_images),
            &mut self.left_thumb_size,
            80.0..=400.0,
        );

        if self.folders.is_empty() {
            render_small_empty(
                ui,
                &palette,
                IconKind::Folder,
                "No subfolders found",
                "Choose another root folder or browse into this path.",
            );
            return;
        }

        let thumb_size = self.left_thumb_size;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut context_command: Option<CommandId> = None;
        let renaming_path = self.renaming.as_ref().map(|r| r.original_path.clone());
        let folders = self.folders.clone();
        let mut item_rects = Vec::with_capacity(folders.len());
        let mut open_folder = None;
        let pending_scroll = self.pending_folder_scroll_to.clone();
        let tile_width = thumb_size.clamp(116.0, 360.0);
        self.folder_columns = ((ui.available_width() + 12.0) / (tile_width + 12.0))
            .floor()
            .max(1.0) as usize;

        let output = egui::ScrollArea::vertical()
            .id_salt("folder_gallery")
            .vertical_scroll_offset(self.left_scroll_offset)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);

                    for folder in &folders {
                        let tile_w = tile_width;
                        let thumb_h = (tile_w * 0.75).max(86.0);
                        let tile_h = thumb_h + 70.0;
                        let (rect, tile_resp) = ui.allocate_exact_size(
                            egui::vec2(tile_w, tile_h),
                            egui::Sense::click_and_drag(),
                        );
                        item_rects.push((folder.id.clone(), rect));
                        if pending_scroll.as_ref() == Some(&folder.id) {
                            tile_resp.scroll_to_me(Some(egui::Align::Center));
                        }

                        if !ui.is_rect_visible(rect) {
                            if let Some(ref p) = folder.first_image {
                                self.thumb_mgr.request_thumbnail(p, thumb_size as u32);
                            }
                            continue;
                        }

                        if let Some(ref p) = folder.first_image {
                            self.thumb_mgr.request_thumbnail(p, thumb_size as u32);
                        }

                        let is_sel = self.folder_selection.contains(&folder.id);
                        paint_tile_shell(ui, rect, is_sel, tile_resp.hovered(), &palette);

                        let t_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(tile_w, thumb_h));
                        let thumb_resp = ui.interact(
                            t_rect,
                            ui.id().with(("lt", &folder.id)),
                            egui::Sense::click_and_drag(),
                        );

                        if let Some(ref p) = folder.first_image {
                            if let Some(tex) = self.thumb_mgr.get(p) {
                                paint_thumbnail(ui, tex, t_rect.shrink(1.0), &palette);
                            } else {
                                paint_placeholder(
                                    ui,
                                    t_rect.shrink(1.0),
                                    IconKind::Loader,
                                    "Loading",
                                    &palette,
                                );
                            }
                        } else {
                            paint_placeholder(
                                ui,
                                t_rect.shrink(1.0),
                                IconKind::Folder,
                                "",
                                &palette,
                            );
                        }

                        let checkbox_clicked = if self.config.show_selection_checkboxes {
                            explorer::selection_checkbox(
                                ui,
                                egui::Rect::from_min_size(
                                    rect.min + egui::vec2(8.0, 8.0),
                                    egui::vec2(19.0, 19.0),
                                ),
                                is_sel,
                                &palette,
                                ("folder_check", &folder.id),
                            )
                            .clicked()
                        } else {
                            false
                        };
                        if thumb_resp.double_clicked() || tile_resp.double_clicked() {
                            self.folder_selection.replace(folder.id.clone());
                            self.selected_folder = Some(folder.id.clone());
                            self.active_pane = ExplorerPane::Folders;
                            open_folder = Some(folder.path.clone());
                        } else if checkbox_clicked {
                            self.select_folder_with_modifiers(
                                folder.id.clone(),
                                egui::Modifiers {
                                    ctrl: true,
                                    ..Default::default()
                                },
                            );
                        } else if thumb_resp.clicked() || tile_resp.clicked() {
                            let modifiers = ui.input(|input| input.modifiers);
                            self.select_folder_with_modifiers(folder.id.clone(), modifiers);
                        }
                        if thumb_resp.drag_started() || tile_resp.drag_started() {
                            self.begin_folder_drag(folder);
                        }
                        if self.internal_drag.is_some()
                            && (thumb_resp.hovered() || tile_resp.hovered())
                        {
                            let modifiers = ui.input(|input| input.modifiers);
                            self.set_internal_drop_target(folder.path.clone(), modifiers);
                        }
                        if self.internal_drop_target_is(&folder.path) {
                            ui.painter().rect_stroke(
                                rect.shrink(2.0),
                                6.0,
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                        if thumb_resp.secondary_clicked() || tile_resp.secondary_clicked() {
                            self.select_folder_for_context(folder.id.clone());
                        }
                        let menu_context = self.command_context();
                        tile_resp.context_menu(|ui| {
                            for command in [
                                CommandId::Open,
                                CommandId::RevealInExplorer,
                                CommandId::Copy,
                                CommandId::Cut,
                                CommandId::Paste,
                                CommandId::Rename,
                                CommandId::Delete,
                            ] {
                                if context_menu_command(
                                    ui,
                                    command,
                                    menu_context,
                                    self.config.shortcuts.get(command),
                                ) {
                                    context_command = Some(command);
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if context_menu_command(
                                ui,
                                CommandId::DeletePermanent,
                                menu_context,
                                self.config.shortcuts.get(CommandId::DeletePermanent),
                            ) {
                                context_command = Some(CommandId::DeletePermanent);
                                ui.close_menu();
                            }
                        });

                        let name_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.min.x + 10.0, rect.min.y + thumb_h + 6.0),
                            egui::vec2(tile_w - 20.0, 34.0),
                        );
                        let is_renaming_this = renaming_path.as_ref() == Some(&folder.path);

                        if is_renaming_this {
                            if let Some(ref mut rename) = self.renaming {
                                let te = ui.put(
                                    name_rect,
                                    egui::TextEdit::singleline(&mut rename.new_name)
                                        .font(egui::FontId::proportional(12.5))
                                        .desired_width(tile_w - 20.0),
                                );
                                te.request_focus();
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter {
                                    commit_rename = true;
                                } else if escape || te.lost_focus() {
                                    cancel_rename = true;
                                }
                            }
                        } else {
                            paint_wrapped_text(
                                ui,
                                name_rect,
                                &folder.name,
                                13.0,
                                palette.text_primary,
                            );
                        }

                        let pill_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.min.x + 10.0, rect.max.y - 24.0),
                            egui::vec2(72.0, 17.0),
                        );
                        paint_count_pill(ui, pill_rect, folder.image_count, &palette, is_sel);

                        ui.painter().text(
                            egui::pos2(pill_rect.max.x + 8.0, pill_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("{} files", folder.total_files),
                            egui::FontId::proportional(11.0),
                            palette.text_tertiary,
                        );
                    }
                });
                ui.add_space(20.0);
            });
        self.left_scroll_offset = output.state.offset.y;
        if self.internal_drag.is_some()
            && self
                .internal_drag
                .as_ref()
                .is_some_and(|drag| drag.target_dir.is_none())
            && ui.input(|input| {
                input
                    .pointer
                    .hover_pos()
                    .is_some_and(|position| output.inner_rect.contains(position))
            })
        {
            if let Some(path) = self.current_path.clone() {
                let modifiers = ui.input(|input| input.modifiers);
                self.set_internal_drop_target(path, modifiers);
            }
        }
        let finished_marquee = explorer::update_marquee_selection(
            ui,
            output.inner_rect,
            &item_rects,
            &mut self.folder_selection,
            &mut self.folder_marquee,
            &palette,
        );
        if self.folder_marquee.is_some() || finished_marquee {
            self.active_pane = ExplorerPane::Folders;
        }
        if finished_marquee {
            if let Some(focus) = self.folder_selection.focus().cloned() {
                if self.selected_folder.as_ref() != Some(&focus) {
                    self.activate_folder(focus);
                }
            } else {
                self.clear_active_folder();
            }
        }
        if pending_scroll
            .as_ref()
            .is_some_and(|id| folders.iter().any(|folder| &folder.id == id))
        {
            self.pending_folder_scroll_to = None;
        }
        if let Some(path) = open_folder {
            self.navigate_to(path);
        }
        if commit_rename {
            self.execute_rename();
        } else if cancel_rename {
            self.renaming = None;
        }
        if let Some(command) = context_command {
            self.execute_command(command);
        }
    }

    fn render_right_pane(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        if self.internal_drag.is_some()
            && ui.input(|input| {
                input
                    .pointer
                    .hover_pos()
                    .is_some_and(|position| ui.max_rect().contains(position))
            })
        {
            if let Some(path) = self.selected_folder_path() {
                let modifiers = ui.input(|input| input.modifiers);
                self.set_internal_drop_target(path, modifiers);
                ui.painter().rect_stroke(
                    ui.max_rect().shrink(2.0),
                    4.0,
                    egui::Stroke::new(2.0, palette.accent),
                );
            }
        }
        let folder_name = self
            .selected_folder_entry()
            .map(|folder| folder.name.clone())
            .unwrap_or_default();
        let img_count = self.right_image_paths.len();
        let other_count = self.right_files.len().saturating_sub(img_count);
        render_pane_header(
            ui,
            &palette,
            &folder_name,
            &format!("{} images  -  {} other files", img_count, other_count),
            &mut self.right_thumb_size,
            80.0..=500.0,
        );
        self.render_explorer_toolbar(ui, &palette);
        self.render_file_action_bar(ui, &palette);

        if self.right_files.is_empty() {
            render_small_empty(
                ui,
                &palette,
                IconKind::Image,
                "This folder is empty",
                "There are no images or other files here.",
            );
            return;
        }

        if self.config.file_view_mode != ViewMode::Gallery {
            self.render_alternate_file_view(ui, &palette);
            return;
        }

        let thumb_size = self.right_thumb_size;
        let mut open_preview: Option<usize> = None;
        let mut open_external: Option<PathBuf> = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut context_command: Option<CommandId> = None;

        let renaming_path = self.renaming.as_ref().map(|r| r.original_path.clone());
        let files = self.right_files.clone();
        let mut item_rects = Vec::with_capacity(files.len());
        let pending_scroll = self.pending_file_scroll_to.clone();
        let tile_width = thumb_size.clamp(160.0, 420.0);
        self.file_columns = ((ui.available_width() + 14.0) / (tile_width + 14.0))
            .floor()
            .max(1.0) as usize;

        let output = egui::ScrollArea::vertical()
            .id_salt("file_gallery")
            .vertical_scroll_offset(self.right_scroll_offset)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                render_section_label(ui, &palette, "Items", files.len());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);

                    for file in &files {
                        let tile_w = tile_width;
                        let info_h = 62.0;
                        let tile_h = tile_w + info_h;
                        let (rect, tile_resp) = ui.allocate_exact_size(
                            egui::vec2(tile_w, tile_h),
                            egui::Sense::click_and_drag(),
                        );
                        item_rects.push((file.id.clone(), rect));
                        if pending_scroll.as_ref() == Some(&file.id) {
                            tile_resp.scroll_to_me(Some(egui::Align::Center));
                        }

                        if !ui.is_rect_visible(rect) {
                            if file.is_image {
                                self.thumb_mgr
                                    .request_thumbnail(&file.path, thumb_size as u32);
                            }
                            continue;
                        }

                        let is_selected = self.file_selection.contains(&file.id);
                        paint_tile_shell(ui, rect, is_selected, tile_resp.hovered(), &palette);

                        let t_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(tile_w, tile_w));
                        let thumb_resp = ui.interact(
                            t_rect,
                            ui.id().with(("rt", &file.id)),
                            egui::Sense::click_and_drag(),
                        );

                        if file.is_image {
                            self.thumb_mgr
                                .request_thumbnail(&file.path, thumb_size as u32);
                            if let Some(tex) = self.thumb_mgr.get(&file.path) {
                                paint_thumbnail(ui, tex, t_rect.shrink(1.0), &palette);
                            } else {
                                paint_placeholder(
                                    ui,
                                    t_rect.shrink(1.0),
                                    IconKind::Loader,
                                    "Loading",
                                    &palette,
                                );
                            }

                            if thumb_resp.double_clicked() || tile_resp.double_clicked() {
                                self.file_selection.replace(file.id.clone());
                                self.selected_file = Some(file.id.clone());
                                self.active_pane = ExplorerPane::Files;
                                if let Some(img_idx) =
                                    self.right_image_paths.iter().position(|p| p == &file.path)
                                {
                                    open_preview = Some(img_idx);
                                }
                            }
                        } else {
                            paint_file_placeholder(ui, t_rect.shrink(1.0), file, &palette);
                            if thumb_resp.double_clicked() || tile_resp.double_clicked() {
                                self.file_selection.replace(file.id.clone());
                                self.selected_file = Some(file.id.clone());
                                self.active_pane = ExplorerPane::Files;
                                open_external = Some(file.path.clone());
                            }
                        }

                        let checkbox_clicked = if self.config.show_selection_checkboxes {
                            explorer::selection_checkbox(
                                ui,
                                egui::Rect::from_min_size(
                                    rect.min + egui::vec2(8.0, 8.0),
                                    egui::vec2(19.0, 19.0),
                                ),
                                is_selected,
                                &palette,
                                ("gallery_check", &file.id),
                            )
                            .clicked()
                        } else {
                            false
                        };
                        if checkbox_clicked {
                            self.select_file_with_modifiers(
                                file.id.clone(),
                                egui::Modifiers {
                                    ctrl: true,
                                    ..Default::default()
                                },
                            );
                        } else if thumb_resp.clicked() || tile_resp.clicked() {
                            let modifiers = ui.input(|input| input.modifiers);
                            self.select_file_with_modifiers(file.id.clone(), modifiers);
                        }
                        if thumb_resp.drag_started() || tile_resp.drag_started() {
                            self.begin_file_drag(file);
                        }
                        if tile_resp.secondary_clicked() || thumb_resp.secondary_clicked() {
                            self.select_file_for_context(file.id.clone());
                        }
                        let menu_context = self.command_context();
                        tile_resp.context_menu(|ui| {
                            for command in [
                                CommandId::Open,
                                CommandId::OpenWith,
                                CommandId::RevealInExplorer,
                                CommandId::Copy,
                                CommandId::Cut,
                                CommandId::Paste,
                                CommandId::Rename,
                                CommandId::Delete,
                            ] {
                                if context_menu_command(
                                    ui,
                                    command,
                                    menu_context,
                                    self.config.shortcuts.get(command),
                                ) {
                                    context_command = Some(command);
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if context_menu_command(
                                ui,
                                CommandId::DeletePermanent,
                                menu_context,
                                self.config.shortcuts.get(CommandId::DeletePermanent),
                            ) {
                                context_command = Some(CommandId::DeletePermanent);
                                ui.close_menu();
                            }
                        });

                        let name_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.min.x + 10.0, rect.min.y + tile_w + 6.0),
                            egui::vec2(tile_w - 20.0, 32.0),
                        );
                        let is_renaming_this = renaming_path.as_ref() == Some(&file.path);

                        if is_renaming_this {
                            if let Some(ref mut rename) = self.renaming {
                                let te = ui.put(
                                    name_rect,
                                    egui::TextEdit::singleline(&mut rename.new_name)
                                        .font(egui::FontId::proportional(12.5))
                                        .desired_width(tile_w - 20.0),
                                );
                                te.request_focus();
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter {
                                    commit_rename = true;
                                } else if escape || te.lost_focus() {
                                    cancel_rename = true;
                                }
                            }
                        } else {
                            paint_wrapped_text(
                                ui,
                                name_rect,
                                &file.name,
                                12.5,
                                palette.text_primary,
                            );
                        }

                        let meta = if file.is_image {
                            format_size(file.size_bytes)
                        } else {
                            format!(
                                "{} - {}",
                                file.extension.to_uppercase(),
                                format_size(file.size_bytes)
                            )
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 10.0, rect.max.y - 12.0),
                            egui::Align2::LEFT_CENTER,
                            meta,
                            egui::FontId::proportional(11.0),
                            palette.text_tertiary,
                        );

                        if thumb_resp.hovered() {
                            thumb_resp.on_hover_text(format!(
                                "{}\n{}\nClick name to rename",
                                file.name,
                                format_size(file.size_bytes)
                            ));
                        }
                    }
                });
                ui.add_space(20.0);
            });
        self.right_scroll_offset = output.state.offset.y;
        let finished_marquee = explorer::update_marquee_selection(
            ui,
            output.inner_rect,
            &item_rects,
            &mut self.file_selection,
            &mut self.file_marquee,
            &palette,
        );
        if self.file_marquee.is_some() || finished_marquee {
            self.active_pane = ExplorerPane::Files;
            self.selected_file = self.file_selection.focus().cloned();
        }
        if pending_scroll
            .as_ref()
            .is_some_and(|id| files.iter().any(|file| &file.id == id))
        {
            self.pending_file_scroll_to = None;
        }

        // Open preview for clicked image
        if let Some(img_idx) = open_preview {
            if img_idx < self.right_image_paths.len() {
                let p = self.right_image_paths[img_idx].clone();
                self.full_mgr.request_full(&p);
                self.preview = Some(PreviewState {
                    path: p,
                    index: img_idx,
                    zoom: 1.0,
                    offset: egui::Vec2::ZERO,
                });
            }
        }

        // Open non-image file externally
        if let Some(path) = open_external {
            Self::open_file_external(&path);
        }

        if commit_rename {
            self.execute_rename();
        } else if cancel_rename {
            self.renaming = None;
        }
        if let Some(command) = context_command {
            self.execute_command(command);
        }
    }

    fn render_preview(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut nav: Option<i32> = None;

        // Keyboard input
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                close = true;
            }
            if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowDown) {
                nav = Some(1);
            }
            if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowUp) {
                nav = Some(-1);
            }
        });

        if close {
            self.preview = None;
            return;
        }

        // Navigation via keyboard
        if let Some(dir) = nav {
            if let Some(ref mut pv) = self.preview {
                let new_idx = pv.index as i32 + dir;
                if new_idx >= 0 && (new_idx as usize) < self.right_image_paths.len() {
                    pv.index = new_idx as usize;
                    pv.path = self.right_image_paths[pv.index].clone();
                    pv.zoom = 1.0;
                    pv.offset = egui::Vec2::ZERO;
                    self.full_mgr.request_full(&pv.path);
                }
            }
        }

        // Collect state we need from preview (to avoid borrow issues)
        let pv = self.preview.as_ref().unwrap();
        let pv_path = pv.path.clone();
        let pv_index = pv.index;
        let pv_zoom = pv.zoom;
        let pv_offset = pv.offset;
        let num_images = self.right_image_paths.len();

        // Request full image load
        self.full_mgr.request_full(&pv_path);

        // Draw overlay — collect actions to apply after
        let mut action_close = false;
        let mut action_zoom: Option<f32> = None;
        let mut action_drag_delta: Option<egui::Vec2> = None;
        let mut action_reset = false;
        let mut action_nav_btn: Option<i32> = None;

        let screen = ctx.screen_rect();

        egui::Area::new(egui::Id::new("preview_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());

                ui.painter().rect_filled(
                    rect,
                    0.0,
                    egui::Color32::from_rgba_premultiplied(8, 10, 12, 245),
                );

                // Zoom with scroll
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    let factor = if scroll > 0.0 { 1.15 } else { 1.0 / 1.15 };
                    action_zoom = Some((pv_zoom * factor).clamp(0.1, 20.0));
                }

                // Pan with drag
                if resp.dragged() {
                    action_drag_delta = Some(resp.drag_delta());
                }

                // Double-click to reset
                if resp.double_clicked() {
                    action_reset = true;
                }

                // Render image
                let current_zoom = action_zoom.unwrap_or(pv_zoom);
                let current_offset = pv_offset + action_drag_delta.unwrap_or(egui::Vec2::ZERO);

                if let Some(tex) = self.full_mgr.get(&pv_path) {
                    let tex_size = tex.size_vec2();
                    let margin = 128.0;
                    let avail = egui::vec2(rect.width() - margin, rect.height() - margin - 80.0);
                    let fit_scale = (avail.x / tex_size.x).min(avail.y / tex_size.y).min(1.0);
                    let display_size = tex_size * fit_scale * current_zoom;

                    let center = rect.center() + current_offset;
                    let img_rect = egui::Rect::from_center_size(center, display_size);

                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    mesh.add_rect_with_uv(
                        img_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    ui.painter().add(egui::Shape::mesh(mesh));
                } else {
                    let load_rect = egui::Rect::from_center_size(
                        rect.center() - egui::vec2(0.0, 18.0),
                        egui::vec2(34.0, 34.0),
                    );
                    draw_icon(
                        ui.painter(),
                        IconKind::Loader,
                        load_rect,
                        egui::Color32::from_gray(190),
                        2.0,
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 24.0),
                        egui::Align2::CENTER_CENTER,
                        format!(
                            "Loading {}...",
                            pv_path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_gray(155),
                    );
                }

                let fname = pv_path.file_name().unwrap_or_default().to_string_lossy();
                let meta = if let Some(orig) = self.full_mgr.get_original_size(&pv_path) {
                    format!("{} x {}", orig[0], orig[1])
                } else {
                    "Loading".into()
                };

                let info_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.bottom() - 52.0),
                    rect.right_bottom(),
                );
                ui.painter().rect_filled(
                    info_rect,
                    0.0,
                    egui::Color32::from_rgba_premultiplied(20, 24, 28, 235),
                );
                ui.painter().line_segment(
                    [info_rect.left_top(), info_rect.right_top()],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 22),
                    ),
                );
                ui.painter().text(
                    info_rect.left_center() + egui::vec2(24.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    fname,
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                );
                ui.painter().text(
                    info_rect.left_center() + egui::vec2(260.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    meta,
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_gray(165),
                );
                let zoom_badge = egui::Rect::from_center_size(
                    info_rect.right_center() - egui::vec2(130.0, 0.0),
                    egui::vec2(82.0, 28.0),
                );
                ui.painter().rect_filled(
                    zoom_badge,
                    6.0,
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, 16),
                );
                ui.painter().rect_stroke(
                    zoom_badge,
                    6.0,
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 26),
                    ),
                );
                ui.painter().text(
                    zoom_badge.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{:.0}%", current_zoom * 100.0),
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_gray(210),
                );
                ui.painter().text(
                    info_rect.right_center() - egui::vec2(24.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{} / {}", pv_index + 1, num_images),
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                );

                let close_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.max.x - 54.0, rect.min.y + 18.0),
                    egui::vec2(36.0, 36.0),
                );
                let close_resp = ui.allocate_rect(close_rect, egui::Sense::click());
                paint_lightbox_button(
                    ui,
                    close_rect,
                    close_resp.hovered(),
                    IconKind::Close,
                    egui::Color32::from_gray(220),
                );
                if close_resp.clicked() {
                    action_close = true;
                }

                let nav_y = rect.center().y;
                let prev_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.min.x + 52.0, nav_y),
                    egui::vec2(56.0, 56.0),
                );
                let prev_resp = ui.allocate_rect(prev_rect, egui::Sense::click());
                paint_lightbox_button(
                    ui,
                    prev_rect,
                    prev_resp.hovered(),
                    IconKind::ChevronLeft,
                    if pv_index > 0 {
                        egui::Color32::from_gray(220)
                    } else {
                        egui::Color32::from_gray(80)
                    },
                );
                if pv_index > 0 && prev_resp.clicked() {
                    action_nav_btn = Some(-1);
                }

                let next_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.max.x - 52.0, nav_y),
                    egui::vec2(56.0, 56.0),
                );
                let next_resp = ui.allocate_rect(next_rect, egui::Sense::click());
                paint_lightbox_button(
                    ui,
                    next_rect,
                    next_resp.hovered(),
                    IconKind::ChevronRight,
                    if pv_index + 1 < num_images {
                        egui::Color32::from_gray(220)
                    } else {
                        egui::Color32::from_gray(80)
                    },
                );
                if pv_index + 1 < num_images && next_resp.clicked() {
                    action_nav_btn = Some(1);
                }
            });

        // Apply deferred actions (no borrow conflict now)
        if action_close {
            self.preview = None;
            return;
        }

        if let Some(pv) = self.preview.as_mut() {
            if let Some(z) = action_zoom {
                pv.zoom = z;
            }
            if let Some(delta) = action_drag_delta {
                pv.offset += delta;
            }
            if action_reset {
                pv.zoom = 1.0;
                pv.offset = egui::Vec2::ZERO;
            }
            if let Some(dir) = action_nav_btn {
                let new_idx = pv.index as i32 + dir;
                if new_idx >= 0 && (new_idx as usize) < self.right_image_paths.len() {
                    pv.index = new_idx as usize;
                    pv.path = self.right_image_paths[pv.index].clone();
                    pv.zoom = 1.0;
                    pv.offset = egui::Vec2::ZERO;
                    self.full_mgr.request_full(&pv.path);
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Palette {
    canvas: egui::Color32,
    surface: egui::Color32,
    surface_inset: egui::Color32,
    tile_bg: egui::Color32,
    thumb_bg: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    text_tertiary: egui::Color32,
    border_soft: egui::Color32,
    border_standard: egui::Color32,
    accent: egui::Color32,
    accent_soft: egui::Color32,
    accent_hover: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    error: egui::Color32,
}

impl Palette {
    fn for_theme(theme: &str) -> Self {
        if theme == "dark" {
            Self {
                canvas: rgb(0x15, 0x17, 0x19),
                surface: rgb(0x1C, 0x20, 0x24),
                surface_inset: rgb(0x11, 0x13, 0x15),
                tile_bg: rgb(0x24, 0x2A, 0x2F),
                thumb_bg: rgb(0x2A, 0x30, 0x36),
                text_primary: rgb(0xEE, 0xF2, 0xF5),
                text_secondary: rgb(0xB6, 0xC0, 0xCA),
                text_tertiary: rgb(0x7F, 0x8A, 0x95),
                border_soft: rgba(238, 242, 245, 20),
                border_standard: rgba(238, 242, 245, 36),
                accent: rgb(0x6D, 0xB7, 0xB1),
                accent_soft: rgba(109, 183, 177, 40),
                accent_hover: rgb(0x83, 0xC8, 0xC2),
                success: rgb(0x68, 0xB9, 0x83),
                warning: rgb(0xD8, 0xA8, 0x4E),
                error: rgb(0xE1, 0x6A, 0x63),
            }
        } else {
            Self {
                canvas: rgb(0xF7, 0xF8, 0xFA),
                surface: rgb(0xFF, 0xFF, 0xFF),
                surface_inset: rgb(0xEE, 0xF2, 0xF5),
                tile_bg: rgb(0xFF, 0xFF, 0xFF),
                thumb_bg: rgb(0xE9, 0xEE, 0xF1),
                text_primary: rgb(0x1F, 0x25, 0x2B),
                text_secondary: rgb(0x5B, 0x64, 0x70),
                text_tertiary: rgb(0x87, 0x91, 0x9C),
                border_soft: rgba(31, 37, 43, 20),
                border_standard: rgba(31, 37, 43, 36),
                accent: rgb(0x2E, 0x7D, 0x7A),
                accent_soft: rgb(0xDD, 0xED, 0xEA),
                accent_hover: rgb(0x25, 0x6B, 0x68),
                success: rgb(0x2F, 0x8F, 0x5B),
                warning: rgb(0xB7, 0x79, 0x1F),
                error: rgb(0xC2, 0x41, 0x3A),
            }
        }
    }
}

#[derive(Clone, Copy)]
enum IconKind {
    Folder,
    FolderOpen,
    Image,
    Refresh,
    History,
    Settings,
    Sun,
    Moon,
    Close,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    Loader,
    Doc,
    Video,
    File,
    Archive,
    Info,
    Edit,
    Search,
}

fn fit_window_size(ctx: &egui::Context, width: f32, height: f32) -> egui::Vec2 {
    let screen = ctx.screen_rect().size();
    egui::vec2(
        width.min((screen.x - 48.0).max(420.0)),
        height.min((screen.y - 120.0).max(320.0)),
    )
}

/// Paint text wrapped to the rect width, clipped to the rect height. Uses the
/// painter only — `ui.put` inside a `horizontal_wrapped` layout would advance
/// the row cursor and skew the tile grid.
fn paint_wrapped_text(
    ui: &egui::Ui,
    rect: egui::Rect,
    text: &str,
    font_size: f32,
    color: egui::Color32,
) {
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(font_size),
        color,
        rect.width(),
    );
    ui.painter()
        .with_clip_rect(rect)
        .galley(rect.left_top(), galley, color);
}

fn paint_brand_icon(
    ui: &egui::Ui,
    slot: &mut Option<egui::TextureHandle>,
    rect: egui::Rect,
) {
    let texture = slot.get_or_insert_with(|| {
        let icon_image = image::load_from_memory(include_bytes!("../assets/icon.png"))
            .expect("LuxePix brand icon must be a readable PNG")
            .to_rgba8();
        let (width, height) = icon_image.dimensions();
        ui.ctx().load_texture(
            "luxepix-brand",
            egui::ColorImage::from_rgba_unmultiplied(
                [width as usize, height as usize],
                icon_image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        )
    });
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

fn resolve_theme(ctx: &egui::Context, preference: &str) -> &'static str {
    match preference {
        "light" => "light",
        "dark" => "dark",
        _ => match ctx.system_theme() {
            Some(egui::Theme::Dark) => "dark",
            _ => "light",
        },
    }
}

fn apply_theme(ctx: &egui::Context, preference: &str) -> &'static str {
    let theme = resolve_theme(ctx, preference);
    let palette = Palette::for_theme(theme);
    let mut visuals = if theme == "dark" {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.surface;
    visuals.window_fill = palette.surface;
    visuals.extreme_bg_color = palette.surface_inset;
    visuals.faint_bg_color = palette.surface_inset;
    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.inactive.bg_fill = palette.surface_inset;
    visuals.widgets.hovered.bg_fill = palette.surface_inset;
    visuals.widgets.active.bg_fill = palette.accent_soft;
    visuals.widgets.inactive.fg_stroke.color = palette.text_secondary;
    visuals.widgets.hovered.fg_stroke.color = palette.text_primary;
    visuals.selection.bg_fill = palette.accent_soft;
    visuals.selection.stroke.color = palette.accent;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    style.spacing.menu_margin = egui::Margin::same(8.0);
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
    theme
}

fn status_color(status: &str, palette: &Palette) -> egui::Color32 {
    let lower = status.to_lowercase();
    if lower.contains("fail")
        || lower.contains("not found")
        || lower.contains("error")
        || lower.contains("invalid")
    {
        palette.error
    } else if lower.contains("renamed") || lower.contains("switched") {
        palette.success
    } else if lower.contains("warning") {
        palette.warning
    } else {
        palette.text_tertiary
    }
}

fn operation_state_color(state: OperationState, palette: &Palette) -> egui::Color32 {
    match state {
        OperationState::Completed => palette.success,
        OperationState::CompletedWithErrors | OperationState::Failed => palette.error,
        OperationState::Cancelled | OperationState::Paused => palette.warning,
        OperationState::Running => palette.accent,
    }
}

fn operation_metrics(record: &OperationRecord) -> String {
    let mut parts = vec![format!(
        "{} / {} item(s)",
        record.completed_items, record.total_items
    )];
    if record.total_bytes > 0 {
        parts.push(format!(
            "{} / {}",
            format_size(record.processed_bytes.min(record.total_bytes)),
            format_size(record.total_bytes)
        ));
    }
    if let Some(speed) = record.speed_bytes_per_second() {
        parts.push(format!("{}/s", format_size(speed.max(0.0) as u64)));
    }
    if let Some(remaining) = record.estimated_remaining() {
        parts.push(format!("{} remaining", format_duration_short(remaining)));
    }
    parts.join("  -  ")
}

fn format_duration_short(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 60 * 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: IconKind,
    label: Option<&str>,
    tooltip: &str,
    active: bool,
    palette: &Palette,
) -> egui::Response {
    let label_w = label
        .map(|text| text.chars().count() as f32 * 7.0 + 12.0)
        .unwrap_or(0.0);
    let width = if label.is_some() {
        34.0 + label_w
    } else {
        34.0
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click());
    let fill = if active {
        palette.accent_soft
    } else if response.hovered() {
        palette.surface_inset
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if active {
        ui.painter()
            .rect_stroke(rect, 6.0, egui::Stroke::new(1.0, palette.border_soft));
    }
    let color = if active {
        palette.accent
    } else if response.hovered() {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let icon_rect = if label.is_some() {
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 17.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        )
    } else {
        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0))
    };
    draw_icon(ui.painter(), icon, icon_rect, color, 1.6);
    if let Some(text) = label {
        ui.painter().text(
            egui::pos2(rect.left() + 31.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            color,
        );
    }
    response.on_hover_text(tooltip)
}

fn primary_button(
    ui: &mut egui::Ui,
    icon: IconKind,
    label: &str,
    palette: &Palette,
) -> egui::Response {
    let width = 44.0 + label.chars().count() as f32 * 7.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click());
    let fill = if response.hovered() {
        palette.accent_hover
    } else {
        palette.accent
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 17.0, rect.center().y),
        egui::vec2(15.0, 15.0),
    );
    draw_icon(ui.painter(), icon, icon_rect, egui::Color32::WHITE, 1.6);
    ui.painter().text(
        egui::pos2(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        egui::Color32::WHITE,
    );
    response
}

fn render_pane_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    meta: &str,
    thumb_size: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) {
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 64.0), egui::Sense::hover())
        .0;
    ui.painter().rect_filled(rect, 0.0, palette.surface);
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.bottom()),
        ],
        egui::Stroke::new(1.0, palette.border_soft),
    );
    ui.painter().text(
        rect.min + egui::vec2(18.0, 20.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.5),
        palette.text_primary,
    );
    ui.painter().text(
        rect.min + egui::vec2(18.0, 39.0),
        egui::Align2::LEFT_CENTER,
        meta,
        egui::FontId::proportional(12.0),
        palette.text_tertiary,
    );

    let slider_rect = egui::Rect::from_min_size(
        egui::pos2(
            (rect.right() - 170.0).max(rect.left() + 160.0),
            rect.center().y - 12.0,
        ),
        egui::vec2(150.0, 24.0),
    );
    if slider_rect.right() <= rect.right() - 12.0 {
        let small = egui::Rect::from_center_size(
            slider_rect.left_center() + egui::vec2(8.0, 0.0),
            egui::vec2(12.0, 12.0),
        );
        let large = egui::Rect::from_center_size(
            slider_rect.right_center() - egui::vec2(8.0, 0.0),
            egui::vec2(16.0, 16.0),
        );
        draw_icon(
            ui.painter(),
            IconKind::Image,
            small,
            palette.text_tertiary,
            1.4,
        );
        draw_icon(
            ui.painter(),
            IconKind::Image,
            large,
            palette.text_tertiary,
            1.4,
        );
        let control_rect = slider_rect.shrink2(egui::vec2(24.0, 0.0));
        ui.put(
            control_rect,
            egui::Slider::new(thumb_size, range).show_value(false),
        );
    }
}

fn render_section_label(ui: &mut egui::Ui, palette: &Palette, label: &str, count: usize) {
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover())
        .0;
    ui.painter().text(
        rect.left_center() + egui::vec2(24.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{}  -  {}", label.to_uppercase(), count),
        egui::FontId::proportional(11.0),
        palette.text_tertiary,
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 130.0, rect.center().y),
            egui::pos2(rect.right() - 24.0, rect.center().y),
        ],
        egui::Stroke::new(1.0, palette.border_soft),
    );
}

fn render_small_empty(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: IconKind,
    title: &str,
    subtitle: &str,
) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            let art = ui
                .allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover())
                .0;
            ui.painter().rect_filled(art, 14.0, palette.surface_inset);
            ui.painter()
                .rect_stroke(art, 14.0, egui::Stroke::new(1.0, palette.border_soft));
            draw_icon(
                ui.painter(),
                icon,
                art.shrink(22.0),
                palette.text_tertiary,
                1.7,
            );
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(title)
                    .size(14.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(subtitle)
                    .size(12.5)
                    .color(palette.text_secondary),
            );
        });
    });
}

fn paint_tile_shell(
    ui: &egui::Ui,
    rect: egui::Rect,
    selected: bool,
    hovered: bool,
    palette: &Palette,
) {
    let fill = if selected {
        palette.accent_soft
    } else {
        palette.tile_bg
    };
    let stroke_color = if selected {
        palette.accent
    } else if hovered {
        palette.border_standard
    } else {
        palette.border_soft
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, stroke_color),
    );
}

fn paint_thumbnail(ui: &egui::Ui, tex: &egui::TextureHandle, rect: egui::Rect, palette: &Palette) {
    ui.painter().rect_filled(rect, 7.0, palette.thumb_bg);
    let tex_size = tex.size_vec2();
    let scale = (rect.width() / tex_size.x).min(rect.height() / tex_size.y);
    let display = egui::vec2(tex_size.x * scale, tex_size.y * scale);
    let offset = (rect.size() - display) / 2.0;
    let img_rect = egui::Rect::from_min_size(rect.min + offset, display);

    let mut mesh = egui::Mesh::with_texture(tex.id());
    mesh.add_rect_with_uv(
        img_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn paint_placeholder(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: IconKind,
    label: &str,
    palette: &Palette,
) {
    ui.painter().rect_filled(rect, 7.0, palette.thumb_bg);
    let icon_rect = egui::Rect::from_center_size(
        rect.center() - egui::vec2(0.0, if label.is_empty() { 0.0 } else { 8.0 }),
        egui::vec2(28.0, 28.0),
    );
    draw_icon(ui.painter(), icon, icon_rect, palette.text_tertiary, 1.7);
    if !label.is_empty() {
        ui.painter().text(
            rect.center() + egui::vec2(0.0, 20.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            palette.text_tertiary,
        );
    }
}

fn paint_file_placeholder(ui: &egui::Ui, rect: egui::Rect, file: &FileEntry, palette: &Palette) {
    ui.painter().rect_filled(rect, 7.0, palette.surface_inset);
    let icon = match file.extension.as_str() {
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "webm" => IconKind::Video,
        "zip" | "rar" | "7z" | "tar" | "gz" => IconKind::Archive,
        "pdf" | "doc" | "docx" | "rtf" | "odt" | "txt" | "log" | "md" => IconKind::Doc,
        _ => IconKind::File,
    };
    draw_icon(
        ui.painter(),
        icon,
        egui::Rect::from_center_size(
            rect.center() - egui::vec2(0.0, 14.0),
            egui::vec2(32.0, 32.0),
        ),
        palette.text_tertiary,
        1.7,
    );
    let ext = if file.extension.is_empty() {
        "FILE".into()
    } else {
        file.extension.to_uppercase()
    };
    let badge = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, 24.0),
        egui::vec2((ext.len() as f32 * 7.0 + 18.0).max(44.0), 22.0),
    );
    ui.painter().rect_filled(badge, 4.0, palette.surface);
    ui.painter()
        .rect_stroke(badge, 4.0, egui::Stroke::new(1.0, palette.border_soft));
    ui.painter().text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        ext,
        egui::FontId::proportional(11.0),
        palette.text_secondary,
    );
}

fn paint_count_pill(
    ui: &egui::Ui,
    rect: egui::Rect,
    count: usize,
    palette: &Palette,
    selected: bool,
) {
    let fill = if selected {
        rgba(
            palette.accent.r(),
            palette.accent.g(),
            palette.accent.b(),
            35,
        )
    } else {
        palette.surface_inset
    };
    let color = if selected {
        palette.accent
    } else {
        palette.text_secondary
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    draw_icon(
        ui.painter(),
        IconKind::Image,
        egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(10.0, 0.0),
            egui::vec2(10.0, 10.0),
        ),
        color,
        1.3,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.0),
        color,
    );
}

fn paint_lightbox_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    hovered: bool,
    icon: IconKind,
    color: egui::Color32,
) {
    let fill = if hovered {
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 30)
    } else {
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 15)
    };
    ui.painter().rect_filled(rect, rect.width() * 0.5, fill);
    ui.painter().rect_stroke(
        rect,
        rect.width() * 0.5,
        egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_premultiplied(255, 255, 255, 26),
        ),
    );
    draw_icon(
        ui.painter(),
        icon,
        rect.shrink(rect.width() * 0.32),
        color,
        1.8,
    );
}

fn draw_icon(
    painter: &egui::Painter,
    icon: IconKind,
    rect: egui::Rect,
    color: egui::Color32,
    width: f32,
) {
    let stroke = egui::Stroke::new(width, color);
    let p = |x: f32, y: f32| -> egui::Pos2 {
        egui::pos2(
            rect.left() + rect.width() * x / 24.0,
            rect.top() + rect.height() * y / 24.0,
        )
    };
    let r = |x1: f32, y1: f32, x2: f32, y2: f32| -> egui::Rect {
        egui::Rect::from_min_max(p(x1, y1), p(x2, y2))
    };

    match icon {
        IconKind::Folder => {
            painter.add(egui::Shape::line(
                vec![
                    p(3.0, 8.0),
                    p(3.0, 19.0),
                    p(21.0, 19.0),
                    p(21.0, 9.0),
                    p(11.0, 9.0),
                    p(9.0, 6.0),
                    p(5.0, 6.0),
                    p(3.0, 8.0),
                ],
                stroke,
            ));
        }
        IconKind::FolderOpen => {
            painter.add(egui::Shape::line(
                vec![
                    p(3.0, 8.0),
                    p(3.0, 18.0),
                    p(19.0, 18.0),
                    p(21.0, 10.0),
                    p(3.0, 10.0),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![
                    p(3.0, 8.0),
                    p(5.0, 6.0),
                    p(9.0, 6.0),
                    p(11.0, 8.5),
                    p(19.0, 8.5),
                ],
                stroke,
            ));
        }
        IconKind::Image => {
            painter.rect_stroke(r(3.5, 4.5, 20.5, 19.5), 2.0, stroke);
            painter.circle_stroke(p(9.0, 10.0), rect.width() * 1.5 / 24.0, stroke);
            painter.add(egui::Shape::line(
                vec![
                    p(4.5, 17.0),
                    p(9.0, 12.5),
                    p(13.5, 17.0),
                    p(17.0, 13.0),
                    p(20.0, 16.0),
                ],
                stroke,
            ));
        }
        IconKind::Refresh => {
            painter.add(egui::Shape::line(
                vec![p(4.0, 5.0), p(4.0, 10.0), p(9.0, 10.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(20.0, 19.0), p(20.0, 14.0), p(15.0, 14.0)],
                stroke,
            ));
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 7.0 / 24.0, stroke);
        }
        IconKind::History => {
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 8.0 / 24.0, stroke);
            painter.add(egui::Shape::line(
                vec![p(4.0, 4.0), p(4.0, 9.0), p(9.0, 9.0)],
                stroke,
            ));
            painter.line_segment([p(12.0, 7.0), p(12.0, 12.0)], stroke);
            painter.line_segment([p(12.0, 12.0), p(15.0, 14.0)], stroke);
        }
        IconKind::Settings => {
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 3.0 / 24.0, stroke);
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 7.0 / 24.0, stroke);
            painter.line_segment([p(12.0, 2.5), p(12.0, 5.0)], stroke);
            painter.line_segment([p(12.0, 19.0), p(12.0, 21.5)], stroke);
            painter.line_segment([p(2.5, 12.0), p(5.0, 12.0)], stroke);
            painter.line_segment([p(19.0, 12.0), p(21.5, 12.0)], stroke);
            painter.line_segment([p(5.3, 5.3), p(7.0, 7.0)], stroke);
            painter.line_segment([p(17.0, 17.0), p(18.7, 18.7)], stroke);
            painter.line_segment([p(5.3, 18.7), p(7.0, 17.0)], stroke);
            painter.line_segment([p(17.0, 7.0), p(18.7, 5.3)], stroke);
        }
        IconKind::Sun => {
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 4.0 / 24.0, stroke);
            for (a, b) in [(12.0, 3.0), (12.0, 21.0), (3.0, 12.0), (21.0, 12.0)] {
                let dx = if a == 12.0 {
                    0.0
                } else if a < 12.0 {
                    2.0
                } else {
                    -2.0
                };
                let dy = if b == 12.0 {
                    0.0
                } else if b < 12.0 {
                    2.0
                } else {
                    -2.0
                };
                painter.line_segment([p(a, b), p(a + dx, b + dy)], stroke);
            }
            painter.line_segment([p(5.5, 5.5), p(7.0, 7.0)], stroke);
            painter.line_segment([p(17.0, 17.0), p(18.5, 18.5)], stroke);
            painter.line_segment([p(5.5, 18.5), p(7.0, 17.0)], stroke);
            painter.line_segment([p(17.0, 7.0), p(18.5, 5.5)], stroke);
        }
        IconKind::Moon => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p(17.0, 4.5),
                    p(14.0, 7.5),
                    p(13.5, 12.0),
                    p(16.0, 16.5),
                    p(20.0, 18.0),
                    p(16.0, 20.0),
                    p(10.0, 19.0),
                    p(5.0, 14.0),
                    p(4.5, 8.0),
                    p(8.0, 4.5),
                    p(12.0, 3.5),
                ],
                egui::Color32::TRANSPARENT,
                stroke,
            ));
        }
        IconKind::Close => {
            painter.line_segment([p(6.0, 6.0), p(18.0, 18.0)], stroke);
            painter.line_segment([p(18.0, 6.0), p(6.0, 18.0)], stroke);
        }
        IconKind::ChevronLeft => {
            painter.add(egui::Shape::line(
                vec![p(15.0, 6.0), p(9.0, 12.0), p(15.0, 18.0)],
                stroke,
            ));
        }
        IconKind::ChevronRight => {
            painter.add(egui::Shape::line(
                vec![p(9.0, 6.0), p(15.0, 12.0), p(9.0, 18.0)],
                stroke,
            ));
        }
        IconKind::ChevronUp => {
            painter.add(egui::Shape::line(
                vec![p(6.0, 15.0), p(12.0, 9.0), p(18.0, 15.0)],
                stroke,
            ));
        }
        IconKind::Loader => {
            painter.circle_stroke(
                p(12.0, 12.0),
                rect.width() * 7.0 / 24.0,
                egui::Stroke::new(width, rgba(color.r(), color.g(), color.b(), 80)),
            );
            painter.add(egui::Shape::line(
                vec![p(12.0, 5.0), p(16.5, 7.0), p(19.0, 11.0)],
                stroke,
            ));
        }
        IconKind::Doc | IconKind::File => {
            painter.add(egui::Shape::line(
                vec![
                    p(7.0, 3.0),
                    p(14.0, 3.0),
                    p(19.0, 8.0),
                    p(19.0, 21.0),
                    p(5.0, 21.0),
                    p(5.0, 5.0),
                    p(7.0, 3.0),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(14.0, 3.0), p(14.0, 8.0), p(19.0, 8.0)],
                stroke,
            ));
            if matches!(icon, IconKind::Doc) {
                painter.line_segment([p(8.0, 13.0), p(16.0, 13.0)], stroke);
                painter.line_segment([p(8.0, 17.0), p(14.0, 17.0)], stroke);
            }
        }
        IconKind::Video => {
            painter.rect_stroke(r(3.0, 6.0, 16.0, 18.0), 2.0, stroke);
            painter.add(egui::Shape::line(
                vec![p(16.0, 10.0), p(21.0, 7.0), p(21.0, 17.0), p(16.0, 14.0)],
                stroke,
            ));
        }
        IconKind::Archive => {
            painter.rect_stroke(r(3.0, 4.0, 21.0, 8.0), 1.0, stroke);
            painter.rect_stroke(r(5.0, 8.0, 19.0, 20.0), 2.0, stroke);
            painter.line_segment([p(10.0, 13.0), p(14.0, 13.0)], stroke);
        }
        IconKind::Info => {
            painter.circle_stroke(p(12.0, 12.0), rect.width() * 8.0 / 24.0, stroke);
            painter.circle_filled(p(12.0, 8.0), rect.width() * 1.0 / 24.0, color);
            painter.line_segment([p(12.0, 11.0), p(12.0, 17.0)], stroke);
        }
        IconKind::Edit => {
            painter.add(egui::Shape::line(
                vec![
                    p(5.0, 19.0),
                    p(8.5, 18.0),
                    p(19.0, 7.5),
                    p(16.5, 5.0),
                    p(6.0, 15.5),
                    p(5.0, 19.0),
                ],
                stroke,
            ));
            painter.line_segment([p(14.5, 7.0), p(17.0, 9.5)], stroke);
        }
        IconKind::Search => {
            painter.circle_stroke(p(10.5, 10.5), rect.width() * 6.0 / 24.0, stroke);
            painter.line_segment([p(15.0, 15.0), p(20.0, 20.0)], stroke);
        }
    }
}

fn breadcrumb_segments(path: &Path) -> Vec<(String, PathBuf)> {
    let mut segments: Vec<(String, PathBuf)> = Vec::new();
    let mut accumulated = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                accumulated.push(prefix.as_os_str());
                segments.push((
                    prefix.as_os_str().to_string_lossy().to_string(),
                    accumulated.clone(),
                ));
            }
            Component::RootDir => {
                accumulated.push(component.as_os_str());
                if let Some((label, target)) = segments.last_mut() {
                    if !label.ends_with(std::path::MAIN_SEPARATOR) {
                        label.push(std::path::MAIN_SEPARATOR);
                    }
                    *target = accumulated.clone();
                } else {
                    segments.push((std::path::MAIN_SEPARATOR.to_string(), accumulated.clone()));
                }
            }
            Component::Normal(value) => {
                accumulated.push(value);
                segments.push((value.to_string_lossy().to_string(), accumulated.clone()));
            }
            Component::CurDir | Component::ParentDir => {}
        }
    }

    segments
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{}…", truncated)
    }
}

fn elide_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 6 {
        return truncate_str(s, max);
    }
    let keep = (max - 3) / 2;
    let start: String = s.chars().take(keep).collect();
    let end: String = s
        .chars()
        .rev()
        .take(max - 3 - keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}...{}", start, end)
}

fn settings_heading(ui: &mut egui::Ui, title: &str, palette: &Palette) {
    ui.label(
        egui::RichText::new(title)
            .size(15.0)
            .strong()
            .color(palette.text_primary),
    );
    ui.add_space(12.0);
}

fn settings_value_row(ui: &mut egui::Ui, label: &str, value: &str, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.add_sized([136.0, 24.0], egui::Label::new(label));
        ui.label(
            egui::RichText::new(value)
                .monospace()
                .color(palette.text_secondary),
        );
    });
}

fn settings_path_row(ui: &mut egui::Ui, label: &str, path: &Path, palette: &Palette) {
    settings_value_row(
        ui,
        label,
        &elide_middle(&path.to_string_lossy(), 72),
        palette,
    );
}

fn shortcut_capture_row(
    ui: &mut egui::Ui,
    command: CommandId,
    shortcuts: &mut ShortcutConfig,
    capture: &mut Option<CommandId>,
    palette: &Palette,
) {
    let is_capturing = *capture == Some(command);
    let current = shortcuts.get(command);
    let button_text = if is_capturing {
        "Press shortcut...".to_string()
    } else if current.trim().is_empty() {
        "Unassigned".to_string()
    } else {
        current.to_string()
    };

    ui.horizontal(|ui| {
        ui.set_height(34.0);
        ui.add_sized([210.0, 24.0], egui::Label::new(command.label()));
        let response = ui.add_sized(
            [220.0, 27.0],
            egui::Button::new(egui::RichText::new(button_text).monospace().color(
                if is_capturing {
                    palette.accent
                } else {
                    palette.text_primary
                },
            )),
        );
        if response.clicked() {
            *capture = Some(command);
        }
        if ui
            .add_enabled(
                !shortcuts.get(command).is_empty(),
                egui::Button::new("Clear"),
            )
            .clicked()
        {
            shortcuts.get_mut(command).clear();
            *capture = None;
        }
    });
}

fn context_menu_command(
    ui: &mut egui::Ui,
    command: CommandId,
    context: CommandContext,
    shortcut: &str,
) -> bool {
    let label = if shortcut.trim().is_empty() {
        command.short_label().to_string()
    } else {
        format!("{}    {}", command.short_label(), shortcut)
    };
    ui.add_enabled(
        command.enabled(context),
        egui::Button::new(label).frame(false),
    )
    .clicked()
}

fn capture_shortcut(ctx: &egui::Context) -> Option<CapturedShortcut> {
    ctx.input(|input| {
        input.events.iter().rev().find_map(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            else {
                return None;
            };

            if *key == egui::Key::Escape && !modifiers.ctrl && !modifiers.shift && !modifiers.alt {
                return Some(CapturedShortcut::Cancel);
            }

            shortcut_key_name(*key).map(|key_name| {
                let mut parts = Vec::with_capacity(4);
                if modifiers.ctrl {
                    parts.push("Ctrl");
                }
                if modifiers.shift {
                    parts.push("Shift");
                }
                if modifiers.alt {
                    parts.push("Alt");
                }
                parts.push(key_name);
                CapturedShortcut::Value(parts.join("+"))
            })
        })
    })
}

fn shortcut_key_name(key: egui::Key) -> Option<&'static str> {
    match key {
        egui::Key::A => Some("A"),
        egui::Key::B => Some("B"),
        egui::Key::C => Some("C"),
        egui::Key::D => Some("D"),
        egui::Key::E => Some("E"),
        egui::Key::F => Some("F"),
        egui::Key::G => Some("G"),
        egui::Key::H => Some("H"),
        egui::Key::I => Some("I"),
        egui::Key::J => Some("J"),
        egui::Key::K => Some("K"),
        egui::Key::L => Some("L"),
        egui::Key::M => Some("M"),
        egui::Key::N => Some("N"),
        egui::Key::O => Some("O"),
        egui::Key::P => Some("P"),
        egui::Key::Q => Some("Q"),
        egui::Key::R => Some("R"),
        egui::Key::S => Some("S"),
        egui::Key::T => Some("T"),
        egui::Key::U => Some("U"),
        egui::Key::V => Some("V"),
        egui::Key::W => Some("W"),
        egui::Key::X => Some("X"),
        egui::Key::Y => Some("Y"),
        egui::Key::Z => Some("Z"),
        egui::Key::Num0 => Some("0"),
        egui::Key::Num1 => Some("1"),
        egui::Key::Num2 => Some("2"),
        egui::Key::Num3 => Some("3"),
        egui::Key::Num4 => Some("4"),
        egui::Key::Num5 => Some("5"),
        egui::Key::Num6 => Some("6"),
        egui::Key::Num7 => Some("7"),
        egui::Key::Num8 => Some("8"),
        egui::Key::Num9 => Some("9"),
        egui::Key::Enter => Some("Enter"),
        egui::Key::Delete => Some("Delete"),
        egui::Key::Backspace => Some("Backspace"),
        egui::Key::Space => Some("Space"),
        egui::Key::Tab => Some("Tab"),
        egui::Key::Insert => Some("Insert"),
        egui::Key::Home => Some("Home"),
        egui::Key::End => Some("End"),
        egui::Key::PageUp => Some("PageUp"),
        egui::Key::PageDown => Some("PageDown"),
        egui::Key::ArrowLeft => Some("Left"),
        egui::Key::ArrowRight => Some("Right"),
        egui::Key::ArrowUp => Some("Up"),
        egui::Key::ArrowDown => Some("Down"),
        egui::Key::F1 => Some("F1"),
        egui::Key::F2 => Some("F2"),
        egui::Key::F3 => Some("F3"),
        egui::Key::F4 => Some("F4"),
        egui::Key::F5 => Some("F5"),
        egui::Key::F6 => Some("F6"),
        egui::Key::F7 => Some("F7"),
        egui::Key::F8 => Some("F8"),
        egui::Key::F9 => Some("F9"),
        egui::Key::F10 => Some("F10"),
        egui::Key::F11 => Some("F11"),
        egui::Key::F12 => Some("F12"),
        egui::Key::Comma => Some(","),
        _ => None,
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn shortcut_pressed(ctx: &egui::Context, spec: &str) -> bool {
    let parts: Vec<String> = spec
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return false;
    }

    let wants_ctrl = parts.iter().any(|p| p == "ctrl" || p == "control");
    let wants_shift = parts.iter().any(|p| p == "shift");
    let wants_alt = parts.iter().any(|p| p == "alt");
    let Some(key_name) = parts
        .iter()
        .rev()
        .find(|p| *p != "ctrl" && *p != "control" && *p != "shift" && *p != "alt")
    else {
        return false;
    };
    let Some(key) = parse_shortcut_key(key_name) else {
        return false;
    };

    ctx.input(|input| {
        input.key_pressed(key)
            && input.modifiers.ctrl == wants_ctrl
            && input.modifiers.shift == wants_shift
            && input.modifiers.alt == wants_alt
    })
}

fn parse_shortcut_key(name: &str) -> Option<egui::Key> {
    match name {
        "a" => Some(egui::Key::A),
        "b" => Some(egui::Key::B),
        "c" => Some(egui::Key::C),
        "d" => Some(egui::Key::D),
        "e" => Some(egui::Key::E),
        "f" => Some(egui::Key::F),
        "g" => Some(egui::Key::G),
        "h" => Some(egui::Key::H),
        "i" => Some(egui::Key::I),
        "j" => Some(egui::Key::J),
        "k" => Some(egui::Key::K),
        "l" => Some(egui::Key::L),
        "m" => Some(egui::Key::M),
        "n" => Some(egui::Key::N),
        "o" => Some(egui::Key::O),
        "p" => Some(egui::Key::P),
        "q" => Some(egui::Key::Q),
        "r" => Some(egui::Key::R),
        "s" => Some(egui::Key::S),
        "t" => Some(egui::Key::T),
        "u" => Some(egui::Key::U),
        "v" => Some(egui::Key::V),
        "w" => Some(egui::Key::W),
        "x" => Some(egui::Key::X),
        "y" => Some(egui::Key::Y),
        "z" => Some(egui::Key::Z),
        "0" => Some(egui::Key::Num0),
        "1" => Some(egui::Key::Num1),
        "2" => Some(egui::Key::Num2),
        "3" => Some(egui::Key::Num3),
        "4" => Some(egui::Key::Num4),
        "5" => Some(egui::Key::Num5),
        "6" => Some(egui::Key::Num6),
        "7" => Some(egui::Key::Num7),
        "8" => Some(egui::Key::Num8),
        "9" => Some(egui::Key::Num9),
        "enter" | "return" => Some(egui::Key::Enter),
        "delete" | "del" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "escape" | "esc" => Some(egui::Key::Escape),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        "insert" => Some(egui::Key::Insert),
        "home" => Some(egui::Key::Home),
        "end" => Some(egui::Key::End),
        "pageup" => Some(egui::Key::PageUp),
        "pagedown" => Some(egui::Key::PageDown),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        "f1" => Some(egui::Key::F1),
        "f2" => Some(egui::Key::F2),
        "f3" => Some(egui::Key::F3),
        "f4" => Some(egui::Key::F4),
        "f5" => Some(egui::Key::F5),
        "f6" => Some(egui::Key::F6),
        "f7" => Some(egui::Key::F7),
        "f8" => Some(egui::Key::F8),
        "f9" => Some(egui::Key::F9),
        "f10" => Some(egui::Key::F10),
        "f11" => Some(egui::Key::F11),
        "f12" => Some(egui::Key::F12),
        "," | "comma" => Some(egui::Key::Comma),
        _ => None,
    }
}

fn default_drop_mode(
    sources: &[PathBuf],
    target_dir: &Path,
    modifiers: egui::Modifiers,
) -> TransferMode {
    if modifiers.ctrl {
        return TransferMode::Copy;
    }
    if modifiers.shift {
        return TransferMode::Move;
    }

    let Some(target_volume) = volume_key(target_dir) else {
        return TransferMode::Copy;
    };
    if !sources.is_empty()
        && sources
            .iter()
            .all(|source| volume_key(source).as_ref() == Some(&target_volume))
    {
        TransferMode::Move
    } else {
        TransferMode::Copy
    }
}

fn volume_key(path: &Path) -> Option<String> {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => Some(
            prefix
                .as_os_str()
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase(),
        ),
        Some(Component::RootDir) => Some("\\".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{breadcrumb_segments, default_drop_mode};
    use crate::file_ops::TransferMode;
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    fn breadcrumbs_accumulate_clickable_parent_paths() {
        let path = PathBuf::from("Products").join("Nails").join("2026");
        let segments = breadcrumb_segments(&path);

        assert_eq!(
            segments
                .iter()
                .map(|(label, _)| label.as_str())
                .collect::<Vec<_>>(),
            vec!["Products", "Nails", "2026"]
        );
        assert_eq!(segments[0].1, PathBuf::from("Products"));
        assert_eq!(segments[1].1, PathBuf::from("Products").join("Nails"));
        assert_eq!(segments[2].1, path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_root_is_one_breadcrumb_segment() {
        let segments = breadcrumb_segments(std::path::Path::new(r"C:\Products\Nails"));

        assert_eq!(segments[0].0, r"C:\");
        assert_eq!(segments[0].1, PathBuf::from(r"C:\"));
        assert_eq!(segments[1].0, "Products");
        assert_eq!(segments[2].0, "Nails");
    }

    #[cfg(windows)]
    #[test]
    fn drag_drop_uses_windows_volume_and_modifier_rules() {
        let sources = vec![PathBuf::from(r"C:\Products\1001\photo.jpg")];

        assert_eq!(
            default_drop_mode(
                &sources,
                std::path::Path::new(r"C:\Products\1002"),
                egui::Modifiers::default(),
            ),
            TransferMode::Move
        );
        assert_eq!(
            default_drop_mode(
                &sources,
                std::path::Path::new(r"D:\Archive"),
                egui::Modifiers::default(),
            ),
            TransferMode::Copy
        );
        assert_eq!(
            default_drop_mode(
                &sources,
                std::path::Path::new(r"C:\Products\1002"),
                egui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            ),
            TransferMode::Copy
        );
        assert_eq!(
            default_drop_mode(
                &sources,
                std::path::Path::new(r"D:\Archive"),
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
            TransferMode::Move
        );
    }
}
