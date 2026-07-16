use super::*;

impl LuxePixApp {
    pub(super) fn render_details_panel(&mut self, ui: &mut egui::Ui) {
        let palette = Palette::for_theme(&self.effective_theme);
        let selected_file = self.selected_file_entry().cloned();
        let selected_folder = self.selected_folder_entry().cloned();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Details")
                    .size(14.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, IconKind::Close, None, "Close details", false, &palette)
                    .clicked()
                {
                    self.config.details_panel_open = false;
                    let _ = self.config.save();
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(12.0);

        if let Some(file) = selected_file {
            self.render_file_details(ui, &file, &palette);
        } else if let Some(folder) = selected_folder {
            self.render_folder_details(ui, &folder, &palette);
        } else {
            render_small_empty(
                ui,
                &palette,
                IconKind::Info,
                "No selection",
                "Select a file or folder to inspect it.",
            );
        }
    }

    fn render_file_details(&mut self, ui: &mut egui::Ui, file: &FileEntry, palette: &Palette) {
        let preview_rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), 180.0),
                egui::Sense::hover(),
            )
            .0;
        if file.is_image {
            self.thumb_mgr.request_thumbnail(&file.path, 420);
            if let Some(texture) = self.thumb_mgr.get(&file.path) {
                paint_thumbnail(ui, texture, preview_rect, palette);
            } else {
                paint_placeholder(ui, preview_rect, IconKind::Loader, "Loading", palette);
            }
        } else {
            paint_file_placeholder(ui, preview_rect, file, palette);
        }

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(&file.name)
                .size(14.0)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(10.0);
        detail_row(ui, "Type", file_type_label(file), palette);
        detail_row(ui, "Size", &format_size(file.size_bytes), palette);
        if let Some([width, height]) = self.thumb_mgr.get_original_size(&file.path) {
            detail_row(ui, "Dimensions", &format!("{width} x {height}"), palette);
        }
        ui.add_space(10.0);
        detail_path(ui, &file.path, palette);
        ui.add_space(12.0);
        if ui.button("Open").clicked() {
            self.execute_command(CommandId::Open);
        }
    }

    fn render_folder_details(
        &mut self,
        ui: &mut egui::Ui,
        folder: &FolderEntry,
        palette: &Palette,
    ) {
        let preview_rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), 160.0),
                egui::Sense::hover(),
            )
            .0;
        if let Some(path) = &folder.first_image {
            self.thumb_mgr.request_thumbnail(path, 420);
            if let Some(texture) = self.thumb_mgr.get(path) {
                paint_thumbnail(ui, texture, preview_rect, palette);
            } else {
                paint_placeholder(ui, preview_rect, IconKind::Loader, "Loading", palette);
            }
        } else {
            paint_placeholder(ui, preview_rect, IconKind::Folder, "", palette);
        }

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(&folder.name)
                .size(14.0)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(10.0);
        detail_row(ui, "Images", &folder.image_count.to_string(), palette);
        detail_row(ui, "Files", &folder.total_files.to_string(), palette);
        ui.add_space(10.0);
        detail_path(ui, &folder.path, palette);
        ui.add_space(12.0);
        if ui.button("Open Folder").clicked() {
            self.execute_command(CommandId::Open);
        }
    }
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [86.0, 20.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .size(11.5)
                    .color(palette.text_tertiary),
            ),
        );
        ui.label(
            egui::RichText::new(value)
                .size(12.0)
                .color(palette.text_secondary),
        );
    });
}

fn detail_path(ui: &mut egui::Ui, path: &Path, palette: &Palette) {
    ui.label(
        egui::RichText::new("Path")
            .size(11.5)
            .color(palette.text_tertiary),
    );
    ui.add(
        egui::Label::new(
            egui::RichText::new(path.to_string_lossy())
                .size(11.5)
                .monospace()
                .color(palette.text_secondary),
        )
        .selectable(true)
        .wrap(),
    );
}

fn file_type_label(file: &FileEntry) -> &str {
    if file.is_image {
        "Image"
    } else if file.extension.is_empty() {
        "File"
    } else {
        &file.extension
    }
}
