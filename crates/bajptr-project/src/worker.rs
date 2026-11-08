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
fn cancellable(action: &str) -> bool {
    read_only(action) || action == "apply"
}
fn read_only(action: &str) -> bool {
    matches!(
        action,
        "check" | "search" | "preview" | "tables" | "baseline"
    )
}
impl Worker {
    pub fn start(data: PathBuf, notify: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        let (input, jobs) = mpsc::channel::<Request>();
        let (output, events) = mpsc::channel();
        let tokens = Arc::new(Mutex::new(HashMap::new()));
        let worker_tokens = tokens.clone();
        let thread=std::thread::Builder::new().name("bajptr-project".into()).spawn(move||{
            let mut project=Project::new(data);let mut completed=VecDeque::<Completed>::new();
            let send=|value:Value|{let _=output.send(value);notify();};
            while let Ok(request)=jobs.recv(){
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
                let callback=|message:Option<&str>|{if let Some(message)=message{send(json!({"id":request.id,"event":"progress","message":message}));} !can_cancel || !request.token.load(Ordering::Relaxed)};
                let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||project.dispatch(&request.action,&request.payload,&Control{callback:&callback})));
                match result{
                    Ok(Ok(data))=>{if read{completed.push_back(Completed{id:request.id,revision,before,after:project.snapshot()});if completed.len()>32{completed.pop_front();}}send(json!({"id":request.id,"event":"result","data":data}));},
                    Ok(Err(Error::Cancelled))=>{project.restore_snapshot(&before);send(json!({"id":request.id,"event":"cancelled"}));},
                    Ok(Err(error))=>{if read{project.restore_snapshot(&before);}send(json!({"id":request.id,"event":"error","message":error.to_string(),"detail":error.to_string()}));},
                    Err(_)=>{send(json!({"id":request.id,"event":"error","message":"项目处理发生内部错误。请重新打开工具，已保存的文件已保留。"}));break;}
                }
                worker_tokens.lock().unwrap().remove(&request.id);
            }
            send(json!({"id":0,"event":"backend_closed","message":"项目服务已退出。"}));
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
                .map_err(|_| "项目服务已退出，请重新打开工具。".into())
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
