use super::*;

impl LuxePixApp {
    pub(super) fn render_explorer_toolbar(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        #[derive(Clone, Copy)]
        enum ToolAction {
            BatchRename,
            FindDuplicates,
            CreateZip,
            ExtractZip,
            FolderSizes,
        }

        let mut view_changed = false;
        let mut sort_changed = false;
        let mut hidden_changed = false;
        let mut tool_action = None;
        let selected_paths = self.selected_paths();
        let has_selection = !selected_paths.is_empty();
        let can_extract_zip = selected_paths.iter().any(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        });
        let power_tool_idle = self.power_tools.active_count() == 0;

        egui::Frame::none()
            .fill(palette.surface)
            .stroke(egui::Stroke::new(1.0, palette.border_soft))
            .inner_margin(egui::Margin::symmetric(14.0, 7.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for mode in ViewMode::ALL {
                        view_changed |= ui
                            .selectable_value(&mut self.config.file_view_mode, mode, mode.label())
                            .changed();
                    }

                    ui.add_space(8.0);
                    ui.menu_button(format!("Sort: {}", self.config.sort_field.label()), |ui| {
                        for field in SortField::ALL {
                            if ui
                                .selectable_value(&mut self.config.sort_field, field, field.label())
                                .changed()
                            {
                                sort_changed = true;
                                ui.close_menu();
                            }
                        }
                    });
                    sort_changed |= ui
                        .selectable_value(
                            &mut self.config.sort_direction,
                            SortDirection::Ascending,
                            "Ascending",
                        )
                        .on_hover_text("Sort in ascending order")
                        .changed();
                    sort_changed |= ui
                        .selectable_value(
                            &mut self.config.sort_direction,
                            SortDirection::Descending,
                            "Descending",
                        )
                        .on_hover_text("Sort in descending order")
                        .changed();

                    ui.menu_button("Tools", |ui| {
                        if ui
                            .add_enabled(
                                power_tool_idle && has_selection,
                                egui::Button::new("Batch Rename..."),
                            )
                            .clicked()
                        {
                            tool_action = Some(ToolAction::BatchRename);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                power_tool_idle && self.current_path.is_some(),
                                egui::Button::new("Find Duplicates..."),
                            )
                            .clicked()
                        {
                            tool_action = Some(ToolAction::FindDuplicates);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                power_tool_idle && has_selection,
                                egui::Button::new("Create ZIP..."),
                            )
                            .clicked()
                        {
                            tool_action = Some(ToolAction::CreateZip);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                power_tool_idle && (can_extract_zip || !has_selection),
                                egui::Button::new("Extract ZIP..."),
                            )
                            .clicked()
                        {
                            tool_action = Some(ToolAction::ExtractZip);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                power_tool_idle && self.current_path.is_some(),
                                egui::Button::new("Folder Sizes..."),
                            )
                            .clicked()
                        {
                            tool_action = Some(ToolAction::FolderSizes);
                            ui.close_menu();
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        hidden_changed |= ui
                            .checkbox(&mut self.config.show_hidden_files, "Hidden")
                            .on_hover_text("Show hidden files; protected system files stay hidden")
                            .changed();
                        let selected_count = self.file_selection.len();
                        if selected_count > 0 {
                            ui.label(
                                egui::RichText::new(format!("{selected_count} selected"))
                                    .size(11.5)
                                    .color(palette.text_tertiary),
                            );
                        }
                    });
                });
            });

        match tool_action {
            Some(ToolAction::BatchRename) => self.open_batch_rename_tool(),
            Some(ToolAction::FindDuplicates) => self.start_duplicate_finder_tool(),
            Some(ToolAction::CreateZip) => self.create_zip_from_selection(),
            Some(ToolAction::ExtractZip) => self.extract_selected_zip(),
            Some(ToolAction::FolderSizes) => self.start_folder_size_tool(),
            None => {}
        }

        if view_changed {
            self.right_scroll_offset = 0.0;
            self.file_marquee = None;
            let _ = self.config.save();
        }
        if sort_changed {
            self.apply_current_sort();
            let _ = self.config.save();
        }
        if hidden_changed {
            let _ = self.config.save();
            self.refresh_current_view();
        }
    }

    pub(super) fn render_alternate_file_view(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        match self.config.file_view_mode {
            ViewMode::Gallery => {}
            ViewMode::Grid => self.render_grid_files(ui, palette),
            ViewMode::Compact => self.render_compact_files(ui, palette),
            ViewMode::Details => self.render_details_files(ui, palette),
        }
    }

    fn render_grid_files(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        let files = self.right_files.clone();
        let tile_width = self.right_thumb_size.clamp(112.0, 280.0);
        let spacing = 10.0;
        self.file_columns = ((ui.available_width() + spacing) / (tile_width + spacing))
            .floor()
            .max(1.0) as usize;

        let mut context_command = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut open_selected = false;
        let mut item_rects = Vec::with_capacity(files.len());
        let pending_scroll = self.pending_file_scroll_to.clone();
        let renaming_path = self
            .renaming
            .as_ref()
            .map(|rename| rename.original_path.clone());

        let output = egui::ScrollArea::vertical()
            .id_salt("file_grid")
            .vertical_scroll_offset(self.right_scroll_offset)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(spacing, spacing);
                    for file in &files {
                        let tile_height = tile_width + 44.0;
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(tile_width, tile_height),
                            egui::Sense::click_and_drag(),
                        );
                        item_rects.push((file.id.clone(), rect));
                        if pending_scroll.as_ref() == Some(&file.id) {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }

                        let selected = self.file_selection.contains(&file.id);
                        paint_tile_shell(ui, rect, selected, response.hovered(), palette);
                        let thumbnail_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(tile_width, tile_width));
                        self.paint_file_thumbnail(ui, file, thumbnail_rect.shrink(1.0), palette);

                        let checkbox_clicked = if self.config.show_selection_checkboxes {
                            selection_checkbox(
                                ui,
                                egui::Rect::from_min_size(
                                    rect.min + egui::vec2(7.0, 7.0),
                                    egui::vec2(19.0, 19.0),
                                ),
                                selected,
                                palette,
                                ("grid_check", &file.id),
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
                        } else if response.clicked() {
                            let modifiers = ui.input(|input| input.modifiers);
                            self.select_file_with_modifiers(file.id.clone(), modifiers);
                        }
                        if response.double_clicked() {
                            self.file_selection.replace(file.id.clone());
                            self.selected_file = Some(file.id.clone());
                            self.active_pane = ExplorerPane::Files;
                            open_selected = true;
                        }
                        if response.drag_started() {
                            self.begin_file_drag(file);
                        }
                        if response.secondary_clicked() {
                            self.select_file_for_context(file.id.clone());
                        }
                        self.attach_item_context_menu(response.clone(), &mut context_command);

                        let name_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.min.x + 8.0, rect.min.y + tile_width + 5.0),
                            egui::vec2(tile_width - 16.0, 20.0),
                        );
                        if renaming_path.as_ref() == Some(&file.path) {
                            if let Some(rename) = &mut self.renaming {
                                let edit = ui.put(
                                    name_rect,
                                    egui::TextEdit::singleline(&mut rename.new_name)
                                        .font(egui::FontId::proportional(12.0))
                                        .desired_width(name_rect.width()),
                                );
                                edit.request_focus();
                                if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                    commit_rename = true;
                                } else if ui.input(|input| input.key_pressed(egui::Key::Escape))
                                    || edit.lost_focus()
                                {
                                    cancel_rename = true;
                                }
                            }
                        } else {
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                truncate_str(&file.name, 26),
                                egui::FontId::proportional(12.0),
                                palette.text_primary,
                            );
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.max.y - 10.0),
                            egui::Align2::LEFT_CENTER,
                            format_size(file.size_bytes),
                            egui::FontId::proportional(10.5),
                            palette.text_tertiary,
                        );
                    }
                });
                ui.add_space(16.0);
            });
        self.right_scroll_offset = output.state.offset.y;

        let finished_marquee = update_marquee_selection(
            ui,
            output.inner_rect,
            &item_rects,
            &mut self.file_selection,
            &mut self.file_marquee,
            palette,
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
        self.finish_file_view_actions(context_command, commit_rename, cancel_rename, open_selected);
    }

    fn render_compact_files(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        let files = self.right_files.clone();
        self.file_columns = 1;
        self.file_marquee = None;
        let mut context_command = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut open_selected = false;
        let mut item_rects = Vec::with_capacity(files.len());
        let pending_scroll = self.pending_file_scroll_to.clone();
        let renaming_path = self
            .renaming
            .as_ref()
            .map(|rename| rename.original_path.clone());

        let output = egui::ScrollArea::vertical()
            .id_salt("file_compact")
            .vertical_scroll_offset(self.right_scroll_offset)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                for file in &files {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 38.0),
                        egui::Sense::click_and_drag(),
                    );
                    item_rects.push((file.id.clone(), rect));
                    if pending_scroll.as_ref() == Some(&file.id) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    let selected = self.file_selection.contains(&file.id);
                    paint_list_row(ui, rect, selected, response.hovered(), palette);

                    let mut content_left = rect.left() + 10.0;
                    let checkbox_clicked = if self.config.show_selection_checkboxes {
                        let checkbox_rect = egui::Rect::from_center_size(
                            egui::pos2(content_left + 9.0, rect.center().y),
                            egui::vec2(18.0, 18.0),
                        );
                        content_left += 28.0;
                        selection_checkbox(
                            ui,
                            checkbox_rect,
                            selected,
                            palette,
                            ("compact_check", &file.id),
                        )
                        .clicked()
                    } else {
                        false
                    };

                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(content_left + 10.0, rect.center().y),
                        egui::vec2(20.0, 20.0),
                    );
                    paint_compact_file_icon(ui, file, icon_rect, palette);
                    let name_rect = egui::Rect::from_min_max(
                        egui::pos2(content_left + 28.0, rect.top() + 7.0),
                        egui::pos2(
                            (rect.right() - 150.0).max(content_left + 90.0),
                            rect.bottom() - 6.0,
                        ),
                    );

                    if renaming_path.as_ref() == Some(&file.path) {
                        if let Some(rename) = &mut self.renaming {
                            let edit = ui.put(
                                name_rect,
                                egui::TextEdit::singleline(&mut rename.new_name)
                                    .font(egui::FontId::proportional(12.5))
                                    .desired_width(name_rect.width()),
                            );
                            edit.request_focus();
                            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                commit_rename = true;
                            } else if ui.input(|input| input.key_pressed(egui::Key::Escape))
                                || edit.lost_focus()
                            {
                                cancel_rename = true;
                            }
                        }
                    } else {
                        ui.painter().text(
                            name_rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            &file.name,
                            egui::FontId::proportional(12.5),
                            palette.text_primary,
                        );
                    }
                    ui.painter().text(
                        rect.right_center() - egui::vec2(12.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        format_size(file.size_bytes),
                        egui::FontId::proportional(11.0),
                        palette.text_tertiary,
                    );

                    if checkbox_clicked {
                        self.select_file_with_modifiers(
                            file.id.clone(),
                            egui::Modifiers {
                                ctrl: true,
                                ..Default::default()
                            },
                        );
                    } else if response.clicked() {
                        let modifiers = ui.input(|input| input.modifiers);
                        self.select_file_with_modifiers(file.id.clone(), modifiers);
                    }
                    if response.double_clicked() {
                        self.file_selection.replace(file.id.clone());
                        self.selected_file = Some(file.id.clone());
                        self.active_pane = ExplorerPane::Files;
                        open_selected = true;
                    }
                    if response.drag_started() {
                        self.begin_file_drag(file);
                    }
                    if response.secondary_clicked() {
                        self.select_file_for_context(file.id.clone());
                    }
                    self.attach_item_context_menu(response, &mut context_command);
                }
                ui.add_space(12.0);
            });
        self.right_scroll_offset = output.state.offset.y;
        if blank_primary_clicked(ui, output.inner_rect, &item_rects) {
            self.active_pane = ExplorerPane::Files;
            self.file_selection.clear();
            self.selected_file = None;
        }
        if pending_scroll
            .as_ref()
            .is_some_and(|id| files.iter().any(|file| &file.id == id))
        {
            self.pending_file_scroll_to = None;
        }
        self.finish_file_view_actions(context_command, commit_rename, cancel_rename, open_selected);
    }

    fn render_details_files(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        let files = self.right_files.clone();
        self.file_columns = 1;
        self.file_marquee = None;
        let width = ui.available_width();
        let name_width = (width * 0.46).max(190.0);
        let modified_width = (width * 0.22).max(110.0);
        let type_width = (width * 0.14).max(80.0);
        let size_width = (width - name_width - modified_width - type_width - 26.0).max(90.0);

        render_details_header(
            ui,
            palette,
            name_width,
            modified_width,
            type_width,
            size_width,
        );

        let mut context_command = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        let mut open_selected = false;
        let mut item_rects = Vec::with_capacity(files.len());
        let pending_scroll = self.pending_file_scroll_to.clone();
        let renaming_path = self
            .renaming
            .as_ref()
            .map(|rename| rename.original_path.clone());

        let output = egui::ScrollArea::vertical()
            .id_salt("file_details")
            .vertical_scroll_offset(self.right_scroll_offset)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for file in &files {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 36.0),
                        egui::Sense::click_and_drag(),
                    );
                    item_rects.push((file.id.clone(), rect));
                    if pending_scroll.as_ref() == Some(&file.id) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    let selected = self.file_selection.contains(&file.id);
                    paint_list_row(ui, rect, selected, response.hovered(), palette);
                    ui.painter().line_segment(
                        [rect.left_bottom(), rect.right_bottom()],
                        egui::Stroke::new(1.0, palette.border_soft),
                    );

                    let mut left = rect.left() + 8.0;
                    let checkbox_clicked = if self.config.show_selection_checkboxes {
                        let checkbox_rect = egui::Rect::from_center_size(
                            egui::pos2(left + 9.0, rect.center().y),
                            egui::vec2(18.0, 18.0),
                        );
                        left += 26.0;
                        selection_checkbox(
                            ui,
                            checkbox_rect,
                            selected,
                            palette,
                            ("details_check", &file.id),
                        )
                        .clicked()
                    } else {
                        false
                    };

                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(left + 9.0, rect.center().y),
                        egui::vec2(18.0, 18.0),
                    );
                    paint_compact_file_icon(ui, file, icon_rect, palette);
                    let name_rect = egui::Rect::from_min_max(
                        egui::pos2(left + 24.0, rect.top() + 6.0),
                        egui::pos2(rect.left() + name_width - 8.0, rect.bottom() - 5.0),
                    );
                    if renaming_path.as_ref() == Some(&file.path) {
                        if let Some(rename) = &mut self.renaming {
                            let edit = ui.put(
                                name_rect,
                                egui::TextEdit::singleline(&mut rename.new_name)
                                    .font(egui::FontId::proportional(12.0))
                                    .desired_width(name_rect.width()),
                            );
                            edit.request_focus();
                            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                commit_rename = true;
                            } else if ui.input(|input| input.key_pressed(egui::Key::Escape))
                                || edit.lost_focus()
                            {
                                cancel_rename = true;
                            }
                        }
                    } else {
                        ui.painter().text(
                            name_rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            truncate_str(&file.name, 54),
                            egui::FontId::proportional(12.0),
                            palette.text_primary,
                        );
                    }

                    let modified_x = rect.left() + name_width;
                    ui.painter().text(
                        egui::pos2(modified_x + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format_modified(file.modified_unix),
                        egui::FontId::proportional(11.0),
                        palette.text_secondary,
                    );
                    let type_x = modified_x + modified_width;
                    ui.painter().text(
                        egui::pos2(type_x + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        file_type_name(file),
                        egui::FontId::proportional(11.0),
                        palette.text_secondary,
                    );
                    ui.painter().text(
                        egui::pos2(type_x + type_width + size_width - 8.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_size(file.size_bytes),
                        egui::FontId::proportional(11.0),
                        palette.text_secondary,
                    );

                    if checkbox_clicked {
                        self.select_file_with_modifiers(
                            file.id.clone(),
                            egui::Modifiers {
                                ctrl: true,
                                ..Default::default()
                            },
                        );
                    } else if response.clicked() {
                        let modifiers = ui.input(|input| input.modifiers);
                        self.select_file_with_modifiers(file.id.clone(), modifiers);
                    }
                    if response.double_clicked() {
                        self.file_selection.replace(file.id.clone());
                        self.selected_file = Some(file.id.clone());
                        self.active_pane = ExplorerPane::Files;
                        open_selected = true;
                    }
                    if response.drag_started() {
                        self.begin_file_drag(file);
                    }
                    if response.secondary_clicked() {
                        self.select_file_for_context(file.id.clone());
                    }
                    self.attach_item_context_menu(response, &mut context_command);
                }
            });
        self.right_scroll_offset = output.state.offset.y;
        if blank_primary_clicked(ui, output.inner_rect, &item_rects) {
            self.active_pane = ExplorerPane::Files;
            self.file_selection.clear();
            self.selected_file = None;
        }
        if pending_scroll
            .as_ref()
            .is_some_and(|id| files.iter().any(|file| &file.id == id))
        {
            self.pending_file_scroll_to = None;
        }
        self.finish_file_view_actions(context_command, commit_rename, cancel_rename, open_selected);
    }

    fn paint_file_thumbnail(
        &mut self,
        ui: &egui::Ui,
        file: &FileEntry,
        rect: egui::Rect,
        palette: &Palette,
    ) {
        if file.is_image {
            self.thumb_mgr
                .request_thumbnail(&file.path, rect.width().max(rect.height()) as u32);
            if let Some(texture) = self.thumb_mgr.get(&file.path) {
                paint_thumbnail(ui, texture, rect, palette);
            } else {
                paint_placeholder(ui, rect, IconKind::Loader, "Loading", palette);
            }
        } else {
            paint_file_placeholder(ui, rect, file, palette);
        }
    }

    fn attach_item_context_menu(
        &mut self,
        response: egui::Response,
        command: &mut Option<CommandId>,
    ) {
        let context = self.command_context();
        let shortcuts = self.config.shortcuts.clone();
        response.context_menu(|ui| {
            for item in [
                CommandId::Open,
                CommandId::OpenWith,
                CommandId::RevealInExplorer,
            ] {
                if context_menu_command(ui, item, context, shortcuts.get(item)) {
                    *command = Some(item);
                    ui.close_menu();
                }
            }
            ui.separator();
            for item in [CommandId::Copy, CommandId::Cut, CommandId::Paste] {
                if context_menu_command(ui, item, context, shortcuts.get(item)) {
                    *command = Some(item);
                    ui.close_menu();
                }
            }
            ui.separator();
            for item in [CommandId::Rename, CommandId::Delete] {
                if context_menu_command(ui, item, context, shortcuts.get(item)) {
                    *command = Some(item);
                    ui.close_menu();
                }
            }
            ui.menu_button("More", |ui| {
                if context_menu_command(
                    ui,
                    CommandId::DeletePermanent,
                    context,
                    shortcuts.get(CommandId::DeletePermanent),
                ) {
                    *command = Some(CommandId::DeletePermanent);
                    ui.close_menu();
                }
            });
        });
    }

    fn finish_file_view_actions(
        &mut self,
        context_command: Option<CommandId>,
        commit_rename: bool,
        cancel_rename: bool,
        open_selected: bool,
    ) {
        if open_selected {
            self.open_selected_item();
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
}

pub(super) fn update_marquee_selection(
    ui: &egui::Ui,
    viewport: egui::Rect,
    item_rects: &[(ItemId, egui::Rect)],
    selection: &mut SelectionModel,
    state: &mut Option<MarqueeState>,
    palette: &Palette,
) -> bool {
    let (pressed, released, primary_down, press_origin, pointer_position, ctrl) =
        ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_released(),
                input.pointer.primary_down(),
                input.pointer.press_origin(),
                input.pointer.interact_pos(),
                input.modifiers.ctrl,
            )
        });

    if pressed {
        if let Some(origin) = press_origin {
            let over_item = item_rects.iter().any(|(_, rect)| rect.contains(origin));
            if viewport.contains(origin) && !over_item {
                *state = Some(MarqueeState {
                    start: origin,
                    current: origin,
                    base: if ctrl {
                        selection.clone()
                    } else {
                        SelectionModel::default()
                    },
                });
            }
        }
    }

    if let Some(marquee) = state.as_mut() {
        if let Some(position) = pointer_position {
            marquee.current = position;
        }
        *selection = marquee.base.clone();
        let selection_rect = egui::Rect::from_two_pos(marquee.start, marquee.current);
        selection.add_many(
            item_rects
                .iter()
                .filter(|(_, rect)| selection_rect.intersects(*rect))
                .map(|(id, _)| id.clone()),
        );
        ui.painter()
            .rect_filled(selection_rect, 2.0, palette.accent_soft);
        ui.painter()
            .rect_stroke(selection_rect, 2.0, egui::Stroke::new(1.0, palette.accent));
    }

    if released || (!primary_down && state.is_some()) {
        let finished = state.is_some();
        *state = None;
        finished
    } else {
        false
    }
}

