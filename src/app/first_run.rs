use super::*;

impl LuxePixApp {
    pub(super) fn render_first_run(&mut self, ctx: &egui::Context) {
        if self.config.first_run_complete {
            return;
        }

        let palette = Palette::for_theme(&self.effective_theme);
        let mut next = false;
        let mut back = false;
        let mut finish = false;
        let mut skip = false;

        egui::Window::new("Welcome to LuxePix")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(520.0, 300.0))
            .show(ctx, |ui| match self.first_run_step {
                0 => {
                    ui.label(
                        egui::RichText::new("Product Source")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        let source_response = ui.add_sized(
                            [320.0, 28.0],
                            egui::TextEdit::singleline(&mut self.first_run_source)
                                .hint_text("Folder containing product folders"),
                        );
                        if source_response.changed() {
                            self.first_run_source_valid = false;
                        }
                        let verify = ui.button("Verify").clicked()
                            || (source_response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        if ui.button("Browse").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.first_run_source = path.to_string_lossy().to_string();
                                self.first_run_source_valid = true;
                            }
                        }
                        if verify {
                            self.first_run_source_valid =
                                Path::new(&self.first_run_source).is_dir();
                        }
                    });
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new(if self.first_run_source_valid {
                            "Source is ready."
                        } else if self.first_run_source.trim().is_empty() {
                            "Choose an accessible folder."
                        } else {
                            "Verify the source before continuing."
                        })
                        .size(12.0)
                        .color(if self.first_run_source_valid {
                            palette.success
                        } else {
                            palette.text_tertiary
                        }),
                    );
                    ui.add_space(90.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(self.first_run_source_valid, egui::Button::new("Next"))
                            .clicked()
                        {
                            next = true;
                        }
                        if ui.button("Skip for Now").clicked() {
                            skip = true;
                        }
                    });
                }
                _ => {
                    ui.label(
                        egui::RichText::new("Local Search Index")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                    ui.add_space(16.0);
                    ui.checkbox(
                        &mut self.config.indexing_enabled,
                        "Build and maintain a local index",
                    );
                    ui.add_space(18.0);
                    settings_path_row(ui, "Data root", &self.storage.root, &palette);
                    settings_path_row(ui, "Index", &self.storage.index_dir, &palette);
                    ui.add_space(90.0);
                    ui.horizontal(|ui| {
                        if ui.button("Back").clicked() {
                            back = true;
                        }
                        if ui.button("Finish").clicked() {
                            finish = true;
                        }
                    });
                }
            });

        if next {
            self.first_run_step = 1;
        }
        if back {
            self.first_run_step = 0;
        }
        if skip {
            self.config.first_run_complete = true;
            let _ = self.config.save();
            self.status = "Setup skipped. Add a source from the sidebar.".into();
        }
        if finish {
            let source = PathBuf::from(&self.first_run_source);
            if source.is_dir() {
                self.config.add_source(&source);
                self.config.first_run_complete = true;
                let _ = self.config.save();
                self.navigate_to(source);
                self.status = "LuxePix setup complete.".into();
            } else {
                self.first_run_source_valid = false;
                self.first_run_step = 0;
                self.status = "The selected source is no longer accessible.".into();
            }
        }
    }
}
