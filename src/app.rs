use std::path::{Path, PathBuf};
use std::{fs, io};

use eframe::egui;

use crate::config::{AppConfig, ShortcutConfig};
use crate::scanner::{self, format_size, FileEntry, FolderEntry};
use crate::thumbs::ThumbManager;

// ── Preview state ──────────────────────────────────────────────
struct PreviewState {
    path: PathBuf,
    index: usize,
    zoom: f32,
    offset: egui::Vec2,
}

// ── App state ──────────────────────────────────────────────────
pub struct LuxePixApp {
    path_input: String,
    current_path: Option<PathBuf>,

    folders: Vec<FolderEntry>,
    selected_folder: Option<usize>,
    left_thumb_size: f32,

    right_files: Vec<FileEntry>,
    right_image_paths: Vec<PathBuf>,
    selected_file: Option<usize>,
    right_thumb_size: f32,

    preview: Option<PreviewState>,

    thumb_mgr: ThumbManager,
    full_mgr: ThumbManager,

    // Rename state
    renaming: Option<RenameState>,

    // Config & history
    config: AppConfig,
    show_history: bool,
    show_settings: bool,
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

#[derive(Clone, Copy)]
enum FileCommand {
    Copy,
    Cut,
    Paste,
    Rename,
    Delete,
    Open,
}

impl LuxePixApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let config = AppConfig::load();
        apply_theme(&cc.egui_ctx, &config.theme);

        let mut app = Self {
            path_input: String::new(),
            current_path: None,
            folders: Vec::new(),
            selected_folder: None,
            left_thumb_size: config.left_thumb_size,
            right_files: Vec::new(),
            right_image_paths: Vec::new(),
            selected_file: None,
            right_thumb_size: config.right_thumb_size,
            preview: None,
            thumb_mgr: ThumbManager::new(3000, 4),
            full_mgr: ThumbManager::new(5, 2),
            renaming: None,
            config,
            show_history: false,
            show_settings: false,
            clipboard: None,
            pending_delete: None,
            status: "Ready. Pick a folder above or choose a recent path.".into(),
        };

        // Restore last opened path
        if let Some(ref last) = app.config.last_path.clone() {
            let p = PathBuf::from(last);
            if p.is_dir() {
                app.path_input = last.clone();
                app.current_path = Some(p.clone());
                app.folders = scanner::scan_directory(&p);
                let total_imgs: usize = app.folders.iter().map(|f| f.image_count).sum();
                app.status = format!(
                    "Restored {} folders and {} images from last path.",
                    app.folders.len(),
                    total_imgs
                );
            }
        }

