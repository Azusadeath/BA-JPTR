use crate::{Control, Error, desktop::Project};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
};
struct Request {
    id: u64,
    action: String,
    payload: Value,
    token: Arc<AtomicBool>,
}
struct Completed {
    id: u64,
    revision: u64,
    before: Value,
    after: Value,
}
pub struct Worker {
    input: Sender<Request>,
    pub events: Receiver<Value>,
    tokens: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    thread: Option<JoinHandle<()>>,
}
pub fn cancellable(action: &str) -> bool {
    read_only(action)
        || matches!(
            action,
            "load"
                | "sources"
                | "import_commit"
                | "export_rules"
                | "export_text"
                | "baseline_commit"
                | "baseline_rebuild_commit"
                | "migration_commit"
                | "migration_start"
                | "restore"
                | "apply"
        )
}
pub fn read_only(action: &str) -> bool {
    matches!(
        action,
        "check"
            | "baseline_rebuild_preview"
            | "search"
            | "search_more"
            | "preview"
            | "prepare_patch"
            | "tables"
            | "scope_sources"
            | "baseline"
            | "batch_open"
            | "migration_preview"
            | "text_open"
            | "batch_preview"
            | "import_read"
            | "template"
    )
}
impl Worker {
    pub fn start(data: PathBuf, notify: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        crate::storage::validate_data_dir(&data)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        let (input, jobs) = mpsc::channel::<Request>();
        let (output, events) = mpsc::channel();
        let tokens = Arc::new(Mutex::new(HashMap::new()));
        let worker_tokens = tokens.clone();
        let thread=std::thread::Builder::new().name("bajptr-project".into()).spawn(move||{
            let send=|value:Value|{let _=output.send(value);notify();};
            let mut active = None;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut project=Project::new(data);let mut completed=VecDeque::<Completed>::new();
            while let Ok(request)=jobs.recv(){
                active = Some((request.id, request.action.clone()));
                if request.action=="quit"{break;}
                if request.action=="accept"{let target=request.payload["target"].as_u64();completed.retain(|c|Some(c.id)!=target);continue;}
                if request.action=="cancel"{
                    let target=request.payload["target"].as_u64();
                    let positions=completed.iter().enumerate().filter(|(_,c)|target.is_none_or(|t|c.id==t)).map(|(i,_)|i).collect::<Vec<_>>();
                    for index in positions.into_iter().rev(){
                        let rejected=completed.remove(index).unwrap();if rejected.revision!=project.revision{continue;}
                        let mut current=project.snapshot();
                        for (key,before) in rejected.before.as_object().unwrap(){let after=&rejected.after[key];if before==after{continue;}
                            if &current[key]==after{current[key]=before.clone();}
                            for later in &mut completed{if later.revision==project.revision{for snapshot in [&mut later.before,&mut later.after]{if &snapshot[key]==after{snapshot[key]=before.clone();}}}}
                        }
                        project.restore_snapshot(&current);
                    }continue;
                }
                let read=read_only(&request.action);let can_cancel=cancellable(&request.action);let before=project.snapshot();let revision=project.revision;
                let callback=|_:Option<&str>| !can_cancel || !request.token.load(Ordering::Relaxed);
                let logger=|level:crate::LogLevel,message:&str|send(json!({"id":request.id,"event":"progress","level":level,"message":message}));
                let control=Control::new(&callback).with_logger(&logger);
                let result=control.check().and_then(|_|project.dispatch(&request.action,&request.payload,&control));
                match result{
                    Ok(data)=>{if read{completed.push_back(Completed{id:request.id,revision,before,after:project.snapshot()});if completed.len()>32{completed.pop_front();}}send(json!({"id":request.id,"event":"result","data":data}));},
                    Err(Error::Cancelled)=>{project.restore_snapshot(&before);send(json!({"id":request.id,"event":"cancelled"}));},
                    Err(error)=>{if read{project.restore_snapshot(&before);}send(json!({"id":request.id,"event":"error","level":"error","message":crate::diagnostics::user_message(&request.action,&error),"detail":error.to_string()}));},
                }
                worker_tokens.lock().unwrap().remove(&request.id);
            }
            }));
            worker_tokens.lock().unwrap_or_else(|e|e.into_inner()).clear();
            let (message, detail) = match outcome {
                Ok(()) => ("项目服务已退出。", String::new()),
                Err(error) => {
                    let reason = error.downcast_ref::<String>().map(String::as_str)
                        .or_else(||error.downcast_ref::<&str>().copied()).unwrap_or("未知内部异常");
                    ("项目处理发生内部错误，项目服务已停止。", format!("请求 {active:?}: {reason}"))
                }
            };
            // Deliver shutdown independently of the callback that may have panicked.
            let _ = output.send(json!({"id":0,"event":"backend_closed","message":message,"detail":detail}));
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||notify()));
        })?;
        Ok(Self {
            input,
            events,
            tokens,
            thread: Some(thread),
        })
    }
    pub fn send(&self, id: u64, action: &str, payload: Value) -> Result<(), String> {
        self.client()(id, action, payload)
    }
    /// An input-only handle lets the JSON-lines CLI read requests concurrently.
    pub fn client(&self) -> impl Fn(u64, &str, Value) -> Result<(), String> + Send + 'static {
        let input = self.input.clone();
        let tokens = self.tokens.clone();
        move |id, action, payload| {
            let token = Arc::new(AtomicBool::new(false));
            if action == "cancel" || action == "quit" {
                let target = payload["target"].as_u64();
                for (id, t) in tokens.lock().unwrap().iter() {
                    if target.is_none_or(|v| v == *id) {
                        t.store(true, Ordering::Relaxed);
                    }
                }
            } else if cancellable(action) {
                tokens.lock().unwrap().insert(id, token.clone());
            }
            input
                .send(Request {
                    id,
                    action: action.into(),
                    payload,
                    token,
                })
                .map_err(|_| {
                    tokens.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                    "项目服务已退出，请重新连接。".into()
                })
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.send(0, "quit", json!({}));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicUsize, time::Duration};

    #[test]
    fn warning_metadata_does_not_turn_a_successful_save_into_an_error() {
        let data = tempfile::tempdir().unwrap();
        let worker = Worker::start(data.path().to_path_buf(), Arc::new(|| {})).unwrap();
        worker.send(1, "load", json!({})).unwrap();
        assert_eq!(
            worker.events.recv_timeout(Duration::from_secs(3)).unwrap()["event"],
            "result"
        );
        worker.send(2, "save_rule", json!({"revision":0,"index":null,"rule":{"old":"原词","new":"新词","mode":"sub","source":"-","table":"test"},"draft":{}})).unwrap();
        let warning = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(warning["event"], "progress");
        assert_eq!(warning["level"], "warning");
        assert!(
            warning["message"]
                .as_str()
                .unwrap()
                .contains("模板表单未保存")
        );
        let result = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(result["event"], "result");
        assert_eq!(result["data"]["rules"][0]["rule"]["old"], "原词");
        worker.send(3, "unknown-action", json!({})).unwrap();
        let error = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(error["event"], "error");
        assert_eq!(error["level"], "error");
    }

    #[test]
    fn queued_loading_import_and_export_requests_can_cancel_before_any_work() {
        let data = tempfile::tempdir().unwrap();
        let (ready, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        let notifications = AtomicUsize::new(0);
        let worker = Worker::start(
            data.path().to_path_buf(),
            Arc::new(move || {
                if notifications.fetch_add(1, Ordering::SeqCst) == 0 {
                    ready.send(()).unwrap();
                    resume
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
            }),
        )
        .unwrap();
        worker.send(1, "unknown", json!({})).unwrap();
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        let actions = [
            "load",
            "sources",
            "import_read",
            "import_commit",
            "export_rules",
            "export_text",
            "baseline_commit",
            "restore",
        ];
        for (i, action) in actions.iter().enumerate() {
            let id = 2 + i as u64;
            worker.send(id, action, json!({})).unwrap();
            worker.send(0, "cancel", json!({"target":id})).unwrap();
        }
        release.send(()).unwrap();
        for i in 0..actions.len() {
            let event = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(event["id"], 2 + i as u64);
            assert_eq!(event["event"], "cancelled", "{event}");
        }
        assert!(!data.path().join(".gui").exists());
        worker.send(20, "load", json!({})).unwrap();
        assert_eq!(
            worker.events.recv_timeout(Duration::from_secs(3)).unwrap()["event"],
            "result"
        );
    }

    #[test]
    fn panic_after_a_save_reports_shutdown_and_reloads_only_saved_data() {
        let data = tempfile::tempdir().unwrap();
        let notifications = AtomicUsize::new(0);
        let worker = Worker::start(
            data.path().to_path_buf(),
            Arc::new(move || {
                if notifications.fetch_add(1, Ordering::SeqCst) == 1 {
                    panic!("notification failure after saving");
                }
            }),
        )
        .unwrap();
        worker.send(1, "unknown-action", json!({})).unwrap();
        assert_eq!(
            worker.events.recv_timeout(Duration::from_secs(3)).unwrap()["event"],
            "error"
        );
        worker
            .send(
                2,
                "settings",
                json!({"revision":0,"game":"","skip_kana":false}),
            )
            .unwrap();
        // A request already queued at failure must not be replayed after recovery.
        let _ = worker.send(
            3,
            "settings",
            json!({"revision":1,"game":"","skip_kana":true}),
        );
        let saved = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(saved["id"], 2);
        assert_eq!(saved["event"], "result");
        let closed = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(closed["event"], "backend_closed");
        assert!(
            closed["detail"]
                .as_str()
                .unwrap()
                .contains("notification failure")
        );
        // Joining removes the race between the terminal event and channel teardown.
        let mut worker = worker;
        worker.thread.take().unwrap().join().unwrap();
        assert!(worker.send(4, "load", json!({})).is_err());
        assert!(worker.tokens.lock().unwrap().is_empty());
        let recovered = Worker::start(data.path().to_path_buf(), Arc::new(|| {})).unwrap();
        recovered.send(1, "load", json!({})).unwrap();
        let loaded = recovered
            .events
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(loaded["event"], "result");
        assert_eq!(loaded["data"]["skip_kana"], false);
    }
}
