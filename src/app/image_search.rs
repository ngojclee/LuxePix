use super::*;

impl LuxePixApp {
    pub(super) fn render_image_search_window(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let mut open = self.show_image_search;
        let mut pick_image = false;
        let mut paste_image = false;
        let mut reveal = None;
        let mut close_after_reveal = false;
        let mut result_count_changed = false;
        let results = self.similarity_results.results.clone();
        let ctrl_v = ctx.input(|input| {
            input.modifiers.ctrl
                && input.key_pressed(egui::Key::V)
                && !input.modifiers.alt
                && !input.modifiers.shift
        });

        egui::Window::new("Find Product by Image")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 68.0))
            .default_size(egui::vec2(720.0, 650.0))
            .min_width(580.0)
            .min_height(430.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Choose Image").clicked() {
                        pick_image = true;
                    }
                    if ui.button("Paste Image").clicked() {
                        paste_image = true;
                    }
                    ui.separator();
                    ui.label("Results");
                    result_count_changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.config.similarity_result_count)
                                .range(5..=50)
                                .speed(1),
                        )
                        .changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(if self.similarity_running {
                                "Comparing...".to_string()
                            } else if self.similarity_query_label.is_empty() {
                                "Ready".to_string()
                            } else {
                                format!(
                                    "{} - {:.1} ms",
                                    self.similarity_results.backend.label(),
                                    self.similarity_results.elapsed.as_secs_f64() * 1_000.0
                                )
                            })
                            .size(11.5)
                            .color(palette.text_tertiary),
                        );
                    });
                });

                ui.add_space(8.0);
                render_query_strip(ui, self, &palette);
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Model Pack: {}", self.model_pack_status))
                            .size(11.0)
                            .color(palette.text_tertiary),
                    );
                });
                if let Some(warning) = &self.similarity_results.fallback_warning {
                    ui.colored_label(palette.warning, warning);
                }
                ui.add_space(8.0);
                ui.separator();

                let arrow_down = ui.input(|input| input.key_pressed(egui::Key::ArrowDown));
                let arrow_up = ui.input(|input| input.key_pressed(egui::Key::ArrowUp));
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                if !results.is_empty() {
                    if arrow_down {
                        self.similarity_selected =
                            (self.similarity_selected + 1).min(results.len() - 1);
                    } else if arrow_up {
                        self.similarity_selected = self.similarity_selected.saturating_sub(1);
                    }
                    if enter {
                        reveal = results.get(self.similarity_selected).cloned();
                    }
                }

                egui::ScrollArea::vertical()
                    .id_salt("image_search_results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.similarity_query_label.is_empty() {
                            render_image_search_empty(
                                ui,
                                &palette,
                                "Choose, paste, or drop an image",
                                "The image stays on this computer.",
                            );
                            return;
                        }
                        if self.similarity_running {
                            render_image_search_empty(
                                ui,
                                &palette,
                                "Comparing indexed images",
                                "You can keep browsing while this runs.",
                            );
                            return;
                        }
                        if results.is_empty() {
                            render_image_search_empty(
                                ui,
                                &palette,
                                "No indexed image matches are available",
                                "Rebuild the local index after adding a product source.",
                            );
                            return;
                        }

                        ui.add_space(7.0);
                        for (index, result) in results.iter().enumerate() {
                            let response = self.render_similarity_result_row(
                                ui,
                                result,
                                self.similarity_selected == index,
                                &palette,
                            );
                            if response.clicked() {
                                self.similarity_selected = index;
                                reveal = Some(result.clone());
                            }
                            if response.double_clicked() {
                                self.similarity_selected = index;
                                reveal = Some(result.clone());
                                close_after_reveal = true;
                            }
                        }
                        ui.add_space(8.0);
                    });
            });

        if ctrl_v {
            paste_image = true;
        }
        if pick_image {
            self.pick_similarity_image();
        }
        if paste_image {
            self.paste_similarity_image();
        }
        if result_count_changed {
            if let Err(error) = self.config.save() {
                self.status = format!("Could not save Image Search settings: {error}");
            } else {
                self.rerun_similarity_query();
            }
        }
        if let Some(result) = reveal {
            self.reveal_similarity_result(&result);
        }
        if close_after_reveal {
            open = false;
        }
        self.show_image_search = open;
    }

    fn render_similarity_result_row(
        &mut self,
        ui: &mut egui::Ui,
        result: &SimilarityResult,
        selected: bool,
        palette: &Palette,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 82.0), egui::Sense::click());
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect,
                5.0,
                if selected {
                    palette.accent_soft
                } else {
                    palette.surface_inset
                },
            );
        }
        if selected {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                1.5,
                palette.accent,
            );
            response.scroll_to_me(Some(egui::Align::Center));
        }

        let thumbnail_rect =
            egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 8.0), egui::vec2(66.0, 66.0));
        self.thumb_mgr.request_thumbnail(&result.matched_image, 220);
        if let Some(texture) = self.thumb_mgr.get(&result.matched_image) {
            paint_thumbnail(ui, texture, thumbnail_rect, palette);
        } else {
            paint_placeholder(ui, thumbnail_rect, IconKind::Loader, "", palette);
        }

        let text_left = thumbnail_rect.right() + 12.0;
        let text_right = rect.right() - 12.0;
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 19.0),
            egui::Align2::LEFT_CENTER,
            truncate_str(&result.product_code, 46),
            egui::FontId::proportional(13.5),
            palette.text_primary,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 40.0),
            egui::Align2::LEFT_CENTER,
            truncate_str(&result.folder_name, 62),
            egui::FontId::proportional(12.0),
            palette.text_secondary,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 62.0),
            egui::Align2::LEFT_CENTER,
            format!(
                "{} - {}",
                truncate_str(&result.source_name, 28),
                truncate_str(
                    &result
                        .matched_image
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    36
                )
            ),
            egui::FontId::proportional(10.8),
            palette.text_tertiary,
        );

        let score_color = if result.low_confidence() {
            palette.warning
        } else {
            palette.accent
        };
        ui.painter().text(
            egui::pos2(text_right, rect.top() + 22.0),
            egui::Align2::RIGHT_CENTER,
            format!("{:.0}%", result.score * 100.0),
            egui::FontId::proportional(12.0),
            score_color,
        );
        if result.low_confidence() {
            ui.painter().text(
                egui::pos2(text_right, rect.top() + 43.0),
                egui::Align2::RIGHT_CENTER,
                "Low confidence",
                egui::FontId::proportional(10.5),
                palette.warning,
            );
        }

        response.on_hover_text(format!(
            "{}\nMatched image: {}\nSource: {}",
            result.folder_path.display(),
            result.matched_image.display(),
            result.source_path.display()
        ))
    }
}

