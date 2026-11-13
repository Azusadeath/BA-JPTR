use crate::{
    Result, VerifyOptions,
    files::{self, Manifest},
    native, require,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

fn clean_command(executable: &Path, cwd: &Path, profile: &Path) -> Result<Command> {
    let windows = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable.")?;
    let windows = Path::new(&windows);
    for folder in ["Temp", "AppData/Local", "AppData/Roaming"] {
        fs::create_dir_all(profile.join(folder))?;
    }
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .creation_flags(0x08000000)
        .env_clear()
        .env("SystemRoot", windows)
        .env("WINDIR", windows)
        .env("ComSpec", windows.join("System32/cmd.exe"))
        .env("PATH", windows.join("System32"))
        .env("TEMP", profile.join("Temp"))
        .env("TMP", profile.join("Temp"))
        .env("USERPROFILE", profile)
        .env("LOCALAPPDATA", profile.join("AppData/Local"))
        .env("APPDATA", profile.join("AppData/Roaming"));
    Ok(command)
}

struct ChildGuard(Child);
impl ChildGuard {
    fn wait(&mut self, timeout: Duration) -> Result {
        let until = Instant::now() + timeout;
        loop {
            if let Some(status) = self.0.try_wait()? {
                return require(status.success(), format!("Child process failed: {status}"));
            }
            require(Instant::now() < until, "Child process timed out.")?;
            thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct Worker {
    child: ChildGuard,
    input: ChildStdin,
    output: Receiver<std::io::Result<String>>,
    stdout_thread: Option<thread::JoinHandle<()>>,
    stderr_thread: Option<thread::JoinHandle<std::io::Result<String>>>,
    sequence: u64,
}

impl Worker {
    fn start(mut command: Command, data: &Path, trash: &Path) -> Result<Self> {
        let mut child = ChildGuard(
            command
                .args([
                    std::ffi::OsStr::new("--desktop-stdio"),
                    std::ffi::OsStr::new("--data-dir"),
                    data.as_os_str(),
                ])
                .env("BAJPTR_TRASH_DIR", trash)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        let input = child.0.stdin.take().ok_or("Missing worker stdin.")?;
        let output = child.0.stdout.take().ok_or("Missing worker stdout.")?;
        let mut error = child.0.stderr.take().ok_or("Missing worker stderr.")?;
        let (sender, receiver) = mpsc::channel();
        let stdout_thread = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr_thread = thread::spawn(move || {
            let mut text = String::new();
            error.read_to_string(&mut text)?;
            Ok(text)
        });
        Ok(Self {
            child,
            input,
            output: receiver,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            sequence: 0,
        })
    }

    fn send(&mut self, value: &Value) -> Result {
        serde_json::to_writer(&mut self.input, value)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        Ok(())
    }

    fn invoke(&mut self, action: &str, payload: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        self.send(&json!({"id":id,"action":action,"payload":payload}))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            require(
                Instant::now() < deadline,
                format!("Worker {action} timed out."),
            )?;
            let line = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| format!("Worker {action} did not return a result: {e}"))??;
            let message: Value = serde_json::from_str(&line)?;
            if message["id"] == id && message["event"] != "progress" {
                require(
                    message["event"] == "result",
                    format!("Worker {action} failed: {message}"),
                )?;
                self.send(&json!({"id":0,"action":"accept","payload":{"target":id}}))?;
                return Ok(message["data"].clone());
            }
        }
    }

    fn stop(mut self) -> Result {
        self.send(&json!({"id":999,"action":"quit","payload":{}}))?;
        let result = self.child.wait(Duration::from_secs(20));
        if result.is_err() {
            let _ = self.child.0.kill();
            let _ = self.child.0.wait();
        }
        if let Some(handle) = self.stdout_thread.take() {
            handle.join().map_err(|_| "Stdout reader panicked.")?;
        }
        let errors = self
            .stderr_thread
            .take()
            .ok_or("Missing stderr reader.")?
            .join()
            .map_err(|_| "Stderr reader panicked.")??;
        result.map_err(|error| format!("{error}\n{errors}").into())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !matches!(self.child.0.try_wait(), Ok(Some(_))) {
            let _ = self.child.0.kill();
            let _ = self.child.0.wait();
        }
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take()
            && let Ok(Ok(text)) = handle.join()
            && !text.trim().is_empty()
        {
            eprintln!("Worker stderr: {text}");
        }
    }
}

fn cli(package: &Path) -> Result<std::path::PathBuf> {
    Ok(package
        .join("native")
        .join(Manifest::read(&package.join("native"))?.cli))
}

fn count(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}
fn healthy(value: &Value, rules: usize) -> Result {
    require(
        value["rules"].is_array()
            && count(&value["rules"]) == rules
            && value["errors"].as_object().is_some_and(|e| e.is_empty()),
        format!("Project did not load correctly: {value}"),
    )
}

pub fn portable(options: &VerifyOptions) -> Result {
    let package = fs::canonicalize(&options.portable)?;
    let work = std::path::absolute(&options.work_dir)?;
    require(!work.exists(), "Use a new portable test directory.")?;
    let copy = work.join("Portable copy");
    files::copy_tree(&package, &copy)?;
    require(
        !copy.join(".gui").exists(),
        "Distribution contains user settings.",
    )?;
    let foreign = work.join("Unrelated working directory");
    fs::create_dir_all(&foreign)?;
    let profile = work.join("Clean user");
    let startup = work.join("gui-startup.json");
    let mut command = clean_command(&copy.join("BA-JPTR.exe"), &foreign, &profile)?;
    command.env("BAJPTR_STARTUP_REPORT", &startup);
    let mut launcher = ChildGuard(command.spawn()?);
    let mut observed = None;
    let mut modules = Vec::new();
    let result = (|| -> Result<Value> {
        launcher.wait(Duration::from_secs(10))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if modules.is_empty()
                && let Ok(processes) = native::engines()
            {
                for process in processes.into_iter().filter(|p| {
                    p.path
                        .file_name()
                        .is_some_and(|n| n.eq_ignore_ascii_case("bajptr-desktop.exe"))
                        && native::inside(&p.path, &copy)
                }) {
                    if observed.is_none() {
                        observed = Some(native::ObservedProcess::open(process.pid)?);
                    }
                    if let Ok(found) = native::modules(process.pid) {
                        modules = found;
                    }
                }
            }
            if startup.exists() {
                break;
            }
            require(
                Instant::now() < deadline,
                "GUI startup probe did not complete.",
            )?;
            thread::sleep(Duration::from_millis(50));
        }
        let gui: Value = serde_json::from_slice(&fs::read(&startup)?)?;
        require(
            gui["project_loaded"] == true && gui["failed"] == false,
            format!("Empty GUI project failed: {gui}"),
        )?;
        require(!modules.is_empty(), "No loaded-module evidence captured.")?;
        observed
            .as_ref()
            .ok_or("No GUI process observed.")?
            .wait(10_000)?;
        Ok(gui)
    })();
    // Also collect a just-spawned probe after launcher errors, so failure paths stop only our isolated GUI.
    if result.is_err()
        && observed.is_none()
        && let Ok(processes) = native::engines()
    {
        for process in processes
            .into_iter()
            .filter(|p| native::inside(&p.path, &copy))
        {
            let _guard = native::ObservedProcess::open(process.pid)?;
        }
    }
    let gui = result?;
    let windows = std::env::var_os("SystemRoot").ok_or("SystemRoot unavailable.")?;
    let external: Vec<_> = modules
        .iter()
        .filter(|p| !native::inside(p, &copy) && !native::inside(p, Path::new(&windows)))
        .cloned()
        .collect();
    if !external.is_empty() {
        eprintln!("Warning: non-Windows host modules found; retest on a clean OS: {external:?}");
    }
    let mut worker = Worker::start(
        clean_command(&cli(&copy)?, &foreign, &profile)?,
        &copy,
        &work.join("retired"),
    )?;
    healthy(&worker.invoke("load", json!({}))?, 0)?;
    let saved = worker.invoke("save_rule", json!({"revision":0,"index":null,"rule":{"old":"Portable text","new":"Portable result","mode":"exact","source":"","table":""}}))?;
    require(count(&saved["rules"]) == 1, "First auto-save failed.")?;
    let export = work.join("export.tsv");
    worker.invoke("export_rules", json!({"path":export}))?;
    require(export.is_file(), "Portable export failed.")?;
    worker.stop()?;
    let report = json!({"passed":true,"test_kind":"isolated process on current Windows, not a new OS image","environment_cleared":true,"fresh_project":true,"foreign_working_directory":true,"separate_user_directories":true,"launcher_passed":true,"gui":gui,"first_save_passed":true,"export_passed":true,"loaded_modules":modules,"module_audit_clean":external.is_empty(),"external_non_windows_modules":external,"package":package});
    files::write(
        &work.join("portable-report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}

pub fn smoke(root: &Path, options: &VerifyOptions) -> Result {
    let package = fs::canonicalize(&options.portable)?;
    let work = std::path::absolute(&options.work_dir)?;
    files::empty_destination(&work)?;
    let fixture = root.join("crates/bajptr-project/tests/fixtures/v1");
    let game = work.join("game");
    let data = work.join("project");
    let bundles = game.join("BlueArchive_Data/StreamingAssets/TableBundles");
    let catalog = bundles.join("Catalog/TableCatalog.bytes");
    let before = fs::read(fixture.join("TableCatalog.bytes"))?;
    let crc = i64::from_le_bytes(
        before
            .get(50..58)
            .ok_or("Fixture catalog too short.")?
            .try_into()?,
    );
    files::write(&catalog, &before)?;
    files::copy(
        &fixture.join("TableCatalog.hash"),
        &bundles.join("Catalog/TableCatalog.hash"),
    )?;
    files::copy(
        &fixture.join("encrypted.db"),
        &bundles.join(format!("123456_{crc}")),
    )?;
    files::copy(&fixture.join("rules.tsv"), &data.join(".gui/rules.tsv"))?;
    files::write(
        &data.join(".gui/settings.json"),
        &serde_json::to_vec(&json!({"format":2,"game":game,"skip_kana":true}))?,
    )?;
    let mut worker = Worker::start(
        clean_command(&cli(&package)?, &package, &work.join("clean-user"))?,
        &data,
        &work.join("retired"),
    )?;
    healthy(&worker.invoke("load", json!({}))?, 2)?;
    let checked = worker.invoke("check", json!({}))?;
    require(
        checked["baseline_status"]["state"] == "missing",
        "Fresh game should ask for a baseline.",
    )?;
    require(
        !data.join("backup/_baselines").exists(),
        "Checking silently created a baseline.",
    )?;
    worker.invoke(
        "baseline_commit",
        json!({"token":checked["baseline_status"]["token"]}),
    )?;
    let checked = worker.invoke("check", json!({}))?;
    require(
        checked["connected"] == true && checked["baseline_status"]["state"] == "ready",
        "Confirmed baseline is not ready.",
    )?;
    require(
        count(&worker.invoke("tables", json!({}))?) == 1,
        "Source table load failed.",
    )?;
    let search = worker.invoke("search", json!({"keyword":"任务"}))?;
    require(
        search["total"] == 1 && count(&search["rows"]) == 1,
        "Search must exclude kana-bearing fixture text.",
    )?;
    let opened = worker.invoke(
        "text_open",
        json!({"index":0,"search_nonce":search["nonce"]}),
    )?;
    let edited = worker.invoke(
        "text_save",
        json!({"nonce":opened["nonce"],"text":"Rust edited text"}),
    )?;
    require(count(&edited["edits"]) == 1, "Targeted edit failed.")?;
    let preview = worker.invoke("preview", json!({}))?;
    require(preview["total_changes"] == 1, "Preview failed.")?;
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&catalog)?;
    let fallback = worker.invoke("apply", json!({"nonce":preview["nonce"]}))?;
    require(
        fallback["installed"] == false
            && fallback["verified"] == true
            && fallback["warning"].as_str().is_some_and(|s| !s.is_empty())
            && fallback["output"]
                .as_str()
                .is_some_and(|p| Path::new(p).exists()),
        "Failed application did not preserve a verified patch and warning.",
    )?;
    drop(guard);
    require(
        fs::read(&catalog)? == before,
        "Failed application changed the catalog.",
    )?;
    let applied = worker.invoke("apply", json!({"nonce":preview["nonce"]}))?;
    require(
        applied["installed"] == true,
        "Application retry did not install the patch.",
    )?;
    worker.invoke("export_text", json!({"path":work.join("export")}))?;
    worker.invoke("restore", json!({}))?;
    require(
        fs::read(&catalog)? == before,
        "Restore differs from original.",
    )?;
    require(
        count(&worker.invoke("load", json!({}))?["edits"]) == 1,
        "Restore discarded documents.",
    )?;
    worker.stop()?;
    let report = json!({"passed":true,"baseline_confirmation_required":true,"environment_cleared":true,"separate_user_directories":true,"python_on_child_path":false,"current_format":true,"search":true,"search_skip_kana":true,"targeted_edit":true,"preview":true,"fallback_patch_verified":fallback["verified"],"fallback_warning":true,"apply_retry":true,"apply":true,"export":true,"restore_equal":true,"documents_preserved":true,"game_dir":game,"data_dir":data});
    files::write(
        &work.join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