        app
    }

    fn navigate_to(&mut self, path: PathBuf) {
        self.current_path = Some(path.clone());
        self.path_input = path.to_string_lossy().to_string();
        self.renaming = None;
        self.show_history = false;

        // Save to history
        self.config.add_path(&self.path_input);

        self.folders = scanner::scan_directory(&path);
        self.selected_folder = None;
        self.right_files.clear();
        self.right_image_paths.clear();
        self.preview = None;

        let total_imgs: usize = self.folders.iter().map(|f| f.image_count).sum();
        let total_files: usize = self.folders.iter().map(|f| f.total_files).sum();
        self.status = format!(
            "Scanned {} folders, {} images and {} files.",
            self.folders.len(),
            total_imgs,
            total_files
        );
    }

    fn select_folder(&mut self, idx: usize) {
        if idx >= self.folders.len() {
            return;
        }
        self.selected_folder = Some(idx);
        self.selected_file = None;
        self.right_files = scanner::list_all_files(&self.folders[idx].path);
        self.right_image_paths = self
            .right_files
            .iter()
            .filter(|f| f.is_image)
            .map(|f| f.path.clone())
            .collect();
        let img_count = self.right_image_paths.len();
        let other_count = self.right_files.len() - img_count;
        self.status = format!(
            "{}: {} images and {} other files.",
            self.folders[idx].name, img_count, other_count
        );
        self.preview = None;
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
                        // Refresh
                        if let Some(path) = self.current_path.clone() {
                            self.folders = scanner::scan_directory(&path);
                            if let Some(sel) = self.selected_folder {
                                if sel < self.folders.len() {
                                    self.select_folder(sel);
                                }
                            }
                        }
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

    fn selected_paths(&self) -> Vec<PathBuf> {
        if let Some(idx) = self.selected_file {
            if let Some(file) = self.right_files.get(idx) {
                return vec![file.path.clone()];
            }
        }
        if let Some(idx) = self.selected_folder {
            if let Some(folder) = self.folders.get(idx) {
                return vec![folder.path.clone()];
            }
        }
        Vec::new()
    }

    fn active_target_dir(&self) -> Option<PathBuf> {
        self.selected_folder
            .and_then(|idx| self.folders.get(idx))
            .map(|folder| folder.path.clone())
            .or_else(|| self.current_path.clone())
    }

    fn refresh_current_view(&mut self) {
        if let Some(path) = self.current_path.clone() {
            let selected_folder_path = self
                .selected_folder
                .and_then(|idx| self.folders.get(idx))
                .map(|f| f.path.clone());
            self.folders = scanner::scan_directory(&path);
            if let Some(sel_path) = selected_folder_path {
                if let Some(new_idx) = self.folders.iter().position(|f| f.path == sel_path) {
                    self.select_folder(new_idx);
                } else {
                    self.selected_folder = None;
                    self.right_files.clear();
                    self.right_image_paths.clear();
                    self.selected_file = None;
                }
            }
        }
    }

    fn execute_file_command(&mut self, command: FileCommand) {
        match command {
            FileCommand::Copy => {
                let paths = self.selected_paths();
                if paths.is_empty() {
                    self.status = "Select a file or folder first.".into();
                } else {
                    let count = paths.len();
                    self.clipboard = Some(FileClipboard {
                        paths,
                        mode: ClipboardMode::Copy,
                    });
                    self.status = format!("Copied {} item(s) to LuxePix clipboard.", count);
                }
            }
            FileCommand::Cut => {
                let paths = self.selected_paths();
                if paths.is_empty() {
                    self.status = "Select a file or folder first.".into();
                } else {
                    let count = paths.len();
                    self.clipboard = Some(FileClipboard {
                        paths,
                        mode: ClipboardMode::Cut,
                    });
                    self.status = format!("Cut {} item(s). Paste to move.", count);
                }
            }
            FileCommand::Paste => self.paste_clipboard(),
            FileCommand::Rename => {
                if let Some(idx) = self.selected_file {
                    if let Some(file) = self.right_files.get(idx) {
                        self.renaming = Some(RenameState {
                            original_path: file.path.clone(),
                            new_name: file.name.clone(),
                        });
                    }
                } else if let Some(idx) = self.selected_folder {
                    if let Some(folder) = self.folders.get(idx) {
                        self.renaming = Some(RenameState {
                            original_path: folder.path.clone(),
                            new_name: folder.name.clone(),
                        });
                    }
                } else {
                    self.status = "Select a file or folder first.".into();
                }
            }
            FileCommand::Delete => {
                let paths = self.selected_paths();
                if paths.is_empty() {
                    self.status = "Select a file or folder first.".into();
                } else {
                    self.pending_delete = Some(PendingDelete::Paths(paths));
                }
            }
            FileCommand::Open => self.open_selected_item(),
        }
    }

    fn open_selected_item(&mut self) {
        if let Some(idx) = self.selected_file {
            if let Some(file) = self.right_files.get(idx) {
                if file.is_image {
                    if let Some(img_idx) =
                        self.right_image_paths.iter().position(|p| p == &file.path)
                    {
                        let p = self.right_image_paths[img_idx].clone();
                        self.full_mgr.request_full(&p);
                        self.preview = Some(PreviewState {
                            path: p,
                            index: img_idx,
                            zoom: 1.0,
                            offset: egui::Vec2::ZERO,
                        });
                    }
                } else {
                    Self::open_file_external(&file.path);
                }
                return;
            }
        }
        if let Some(idx) = self.selected_folder {
            if let Some(folder) = self.folders.get(idx) {
                self.navigate_to(folder.path.clone());
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
        {
            return;
        }
        if shortcut_pressed(ctx, &self.config.shortcuts.settings) {
            self.show_settings = true;
        } else if shortcut_pressed(ctx, &self.config.shortcuts.copy) {
            self.execute_file_command(FileCommand::Copy);
        } else if shortcut_pressed(ctx, &self.config.shortcuts.cut) {
            self.execute_file_command(FileCommand::Cut);
        } else if shortcut_pressed(ctx, &self.config.shortcuts.paste) {
            self.execute_file_command(FileCommand::Paste);
        } else if shortcut_pressed(ctx, &self.config.shortcuts.rename) {
            self.execute_file_command(FileCommand::Rename);
        } else if shortcut_pressed(ctx, &self.config.shortcuts.delete) {
            self.execute_file_command(FileCommand::Delete);
        } else if shortcut_pressed(ctx, &self.config.shortcuts.open) {
            self.execute_file_command(FileCommand::Open);
        }
    }
}

// ── eframe::App impl ──────────────────────────────────────────
impl eframe::App for LuxePixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let palette = Palette::for_theme(&self.config.theme);

        // Process thumbnail results
        let loaded_thumbs = self.thumb_mgr.process_results(ctx);
        let loaded_full = self.full_mgr.process_results(ctx);
        if loaded_thumbs || loaded_full {
            ctx.request_repaint();
        }

        self.handle_global_shortcuts(ctx);

        // ── Arrow key navigation (when not in preview and not renaming) ──
        if self.preview.is_none() && self.renaming.is_none() {
            let mut key_left = false;
            let mut key_right = false;
            let mut key_up = false;
            let mut key_down = false;
            let mut key_enter = false;
            ctx.input(|input| {
                key_left = input.key_pressed(egui::Key::ArrowLeft);
                key_right = input.key_pressed(egui::Key::ArrowRight);
                key_up = input.key_pressed(egui::Key::ArrowUp);
                key_down = input.key_pressed(egui::Key::ArrowDown);
                key_enter = input.key_pressed(egui::Key::Enter);
            });

            if !self.folders.is_empty() {
                // Left pane: Left/Right = prev/next, Up/Down = row nav
                let left_cols = ((self.config.left_panel_width - 20.0)
                    / (self.left_thumb_size + 6.0))
                    .max(1.0) as i32;

                let mut delta: Option<i32> = None;
                if key_left {
                    delta = Some(-1);
                }
                if key_right {
                    delta = Some(1);
                }
                if key_up {
                    delta = Some(-left_cols);
                }
                if key_down {
                    delta = Some(left_cols);
                }

                if let Some(d) = delta {
                    let current = self.selected_folder.unwrap_or(0) as i32;
                    let new_idx = (current + d).clamp(0, self.folders.len() as i32 - 1) as usize;
                    self.select_folder(new_idx);
                }

                // Enter on folder → navigate into it
                if key_enter && self.selected_file.is_none() {
                    if let Some(idx) = self.selected_folder {
                        let p = self.folders[idx].path.clone();
                        self.navigate_to(p);
                    }
                }
            }
        }

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

        // ── Left pane (folder grid) ──
        let panel_width = self.config.left_panel_width;
        let left_resp = egui::SidePanel::left("left_pane")
            .default_width(panel_width)
            .min_width(280.0)
            .max_width(760.0)
            .resizable(true)
            .frame(egui::Frame::none().fill(palette.surface))
            .show(ctx, |ui| {
                self.render_left_pane(ui);
            });
        // Track panel width for config save
        let new_panel_w = left_resp.response.rect.width();

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

        // (Rename is now inline in panes — no separate dialog needed)

        // ── Save UI state periodically ──
        self.config
            .save_ui_state(self.left_thumb_size, self.right_thumb_size, new_panel_w);

        // Request repaint if there are pending thumbnails
        if !self.thumb_mgr.is_idle() || !self.full_mgr.is_idle() {
            ctx.request_repaint();
        }
    }
}

