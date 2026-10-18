// Release builds are GUI apps: no console window behind the main window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
#[cfg(test)]
mod demo;
mod git;
mod github;
mod hub;
mod hub_ui;
mod images;
mod markdown;
mod model;
mod platform;
mod scanner;
mod store;
mod theme;
mod update;
mod util;
mod widgets;

const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        // packaging/GitDaddy.iss waits for a window with exactly this title to close during updates.
        .with_title("GitDaddy — Git Repository Update Manager")
        .with_app_id("GitDaddy")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([1000.0, 620.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(WINDOW_ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native("GitDaddy", options, Box::new(|cc| Ok(Box::new(app::GitDaddyApp::new(cc)))))
}
