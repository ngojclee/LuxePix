use super::*;

impl LuxePixApp {
    pub(super) fn render_quick_find_window(&mut self, ctx: &egui::Context) {
        let palette = Palette::for_theme(&self.effective_theme);
        let mut open = self.show_quick_find;
        let mut query_changed = false;
        let mut scope_changed = false;
        let mut reveal = None;
        let mut close_after_reveal = false;
        let mut escape_requested = false;
        let groups = self.quick_find_results.groups.clone();
        let result_count = self.quick_find_results.result_count();

        egui::Window::new("Quick Find")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 68.0))
            .default_width(640.0)
            .min_width(520.0)
            .max_height(620.0)
            .show(ctx, |ui| {
                let search_response = ui.add_sized(
                    [ui.available_width(), 34.0],
                    egui::TextEdit::singleline(&mut self.quick_find_query)
                        .hint_text("Product code or folder name")
                        .font(egui::TextStyle::Body),
                );
                if self.quick_find_focus_requested {
                    search_response.request_focus();
                    self.quick_find_focus_requested = false;
                }
                query_changed |= search_response.changed();

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for scope in QuickFindScope::ALL {
                        scope_changed |= ui
                            .selectable_value(&mut self.quick_find_scope, scope, scope.label())
                            .changed();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let snapshot = self.index.snapshot();
                        let label = if self.quick_find_running {
                            "Searching...".to_string()
                        } else if self.quick_find_query.trim().is_empty() {
                            snapshot.state.label().to_string()
                        } else {
                            format!(
                                "{} result(s) in {:.1} ms",
                                result_count,
                                self.quick_find_results.elapsed.as_secs_f64() * 1_000.0
                            )
                        };
                        ui.label(
                            egui::RichText::new(label)
                                .size(11.5)
                                .color(palette.text_tertiary),
                        );
                    });
                });

                let arrow_down = ui.input(|input| input.key_pressed(egui::Key::ArrowDown));
                let arrow_up = ui.input(|input| input.key_pressed(egui::Key::ArrowUp));
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if escape {
                    escape_requested = true;
                }
                if result_count > 0 {
                    if arrow_down {
                        self.quick_find_selected =
                            (self.quick_find_selected + 1).min(result_count - 1);
                        reveal = self.quick_find_result_at(self.quick_find_selected).cloned();
                    } else if arrow_up {
                        self.quick_find_selected = self.quick_find_selected.saturating_sub(1);
                        reveal = self.quick_find_result_at(self.quick_find_selected).cloned();
                    }
                    if enter {
                        reveal = self.quick_find_result_at(self.quick_find_selected).cloned();
                        close_after_reveal = reveal.is_some();
                    }
                }

                ui.add_space(8.0);
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("quick_find_results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.quick_find_query.trim().is_empty() {
                            render_quick_find_empty(
                                ui,
                                &palette,
                                IconKind::Search,
                                "Enter a product code",
                                " ",
                            );
                            return;
                        }
                        if self.index.snapshot().state == IndexState::Disabled {
                            render_quick_find_empty(
                                ui,
                                &palette,
                                IconKind::Info,
                                "Search indexing is disabled",
                                "Enable it in Search & Index settings.",
                            );
                            return;
                        }
                        if groups.is_empty() {
                            if self.quick_find_running {
                                render_quick_find_empty(
                                    ui,
                                    &palette,
                                    IconKind::Loader,
                                    "Searching",
                                    " ",
                                );
                            } else {
                                render_quick_find_empty(
                                    ui,
                                    &palette,
                                    IconKind::Info,
                                    "No indexed product folder found",
                                    " ",
                                );
                            }
                            return;
                        }

                        let mut flat_index = 0usize;
                        for group in &groups {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(group.kind.label().to_ascii_uppercase())
                                    .size(10.5)
                                    .strong()
                                    .color(palette.text_tertiary),
                            );
                            ui.add_space(4.0);
                            for result in &group.results {
                                let selected = self.quick_find_selected == flat_index;
                                let response = self
                                    .render_quick_find_result_row(ui, result, selected, &palette);
                                if response.clicked() {
                                    self.quick_find_selected = flat_index;
                                    reveal = Some(result.clone());
                                }
                                if response.double_clicked() {
                                    self.quick_find_selected = flat_index;
                                    reveal = Some(result.clone());
                                    close_after_reveal = true;
                                }
                                flat_index += 1;
                            }
                        }
                        ui.add_space(8.0);
                    });
            });

        if query_changed || scope_changed {
            self.quick_find_selected = 0;
            self.queue_quick_find_search();
        }
        if let Some(result) = reveal {
            self.reveal_search_result(&result);
        }
        if close_after_reveal || escape_requested {
            open = false;
        }
        self.show_quick_find = open;
    }

    fn render_quick_find_result_row(
        &mut self,
        ui: &mut egui::Ui,
        result: &ProductSearchResult,
        selected: bool,
        palette: &Palette,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::click());
        let hovered = response.hovered();
        if selected || hovered {
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
            egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 8.0), egui::vec2(54.0, 54.0));
        if let Some(path) = &result.preview_path {
            self.thumb_mgr.request_thumbnail(path, 180);
            if let Some(texture) = self.thumb_mgr.get(path) {
                paint_thumbnail(ui, texture, thumbnail_rect, palette);
            } else {
                paint_placeholder(ui, thumbnail_rect, IconKind::Loader, "", palette);
            }
        } else {
            paint_placeholder(ui, thumbnail_rect, IconKind::Folder, "", palette);
        }

        let text_left = thumbnail_rect.right() + 12.0;
        let text_right = rect.right() - 12.0;
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 18.0),
            egui::Align2::LEFT_CENTER,
            truncate_str(&result.product_code, 44),
            egui::FontId::proportional(13.5),
            palette.text_primary,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 38.0),
            egui::Align2::LEFT_CENTER,
            truncate_str(&result.folder_name, 58),
            egui::FontId::proportional(12.0),
            palette.text_secondary,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 56.0),
            egui::Align2::LEFT_CENTER,
            format!(
                "{} - {} image(s)",
                truncate_str(&result.source_name, 30),
                result.image_count
            ),
            egui::FontId::proportional(10.8),
            palette.text_tertiary,
        );
        if result.group == crate::search::SearchGroupKind::Fuzzy {
            ui.painter().text(
                egui::pos2(text_right, rect.top() + 18.0),
                egui::Align2::RIGHT_CENTER,
                format!("{:.0}%", result.score * 100.0),
                egui::FontId::proportional(11.0),
                palette.warning,
            );
        }

        response.on_hover_text(format!(
            "{}\nSource: {}\n{} indexed image(s)",
            result.folder_path.display(),
            result.source_path.display(),
            result.image_paths.len()
        ))
    }

    fn quick_find_result_at(&self, index: usize) -> Option<&ProductSearchResult> {
        self.quick_find_results.flattened().nth(index)
    }
}

fn render_quick_find_empty(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: IconKind,
    title: &str,
    detail: &str,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 180.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(42.0);
            let icon_rect = ui
                .allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover())
                .0;
            draw_icon(
                ui.painter(),
                icon,
                icon_rect.shrink(4.0),
                palette.text_tertiary,
                1.6,
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(title)
                    .size(13.0)
                    .color(palette.text_secondary),
            );
            if !detail.trim().is_empty() {
                ui.label(
                    egui::RichText::new(detail)
                        .size(11.5)
                        .color(palette.text_tertiary),
                );
            }
        },
    );
}
