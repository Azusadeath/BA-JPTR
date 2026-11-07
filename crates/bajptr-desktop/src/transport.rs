use bajptr_project::worker::Worker;
use serde_json::Value;
use std::{path::Path, sync::Arc};
pub struct Backend {
    worker: Worker,
    next: u64,
}
impl Backend {
    pub fn start(_project: &Path, data: &Path, ctx: eframe::egui::Context) -> Result<Self, String> {
        Worker::start(data.to_path_buf(), Arc::new(move || ctx.request_repaint()))
            .map(|worker| Self { worker, next: 1 })
            .map_err(|e| format!("无法启动项目服务：{e}"))
    }
    pub fn send(&mut self, action: &str, payload: Value) -> Result<u64, String> {
        let id = self.next;
        self.next += 1;
        self.worker.send(id, action, payload)?;
        Ok(id)
    }
    pub fn events(&self) -> &std::sync::mpsc::Receiver<Value> {
        &self.worker.events
    }
}
