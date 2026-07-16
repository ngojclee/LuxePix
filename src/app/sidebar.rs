use super::*;

impl LuxePixApp {
    pub(super) fn render_navigation_sidebar(&mut self, ui: &mut egui::Ui, force_compact: bool) {
        let palette = Palette::for_theme(&self.effective_theme);
        let collapsed = self.config.sidebar_collapsed || force_compact;
        let mut navigate_to = None;
        let mut remove_source = None;
        let mut toggle_favorite = None;

        ui.horizontal(|ui| {
            if !collapsed {
                ui.label(
                    egui::RichText::new("LIBRARY")
                        .size(11.0)
                        .strong()
                        .color(palette.text_tertiary),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let icon = if collapsed {
                    IconKind::ChevronRight
                } else {
                    IconKind::ChevronLeft
                };
                if icon_button(
                    ui,
                    icon,
                    None,
                    if force_compact {
                        "Sidebar expands when the window is wider"
                    } else if collapsed {
                        "Expand sidebar"
                    } else {
                        "Collapse sidebar"
                    },
                    false,
                    &palette,
                )
                .clicked()
                    && !force_compact
                {
                    self.config.sidebar_collapsed = !self.config.sidebar_collapsed;
                    let _ = self.config.save();
                }
            });
        });
        ui.add_space(8.0);

        let sources = self.config.sources.clone();
        if collapsed {
            for source in sources.iter().take(7) {
                let path = PathBuf::from(&source.path);
                let source_status = self.index.source_status(&source.path);
                let availability = source_status
                    .map(|status| {
                        if status.available {
                            "available"
                        } else {
                            "unavailable"
                        }
                    })
                    .unwrap_or("not indexed");
                if icon_button(
                    ui,
                    IconKind::Folder,
                    None,
                    &format!("{} - {} ({availability})", source.name, source.path),
                    self.current_path.as_ref() == Some(&path),
                    &palette,
                )
                .clicked()
                {
                    navigate_to = Some(path);
                }
            }
        } else {
            let favorites: Vec<_> = sources
                .iter()
                .filter(|source| source.favorite)
                .cloned()
                .collect();
            if !favorites.is_empty() {
                sidebar_section_label(ui, "FAVORITES", &palette);
                for source in &favorites {
                    if source_row(
                        ui,
                        source,
                        self.current_path.as_deref(),
                        self.index.source_status(&source.path),
                        &palette,
                    ) {
                        navigate_to = Some(PathBuf::from(&source.path));
                    }
                }
                ui.add_space(12.0);
            }

            sidebar_section_label(ui, "SOURCES", &palette);
            if sources.is_empty() {
                ui.label(
                    egui::RichText::new("No sources configured")
                        .size(12.0)
                        .color(palette.text_tertiary),
                );
            } else {
                for source in &sources {
                    let response = source_row_response(
                        ui,
                        source,
                        self.current_path.as_deref(),
                        self.index.source_status(&source.path),
                        &palette,
                    );
                    if response.clicked() {
                        navigate_to = Some(PathBuf::from(&source.path));
                    }
                    response.context_menu(|ui| {
                        if ui
                            .button(if source.favorite {
                                "Remove from Favorites"
                            } else {
                                "Add to Favorites"
                            })
                            .clicked()
                        {
                            toggle_favorite = Some(source.path.clone());
                            ui.close_menu();
                        }
                        if ui.button("Remove Source").clicked() {
                            remove_source = Some(source.path.clone());
                            ui.close_menu();
                        }
                    });
                }
            }

            ui.add_space(6.0);
            if ui.button("Add Source").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.config.add_source(&path);
                    self.request_index_rebuild();
                    navigate_to = Some(path);
                }
            }

            ui.add_space(16.0);
            sidebar_section_label(ui, "RECENT", &palette);
            for path_value in self.config.path_history.iter().take(6) {
                let path = PathBuf::from(path_value);
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| elide_middle(path_value, 24));
                if ui
                    .selectable_label(
                        self.current_path.as_ref() == Some(&path),
                        egui::RichText::new(label)
                            .size(12.0)
                            .color(palette.text_secondary),
                    )
                    .on_hover_text(path_value)
                    .clicked()
                {
                    navigate_to = Some(path);
                }
            }
        }

        if let Some(path) = toggle_favorite {
            self.config.toggle_source_favorite(&path);
        }
        if let Some(path) = remove_source {
            self.config.remove_source(&path);
            self.request_index_rebuild();
        }
        if let Some(path) = navigate_to {
            self.navigate_to(path);
        }
    }
}

fn sidebar_section_label(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .size(10.5)
            .strong()
            .color(palette.text_tertiary),
    );
    ui.add_space(4.0);
}

fn source_row(
    ui: &mut egui::Ui,
    source: &crate::config::SourceConfig,
    current: Option<&Path>,
    status: Option<&crate::index::SourceIndexStatus>,
    palette: &Palette,
) -> bool {
    source_row_response(ui, source, current, status, palette).clicked()
}

fn source_row_response(
    ui: &mut egui::Ui,
    source: &crate::config::SourceConfig,
    current: Option<&Path>,
    status: Option<&crate::index::SourceIndexStatus>,
    palette: &Palette,
) -> egui::Response {
    let path = Path::new(&source.path);
    let selected = current == Some(path);
    let suffix = match status {
        Some(status) if status.available => "  [ready]",
        Some(_) => "  [offline]",
        None => "",
    };
    let response = ui.selectable_label(
        selected,
        egui::RichText::new(format!("{}{}", truncate_str(&source.name, 17), suffix))
            .size(12.5)
            .color(if selected {
                palette.accent
            } else if status.is_some_and(|status| !status.available) {
                palette.error
            } else {
                palette.text_primary
            }),
    );
    response.on_hover_text(
        status
            .map(|status| {
                format!(
                    "{}\n{}\n{}",
                    status.name,
                    status.path.display(),
                    status.last_error.as_deref().unwrap_or(if status.available {
                        "Available"
                    } else {
                        "Unavailable"
                    })
                )
            })
            .unwrap_or_else(|| source.path.clone()),
    )
}
