use bajptr_project::worker::Worker;
use serde_json::Value;
use std::{path::Path, sync::Arc};
pub struct Backend {
    worker: Worker,
    next: u64,
    ctx: eframe::egui::Context,
}
impl Backend {
    pub fn start(_project: &Path, data: &Path, ctx: eframe::egui::Context) -> Result<Self, String> {
        let repaint = ctx.clone();
        Worker::start(
            data.to_path_buf(),
            Arc::new(move || repaint.request_repaint()),
        )
        .map(|worker| Self {
            worker,
            next: 1,
            ctx,
        })
        .map_err(|error| {
            let message = if let Some(source) = error
                .get_ref()
                .and_then(|source| source.downcast_ref::<bajptr_project::Error>())
            {
                bajptr_project::diagnostics::user_message("load", source)
            } else {
                bajptr_project::diagnostics::user_message("load", &bajptr_project::Error::Io(error))
            };
            format!("无法启动项目服务：{message}")
        })
    }
    pub fn send(&mut self, action: &str, payload: Value) -> Result<u64, String> {
        let id = self.next;
        self.next += 1;
        self.worker.send(id, action, payload)?;
        Ok(id)
    }
    pub fn poll(&self) -> (Vec<Value>, bool) {
        let mut events = Vec::new();
        for _ in 0..200 {
            match self.worker.events.try_recv() {
                Ok(event) => events.push(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => return (events, false),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return (events, true),
            }
        }
        self.ctx.request_repaint();
        (events, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_poll_distinguishes_disconnection_from_an_idle_worker() {
        let data = std::env::temp_dir().join("bajptr-transport-test");
        let mut backend = Backend::start(&data, &data, Default::default()).unwrap();
        assert!(!backend.poll().1);
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(serde_json::json!({"id":7,"event":"progress"}))
            .unwrap();
        drop(sender);
        backend.worker.events = receiver;
        let (events, disconnected) = backend.poll();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], 7);
        assert!(disconnected);
    }
}
