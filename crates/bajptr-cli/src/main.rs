mod commands;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn main() {
    if let Some(code) = bajptr_project::cleanup::handle_mode() {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--engine-stdio") {
        kernel_stdio();
        return;
    }
    if args.get(1).is_some_and(|s| s == "--desktop-stdio") {
        desktop_stdio(&args);
        return;
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let _ = ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed));
    let callback = |message: Option<&str>| {
        if let Some(m) = message {
            eprintln!("{m}");
        }
        !cancelled.load(Ordering::Relaxed)
    };
    if let Err(error) = commands::run(&Control::new(&callback)) {
        eprintln!("错误：{error}");
        std::process::exit(if matches!(error, bajptr_core::Error::Cancelled) {
            130
        } else {
            1
        });
    }
}
fn desktop_stdio(args: &[String]) {
    let data = args
        .iter()
        .position(|s| s == "--data-dir")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let worker =
        bajptr_project::worker::Worker::start(data, Arc::new(|| {})).expect("启动项目服务失败");
    let send = worker.client();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(Result::ok) {
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(v) => {
                    let action = v["action"].as_str().unwrap_or("");
                    let _ = send(v["id"].as_u64().unwrap_or(0), action, v["payload"].clone());
                    if action == "quit" {
                        break;
                    }
                }
                Err(e) => eprintln!("{e}"),
            }
        }
        let _ = send(0, "quit", serde_json::json!({}));
    });
    while let Ok(value) = worker.events.recv() {
        println!("{value}");
        let _ = io::stdout().flush();
        if value["event"] == "backend_closed" {
            break;
        }
    }
}
use bajptr_core::{
    Control,
    protocol::{Session, envelope, execute},
};
use std::io::{self, BufRead, Write};

fn kernel_stdio() {
    let mut session = Session::default();
    let callback = |message: Option<&str>| {
        if let Some(message) = message {
            eprintln!("{message}");
        }
        true
    };
    let control = Control::new(&callback);
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(bajptr_core::Error::Io)
            .and_then(|line| Ok(serde_json::from_str(&line)?))
            .and_then(|value| execute(&mut session, value, &control));
        println!("{}", envelope(result));
        let _ = io::stdout().flush();
    }
}