fn render_query_strip(ui: &mut egui::Ui, app: &mut LuxePixApp, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 72.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 5.0, palette.surface_inset);
    let preview_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(56.0, 56.0));
    if let Some(path) = &app.similarity_query_path {
        app.thumb_mgr.request_thumbnail(path, 180);
        if let Some(texture) = app.thumb_mgr.get(path) {
            paint_thumbnail(ui, texture, preview_rect, palette);
        } else {
            paint_placeholder(ui, preview_rect, IconKind::Loader, "", palette);
        }
    } else {
        paint_placeholder(ui, preview_rect, IconKind::Image, "", palette);
    }
    let label = if app.similarity_query_label.is_empty() {
        "No query image selected"
    } else {
        app.similarity_query_label.as_str()
    };
    ui.painter().text(
        egui::pos2(preview_rect.right() + 12.0, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        truncate_str(label, 68),
        egui::FontId::proportional(13.0),
        palette.text_primary,
    );
    ui.painter().text(
        egui::pos2(preview_rect.right() + 12.0, rect.center().y + 12.0),
        egui::Align2::LEFT_CENTER,
        "Local processing only",
        egui::FontId::proportional(10.8),
        palette.text_tertiary,
    );
}

fn render_image_search_empty(ui: &mut egui::Ui, palette: &Palette, title: &str, detail: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 210.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(52.0);
            let icon_rect = ui
                .allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover())
                .0;
            draw_icon(
                ui.painter(),
                IconKind::Image,
                icon_rect.shrink(5.0),
                palette.text_tertiary,
                1.6,
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(title)
                    .size(13.0)
                    .color(palette.text_secondary),
            );
            ui.label(
                egui::RichText::new(detail)
                    .size(11.5)
                    .color(palette.text_tertiary),
            );
        },
    );
}