pub(super) fn selection_checkbox(
    ui: &egui::Ui,
    rect: egui::Rect,
    selected: bool,
    palette: &Palette,
    id_salt: impl std::hash::Hash,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(id_salt), egui::Sense::click());
    let fill = if selected {
        palette.accent
    } else if response.hovered() {
        palette.surface
    } else {
        palette.surface_inset
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(
            1.0,
            if selected {
                palette.accent
            } else {
                palette.border_standard
            },
        ),
    );
    if selected {
        ui.painter().add(egui::Shape::line(
            vec![
                rect.left_center() + egui::vec2(4.0, 0.0),
                rect.center() + egui::vec2(-1.0, 4.0),
                rect.right_center() + egui::vec2(-3.0, -4.0),
            ],
            egui::Stroke::new(1.7, egui::Color32::WHITE),
        ));
    }
    response
}

fn blank_primary_clicked(
    ui: &egui::Ui,
    viewport: egui::Rect,
    item_rects: &[(ItemId, egui::Rect)],
) -> bool {
    ui.input(|input| {
        input.pointer.primary_pressed()
            && input.pointer.press_origin().is_some_and(|origin| {
                viewport.contains(origin)
                    && !item_rects.iter().any(|(_, rect)| rect.contains(origin))
            })
    })
}