// ── UI Rendering ──────────────────────────────────────────────
impl LuxePixApp {
    fn render_address_bar(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.config.theme);

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

            let path_width = (ui.available_width() - 250.0).clamp(260.0, 620.0);
            let path_frame = egui::Frame::none()
                .fill(palette.surface_inset)
                .stroke(egui::Stroke::new(1.0, palette.border_soft))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 0.0));

            let path_resp = path_frame
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
                        let resp = ui.add_sized(
                            [ui.available_width(), 26.0],
                            egui::TextEdit::singleline(&mut self.path_input)
                                .hint_text("Paste or type a folder path...")
                                .font(egui::TextStyle::Body)
                                .frame(false)
                                .text_color(palette.text_primary),
                        );
                        resp
                    })
                    .inner
                })
                .inner;

            if path_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let p = PathBuf::from(&self.path_input);
                if p.is_dir() {
                    self.navigate_to(p);
                } else {
                    self.status = format!("Path not found or not accessible: {}", self.path_input);
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

            if icon_button(ui, IconKind::Refresh, None, "Refresh", false, &palette).clicked() {
                if let Some(p) = self.current_path.clone() {
                    self.thumb_mgr.clear();
                    self.navigate_to(p);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let theme_icon = if self.config.theme == "dark" {
                    IconKind::Sun
                } else {
                    IconKind::Moon
                };
                let tip = if self.config.theme == "dark" {
                    "Switch to light theme"
                } else {
                    "Switch to dark theme"
                };
                if icon_button(ui, theme_icon, None, tip, false, &palette).clicked() {
                    self.config.toggle_theme();
                    apply_theme(ui.ctx(), &self.config.theme);
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
                    self.show_settings = true;
                }
            });
        });
    }

    fn render_history_popover(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.config.theme);
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
        let palette = Palette::for_theme(&self.config.theme);
        ui.horizontal_centered(|ui| {
            let dot_color = status_color(&self.status, &palette);
            let dot_rect = ui
                .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                .0;
            ui.painter()
                .circle_filled(dot_rect.center(), 3.0, dot_color);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&self.status)
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
        let palette = Palette::for_theme(&self.config.theme);
        let mut open = self.show_settings;
        let mut save = false;
        let mut reset = false;
        let mut changed = false;

        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Keyboard shortcuts")
                        .size(15.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Format: Ctrl+C, Ctrl+Shift+C, F2, Delete, Enter.")
                        .size(12.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(12.0);

                changed |= shortcut_row(ui, "Copy", &mut self.config.shortcuts.copy);
                changed |= shortcut_row(ui, "Cut", &mut self.config.shortcuts.cut);
                changed |= shortcut_row(ui, "Paste", &mut self.config.shortcuts.paste);
                changed |= shortcut_row(ui, "Rename", &mut self.config.shortcuts.rename);
                changed |= shortcut_row(ui, "Delete", &mut self.config.shortcuts.delete);
                changed |= shortcut_row(ui, "Open", &mut self.config.shortcuts.open);
                changed |= shortcut_row(ui, "Settings", &mut self.config.shortcuts.settings);

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Reset defaults").clicked() {
                        reset = true;
                    }
                });
            });

        if reset {
            self.config.shortcuts = ShortcutConfig::default();
            self.config.save();
            self.status = "Shortcut defaults restored.".into();
        } else if save || changed {
            self.config.save();
            if save {
                self.status = "Shortcut settings saved.".into();
            }
        }
        self.show_settings = open;
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
                        .color(Palette::for_theme(&self.config.theme).text_secondary),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("This removes files from disk.")
                        .size(12.0)
                        .color(Palette::for_theme(&self.config.theme).error),
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
        let palette = Palette::for_theme(&self.config.theme);
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
        let palette = Palette::for_theme(&self.config.theme);
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
        let has_selection = self.selected_file.is_some();
        if !has_selection && self.clipboard.is_none() {
            return;
        }

        egui::Frame::none()
            .fill(palette.canvas)
            .inner_margin(egui::Margin::symmetric(18.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let selected_label = self
                        .selected_file
                        .and_then(|idx| self.right_files.get(idx))
                        .map(|f| truncate_str(&f.name, 34))
                        .unwrap_or_else(|| "Clipboard ready".into());
                    ui.label(
                        egui::RichText::new(selected_label)
                            .size(12.0)
                            .color(palette.text_secondary),
                    );
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Open"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Open);
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Copy"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Copy);
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Cut"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Cut);
                    }
                    if ui
                        .add_enabled(self.clipboard.is_some(), egui::Button::new("Paste"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Paste);
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Rename"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Rename);
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.execute_file_command(FileCommand::Delete);
                    }
                });
            });
    }

    fn render_left_pane(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.config.theme);
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
        let selected = self.selected_folder;
        let mut new_select: Option<usize> = None;
        let mut start_rename: Option<(PathBuf, String)> = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut context_command: Option<FileCommand> = None;

        let renaming_path = self.renaming.as_ref().map(|r| r.original_path.clone());

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);

                    for (i, folder) in self.folders.iter().enumerate() {
                        let tile_w = thumb_size.clamp(116.0, 360.0);
                        let thumb_h = (tile_w * 0.75).max(86.0);
                        let tile_h = thumb_h + 58.0;
                        let (rect, tile_resp) = ui
                            .allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::click());

                        if !ui.is_rect_visible(rect) {
                            if let Some(ref p) = folder.first_image {
                                self.thumb_mgr.request_thumbnail(p, thumb_size as u32);
                            }
                            continue;
                        }

                        if let Some(ref p) = folder.first_image {
                            self.thumb_mgr.request_thumbnail(p, thumb_size as u32);
                        }

                        let is_sel = selected == Some(i);
                        paint_tile_shell(ui, rect, is_sel, tile_resp.hovered(), &palette);

                        let t_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(tile_w, thumb_h));
                        let thumb_resp =
                            ui.interact(t_rect, ui.id().with(("lt", i)), egui::Sense::click());

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

                        if thumb_resp.clicked() || tile_resp.clicked() {
                            new_select = Some(i);
                        }
                        if thumb_resp.secondary_clicked() || tile_resp.secondary_clicked() {
                            new_select = Some(i);
                        }
                        tile_resp.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                context_command = Some(FileCommand::Open);
                                ui.close_menu();
                            }
                            if ui.button("Copy").clicked() {
                                context_command = Some(FileCommand::Copy);
                                ui.close_menu();
                            }
                            if ui.button("Cut").clicked() {
                                context_command = Some(FileCommand::Cut);
                                ui.close_menu();
                            }
                            if ui.button("Paste").clicked() {
                                context_command = Some(FileCommand::Paste);
                                ui.close_menu();
                            }
                            if ui.button("Rename").clicked() {
                                context_command = Some(FileCommand::Rename);
                                ui.close_menu();
                            }
                            if ui.button("Delete").clicked() {
                                context_command = Some(FileCommand::Delete);
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
                            let name_resp = ui.interact(
                                name_rect,
                                ui.id().with(("ln", i)),
                                egui::Sense::click(),
                            );
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                truncate_str(&folder.name, 28),
                                egui::FontId::proportional(13.0),
                                if name_resp.hovered() {
                                    palette.accent
                                } else {
                                    palette.text_primary
                                },
                            );
                            if name_resp.clicked() {
                                start_rename = Some((folder.path.clone(), folder.name.clone()));
                            }
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

        if let Some(idx) = new_select {
            self.select_folder(idx);
        }
        if let Some((path, name)) = start_rename {
            self.renaming = Some(RenameState {
                original_path: path,
                new_name: name,
            });
        }
        if commit_rename {
            self.execute_rename();
        } else if cancel_rename {
            self.renaming = None;
        }
        if let Some(command) = context_command {
            self.execute_file_command(command);
        }
    }

    fn render_right_pane(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.config.theme);
        let folder_name = self
            .selected_folder
            .and_then(|i| self.folders.get(i))
            .map(|f| f.name.as_str())
            .unwrap_or("");
        let img_count = self.right_image_paths.len();
        let other_count = self.right_files.len() - img_count;
        render_pane_header(
            ui,
            &palette,
            folder_name,
            &format!("{} images  -  {} other files", img_count, other_count),
            &mut self.right_thumb_size,
            80.0..=500.0,
        );
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

        let thumb_size = self.right_thumb_size;
        let mut open_preview: Option<usize> = None;
        let mut open_external: Option<PathBuf> = None;
        let mut start_rename: Option<(PathBuf, String)> = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut context_command: Option<FileCommand> = None;

        let renaming_path = self.renaming.as_ref().map(|r| r.original_path.clone());

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                if img_count > 0 {
                    render_section_label(ui, &palette, "Images", img_count);
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);

                    for (fi, file) in self.right_files.iter().enumerate() {
                        let tile_w = thumb_size.clamp(120.0, 420.0);
                        let info_h = 48.0;
                        let tile_h = tile_w + info_h;
                        let (rect, tile_resp) = ui
                            .allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::click());

                        if !ui.is_rect_visible(rect) {
                            if file.is_image {
                                self.thumb_mgr
                                    .request_thumbnail(&file.path, thumb_size as u32);
                            }
                            continue;
                        }

                        let is_selected = self.selected_file == Some(fi);
                        paint_tile_shell(ui, rect, is_selected, tile_resp.hovered(), &palette);

                        let t_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(tile_w, tile_w));
                        let thumb_resp =
                            ui.interact(t_rect, ui.id().with(("rt", fi)), egui::Sense::click());

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

                            if thumb_resp.clicked() || tile_resp.clicked() {
                                self.selected_file = Some(fi);
                            }
                            if thumb_resp.double_clicked() || tile_resp.double_clicked() {
                                if let Some(img_idx) =
                                    self.right_image_paths.iter().position(|p| p == &file.path)
                                {
                                    open_preview = Some(img_idx);
                                }
                            }
                        } else {
                            paint_file_placeholder(ui, t_rect.shrink(1.0), file, &palette);
                            if thumb_resp.clicked() || tile_resp.clicked() {
                                self.selected_file = Some(fi);
                            }
                            if thumb_resp.double_clicked() || tile_resp.double_clicked() {
                                open_external = Some(file.path.clone());
                            }
                        }

                        if tile_resp.secondary_clicked() || thumb_resp.secondary_clicked() {
                            self.selected_file = Some(fi);
                        }
                        tile_resp.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                context_command = Some(FileCommand::Open);
                                ui.close_menu();
                            }
                            if ui.button("Copy").clicked() {
                                context_command = Some(FileCommand::Copy);
                                ui.close_menu();
                            }
                            if ui.button("Cut").clicked() {
                                context_command = Some(FileCommand::Cut);
                                ui.close_menu();
                            }
                            if ui.button("Paste").clicked() {
                                context_command = Some(FileCommand::Paste);
                                ui.close_menu();
                            }
                            if ui.button("Rename").clicked() {
                                context_command = Some(FileCommand::Rename);
                                ui.close_menu();
                            }
                            if ui.button("Delete").clicked() {
                                context_command = Some(FileCommand::Delete);
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
                            let name_resp = ui.interact(
                                name_rect,
                                ui.id().with(("rn", fi)),
                                egui::Sense::click(),
                            );
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                truncate_str(&file.name, 30),
                                egui::FontId::proportional(12.5),
                                if name_resp.hovered() {
                                    palette.accent
                                } else {
                                    palette.text_primary
                                },
                            );
                            if name_resp.clicked() {
                                start_rename = Some((file.path.clone(), file.name.clone()));
                            }
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

        // Start rename
        if let Some((path, name)) = start_rename {
            self.renaming = Some(RenameState {
                original_path: path,
                new_name: name,
            });
        }
        if commit_rename {
            self.execute_rename();
        } else if cancel_rename {
            self.renaming = None;
        }
        if let Some(command) = context_command {
            self.execute_file_command(command);
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
    HardDrive,
    Loader,
    Doc,
    Video,
    File,
    Archive,
}

fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

fn apply_theme(ctx: &egui::Context, theme: &str) {
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
    }
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

fn shortcut_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_height(30.0);
        ui.add_sized([112.0, 22.0], egui::Label::new(label));
        changed = ui
            .add_sized(
                [180.0, 24.0],
                egui::TextEdit::singleline(value)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(180.0),
            )
            .changed();
    });
    changed
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
        "enter" | "return" => Some(egui::Key::Enter),
        "delete" | "del" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "escape" | "esc" => Some(egui::Key::Escape),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
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
