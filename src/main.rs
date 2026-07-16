#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod config;
mod file_ops;
mod model;
mod navigation;
mod operations;
mod scanner;
mod selection;
mod sorting;
mod storage;
mod tasks;
mod thumbs;
mod undo;
mod windows_shell;

use eframe::egui;

fn main() -> eframe::Result {
    // Load icon for window titlebar
    let icon_data = include_bytes!("../assets/icon.png");
    let icon_image = image::load_from_memory(icon_data)
        .expect("Failed to load icon")
        .to_rgba8();
    let (w, h) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width: w,
        height: h,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("LuxePix — Image Browser")
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };

    eframe::run_native(
        "LuxePix",
        options,
        Box::new(|cc| Ok(Box::new(app::LuxePixApp::new(cc)))),
    )
}