fn paint_list_row(
    ui: &egui::Ui,
    rect: egui::Rect,
    selected: bool,
    hovered: bool,
    palette: &Palette,
) {
    let fill = if selected {
        palette.accent_soft
    } else if hovered {
        palette.surface_inset
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
}

fn paint_compact_file_icon(ui: &egui::Ui, file: &FileEntry, rect: egui::Rect, palette: &Palette) {
    let icon = if file.is_image {
        IconKind::Image
    } else {
        file_icon_kind(file)
    };
    draw_icon(ui.painter(), icon, rect, palette.text_tertiary, 1.5);
}

fn file_icon_kind(file: &FileEntry) -> IconKind {
    match file.extension.as_str() {
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "webm" => IconKind::Video,
        "zip" | "rar" | "7z" | "tar" | "gz" => IconKind::Archive,
        "pdf" | "doc" | "docx" | "rtf" | "odt" | "txt" | "log" | "md" => IconKind::Doc,
        _ => IconKind::File,
    }
}

fn render_details_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    name_width: f32,
    modified_width: f32,
    type_width: f32,
    size_width: f32,
) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette.surface_inset);
    let columns = [
        ("Name", rect.left() + 10.0, egui::Align2::LEFT_CENTER),
        (
            "Modified",
            rect.left() + name_width + 8.0,
            egui::Align2::LEFT_CENTER,
        ),
        (
            "Type",
            rect.left() + name_width + modified_width + 8.0,
            egui::Align2::LEFT_CENTER,
        ),
        (
            "Size",
            rect.left() + name_width + modified_width + type_width + size_width - 8.0,
            egui::Align2::RIGHT_CENTER,
        ),
    ];
    for (label, x, align) in columns {
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            align,
            label,
            egui::FontId::proportional(11.0),
            palette.text_tertiary,
        );
    }
}

fn file_type_name(file: &FileEntry) -> String {
    if file.is_image {
        if file.extension.is_empty() {
            "Image".into()
        } else {
            format!("{} image", file.extension.to_uppercase())
        }
    } else if file.extension.is_empty() {
        "File".into()
    } else {
        file.extension.to_uppercase()
    }
}

pub(super) fn format_modified(unix: u64) -> String {
    if unix == 0 {
        return "Unknown".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(unix);
    let age = now.saturating_sub(unix);
    match age {
        0..=59 => "Just now".into(),
        60..=3_599 => format!("{} min ago", age / 60),
        3_600..=86_399 => format!("{} hr ago", age / 3_600),
        86_400..=604_799 => format!("{} days ago", age / 86_400),
        _ => format!("{} days ago", age / 86_400),
    }
}
