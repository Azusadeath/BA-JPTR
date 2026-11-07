//! Opt-in benchmark markers. Normal launches do not create metrics files.
use eframe::egui;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

static STARTED: OnceLock<Instant> = OnceLock::new();
pub fn begin() {
    let _ = STARTED.set(Instant::now());
}

pub struct Probe {
    path: PathBuf,
    data: Value,
    ready: Option<Instant>,
}
impl Probe {
    pub fn from_environment() -> Option<Self> {
        std::env::var_os("BAJPTR_STARTUP_REPORT").map(|path| Self {
            path: path.into(),
            data: json!({}),
            ready: None,
        })
    }
    pub fn frame(&mut self, ctx: &egui::Context, loaded: bool, connected: bool, failed: bool) {
        let elapsed = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64();
        if self.data.get("first_ui_seconds").is_none() {
            self.data["first_ui_seconds"] = json!(elapsed);
        }
        if loaded && self.data.get("project_loaded_seconds").is_none() {
            self.data["project_loaded_seconds"] = json!(elapsed);
        }
        if connected && self.data.get("game_connected_seconds").is_none() {
            self.data["game_connected_seconds"] = json!(elapsed);
        }
        if self.ready.is_none() && (connected || failed || elapsed > 15.) {
            self.ready = Some(Instant::now());
        }
        if self
            .ready
            .is_some_and(|t| t.elapsed() > Duration::from_millis(100))
        {
            self.data["connected"] = json!(connected);
            self.data["project_loaded"] = json!(loaded);
            self.data["failed"] = json!(failed);
            if let Ok(bytes) = serde_json::to_vec_pretty(&self.data) {
                let _ = std::fs::write(&self.path, bytes);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}
