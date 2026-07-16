use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::{fs, io};

use eframe::egui;

use crate::commands::{CommandContext, CommandId};
use crate::config::{AppConfig, ShortcutConfig};
use crate::model::{FileEntry, FolderEntry, ItemId, SortDirection, SortField, ViewMode};
use crate::navigation::NavigationHistory;
use crate::scanner::{format_size, ScanOptions};
use crate::selection::SelectionModel;
use crate::sorting::{sort_files, sort_folders};
use crate::storage::{transactional_write, StoragePaths};
use crate::tasks::{TaskEvent, TaskId, TaskKind, TaskManager};
use crate::thumbs::ThumbManager;

mod details;
mod explorer;
mod first_run;
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
    pending_delete: Option<PendingDelete>,

    status: String,
}

struct RenameState {
    original_path: PathBuf,
    new_name: String,
}

#[derive(Clone, Copy)]
enum ClipboardMode {
    Copy,
    Cut,
}

struct FileClipboard {
    paths: Vec<PathBuf>,
    mode: ClipboardMode,
}

enum PendingDelete {
    Paths(Vec<PathBuf>),
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
        let effective_theme = apply_theme(&cc.egui_ctx, &config.theme).to_string();
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
            thumb_mgr: ThumbManager::new(3000, 4),
            full_mgr: ThumbManager::new(5, 2),
            tasks: TaskManager::new(4),
            root_scan_task: None,
            folder_scan_task: None,
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
            pending_delete: None,
            status: "Ready. Pick a folder above or choose a recent path.".into(),
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
            has_clipboard: self.clipboard.is_some(),
            has_target_directory: self.active_target_dir().is_some(),
            has_current_path,
            has_items: !self.folders.is_empty() || !self.right_files.is_empty(),
            has_previewable_selection,
            can_navigate_back: self.navigation.can_back(),
            can_navigate_forward: self.navigation.can_forward(),
            can_navigate_up: self.navigation.can_up(),
            ..CommandContext::default()
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
                    paths,
                    mode: ClipboardMode::Copy,
                });
                self.status = format!("Copied {} item(s) to LuxePix clipboard.", count);
            }
            CommandId::Cut => {
                let paths = self.selected_paths();
                let count = paths.len();
                self.clipboard = Some(FileClipboard {
                    paths,
                    mode: ClipboardMode::Cut,
                });
                self.status = format!("Cut {} item(s). Paste to move.", count);
            }
            CommandId::Paste => self.paste_clipboard(),
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
            CommandId::Delete | CommandId::DeletePermanent => {
                let paths = self.selected_paths();
                self.pending_delete = Some(PendingDelete::Paths(paths));
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
            CommandId::NewFolder | CommandId::Undo | CommandId::Find => {
                self.status = format!(
                    "{} is registered and will be enabled with its explorer feature.",
                    command.short_label()
                );
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
        let Some(clip) = &self.clipboard else {
            self.status = "LuxePix clipboard is empty.".into();
            return;
        };

        let paths = clip.paths.clone();
        let mode = clip.mode;
        let mut completed = 0usize;
        let mut last_error: Option<String> = None;

        for src in &paths {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dst = unique_destination(&target_dir.join(name));
            let result = match mode {
                ClipboardMode::Copy => copy_path(src, &dst),
                ClipboardMode::Cut => move_path(src, &dst),
            };
            match result {
                Ok(()) => completed += 1,
                Err(e) => last_error = Some(format!("{}: {}", src.display(), e)),
            }
        }

        if matches!(mode, ClipboardMode::Cut) {
            self.clipboard = None;
        }
        self.refresh_current_view();
        if let Some(err) = last_error {
            self.status = format!("Pasted {} item(s), last error: {}", completed, err);
        } else {
            self.status = format!(
                "Pasted {} item(s) into {}.",
                completed,
                target_dir.display()
            );
        }
    }

    fn confirm_delete(&mut self) {
        let Some(PendingDelete::Paths(paths)) = self.pending_delete.take() else {
            return;
        };
        let mut deleted = 0usize;
        let mut last_error: Option<String> = None;
        for path in paths {
            let result = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match result {
                Ok(()) => deleted += 1,
                Err(e) => last_error = Some(format!("{}: {}", path.display(), e)),
            }
        }
        self.refresh_current_view();
        if let Some(err) = last_error {
            self.status = format!("Deleted {} item(s), last error: {}", deleted, err);
        } else {
            self.status = format!("Deleted {} item(s).", deleted);
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input()
            || self.preview.is_some()
            || self.renaming.is_some()
            || self.show_settings
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
        let palette = Palette::for_theme(&self.effective_theme);

        self.process_task_events();

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
                    .inner_margin(egui::Margin::symmetric(10.0, 12.0)),
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
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0)),
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

        // ── Preview overlay ──
        if self.preview.is_some() {
            self.render_preview(ctx);
        }

        if self.show_settings {
            self.render_settings_window(ctx);
        }

        if self.pending_delete.is_some() {
            self.render_delete_confirmation(ctx);
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
        if self.tasks.active_count() > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
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
                .allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover())
                .0;
            ui.painter().rect_filled(mark_rect, 7.0, palette.accent);
            draw_icon(
                ui.painter(),
                IconKind::Image,
                mark_rect.shrink(6.0),
                egui::Color32::WHITE,
                1.6,
            );
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
                                                egui::vec2(ui.available_width(), 38.0),
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
                                                rect.min + egui::vec2(14.0, 11.0),
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
        let mut close_window = false;

        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size(egui::vec2(760.0, 570.0))
            .min_width(680.0)
            .min_height(480.0)
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
                                    &mut self.config.confirm_permanent_delete,
                                    "Confirm permanent deletion",
                                )
                                .changed();
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(if self.clipboard.is_some() {
                                    "LuxePix clipboard contains selected items."
                                } else {
                                    "LuxePix clipboard is empty."
                                })
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
                            ui.add_space(12.0);
                            if ui.button("Open Data Folder").clicked() {
                                Self::open_file_external(&self.storage.root);
                            }
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

        if config_changed {
            self.effective_theme = apply_theme(ctx, &self.config.theme).to_string();
            if let Err(error) = self.config.save() {
                self.status = format!("Could not save settings: {error}");
            }
        }
        if hidden_files_changed {
            self.refresh_current_view();
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
        let Some(PendingDelete::Paths(paths)) = &self.pending_delete else {
            return;
        };
        let count = paths.len();
        let sample = paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new("Delete")
            .resizable(false)
            .collapsible(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Delete {} selected item(s)?", count));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(elide_middle(&sample, 72))
                        .size(12.0)
                        .color(Palette::for_theme(&self.effective_theme).text_secondary),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("This removes files from disk.")
                        .size(12.0)
                        .color(Palette::for_theme(&self.effective_theme).error),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
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

    fn render_initial_state(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                let art = ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::hover()).0;
                ui.painter().rect_filled(art, 18.0, palette.surface_inset);
                ui.painter().rect_stroke(art, 18.0, egui::Stroke::new(1.0, palette.border_soft));
                draw_icon(ui.painter(), IconKind::HardDrive, art.shrink(28.0), palette.text_tertiary, 1.7);
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
        ui.centered_and_justified(|ui| {
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
            .inner_margin(egui::Margin::symmetric(18.0, 8.0))
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
                        let tile_h = thumb_h + 58.0;
                        let (rect, tile_resp) = ui
                            .allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::click());
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
                            egui::Sense::click(),
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
                            egui::pos2(rect.min.x + 10.0, rect.min.y + thumb_h + 8.0),
                            egui::vec2(tile_w - 20.0, 22.0),
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
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                truncate_str(&folder.name, 28),
                                egui::FontId::proportional(13.0),
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
                        let info_h = 48.0;
                        let tile_h = tile_w + info_h;
                        let (rect, tile_resp) = ui
                            .allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::click());
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
                            egui::Sense::click(),
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
                            egui::pos2(rect.min.x + 10.0, rect.min.y + tile_w + 7.0),
                            egui::vec2(tile_w - 20.0, 21.0),
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
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                truncate_str(&file.name, 30),
                                egui::FontId::proportional(12.5),
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
    HardDrive,
    Loader,
    Doc,
    Video,
    File,
    Archive,
    Info,
    Edit,
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
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
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
        IconKind::HardDrive => {
            painter.rect_stroke(r(3.0, 13.0, 21.0, 19.0), 2.0, stroke);
            painter.add(egui::Shape::line(
                vec![p(6.0, 13.0), p(8.0, 6.0), p(16.0, 6.0), p(18.0, 13.0)],
                stroke,
            ));
            painter.circle_filled(p(7.5, 16.0), rect.width() * 0.8 / 24.0, color);
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
        ui.add_sized([130.0, 22.0], egui::Label::new(label));
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

fn unique_destination(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("item");
    let ext = path.extension().and_then(|e| e.to_str());
    for i in 1..10_000 {
        let suffix = if i == 1 {
            " - Copy".to_string()
        } else {
            format!(" - Copy {}", i)
        };
        let name = if let Some(ext) = ext {
            format!("{}{}.{}", stem, suffix, ext)
        } else {
            format!("{}{}", stem, suffix)
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{} - Copy {}", stem, uuid_like_suffix()))
}

fn uuid_like_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "new".into())
}

fn copy_path(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
        Ok(())
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            copy_path(src, dst)?;
            let cleanup = if src.is_dir() {
                fs::remove_dir_all(src)
            } else {
                fs::remove_file(src)
            };
            cleanup.map_err(|cleanup_err| {
                io::Error::new(
                    cleanup_err.kind(),
                    format!(
                        "moved by copy but cleanup failed after rename error '{}': {}",
                        rename_err, cleanup_err
                    ),
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::breadcrumb_segments;
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
}
