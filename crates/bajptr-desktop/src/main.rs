#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod dialogs;
mod fonts;
mod logging;
mod migration_review;
mod migration_ui;
mod pages;
mod startup;
mod state;
mod theme;
mod transport;
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    if let Some(code) = bajptr_project::cleanup::handle_mode() {
        std::process::exit(code);
    }
    startup::begin();
    let args: Vec<_> = std::env::args_os().collect();
    let argument = |name: &str| {
        args.iter()
            .position(|v| v == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let project = argument("--project-dir")
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                exe.ancestors()
                    .find(|p| {
                        p.join("native/engine.json").is_file() || p.join("Cargo.toml").is_file()
                    })
                    .map(PathBuf::from)
            })
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let data = argument("--data-dir").unwrap_or_else(|| project.clone());
    // Optional layout QA size; ordinary launches keep the established default.
    let size = std::env::var("BAJPTR_GUI_SIZE")
        .ok()
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            let width: u16 = width.parse().ok()?;
            let height: u16 = height.parse().ok()?;
            (width >= 900 && height >= 620).then_some([f32::from(width), f32::from(height)])
        })
        .unwrap_or([1080., 680.]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size([900., 620.])
            .with_icon(theme::app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "蔚蓝档案 · 日服译名替换",
        options,
        Box::new(move |cc| Ok(Box::new(app::Desktop::new(cc, project, data)))),
    )
}
