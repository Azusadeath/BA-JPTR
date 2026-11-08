//! Verified baseline transformations shared by the desktop and command line.
use crate::{
    Control, Result,
    baseline::{self, Baseline, Baselines},
    invalid,
    model::{self, Edit},
    region,
    storage::{self, Lock},
};
use bajptr_core::{
    codec, database,
    protocol::{Session, execute},
    rules::Rule,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

pub struct Config {
    pub rules: Vec<Rule>,
    pub edits: Vec<Edit>,
    pub rules_path: Option<PathBuf>,
    pub edits_path: Option<PathBuf>,
    pub skip_kana: bool,
    pub ascii_exact: bool,
    pub tables: String,
    pub work_dir: Option<PathBuf>,
}
pub struct PatchJob<'a> {
    pub game: &'a str,
    pub data: &'a Path,
    pub backup_root: &'a Path,
    pub output_root: &'a Path,
    pub config: &'a Config,
    pub install: bool,
    pub expected: Option<&'a Value>,
}
impl Config {
    pub fn new(rules: Vec<Rule>, edits: Vec<Edit>, skip_kana: bool) -> Self {
        Self {
            rules,
            edits,
            rules_path: None,
            edits_path: None,
            skip_kana,
            ascii_exact: true,
            tables: database::default_tables(),
            work_dir: None,
        }
    }
    fn options(&self, game: &str, version: &str) -> Value {
        json!({"skip_kana":self.skip_kana,"ascii_exact":self.ascii_exact,"tables":self.tables,"explain":true,"edits":self.edits.iter().filter(|e|e.game==game && e.version==version).map(Edit::core).collect::<Vec<_>>()})
    }
    fn check_documents(&self) -> Result<()> {
        if let Some(path) = &self.rules_path {
            let rules = if path.exists() {
                model::parse_rules(&fs::read(path)?)?
            } else {
                vec![]
            };
            if rules != self.rules {
                return Err(invalid("词表已被其他窗口修改，请重新打开并预览。"));
            }
        }
        if let Some(path) = &self.edits_path {
            let edits = if path.exists() {
                model::parse_edits(&fs::read(path)?)?
            } else {
                vec![]
            };
            if edits != self.edits {
                return Err(invalid("单条修改记录已变化，请重新打开并预览。"));
            }
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Engine {
    pub baselines: Baselines,
    session: Session,
    source_key: String,
    scans: VecDeque<(String, Value)>,
    previews: VecDeque<(String, Value)>,
}
impl Engine {
    pub fn close(&mut self) {
        self.session = Session::default();
        self.source_key.clear();
        self.scans.clear();
        self.previews.clear();
    }
    fn open(&mut self, original: &Baseline, control: &Control<'_>) -> Result<()> {
        let key = format!(
            "{}:{:?}",
            original.bundle.display(),
            baseline::stamp(&original.bundle)?
        );
        if key != self.source_key {
            self.close();
            control.log("正在读取初始底稿…")?;
            execute(
                &mut self.session,
                json!({"op":"open","input":original.bundle,"key":region::KEY,"encrypted":true}),
                control,
            )?;
            self.source_key = key;
        }
        Ok(())
    }
    pub fn baseline(&mut self, game: &str, root: &Path, control: &Control<'_>) -> Result<Baseline> {
        let game = region::locate(game)?;
        let _lock = Lock::acquire(&game.catalog)?;
        self.baselines.resolve(&game, root, control)
    }
    pub fn baseline_status(
        &mut self,
        game: &str,
        root: &Path,
        data: &Path,
        control: &Control<'_>,
    ) -> Result<Value> {
        let live = region::locate(game)?;
        let _lock = Lock::acquire(&live.catalog)?;
        self.baseline_status_locked(&live, root, data, control)
    }
    fn baseline_status_locked(
        &mut self,
        live: &region::Game,
        root: &Path,
        data: &Path,
        control: &Control<'_>,
    ) -> Result<Value> {
        let destination = baseline::location(live, root)?;
        let record = destination.join("baseline.json");
        let identity = region::resource_identity(live)?;
        let version = region::version(&live.catalog)?;
        let key = storage::identity(&live.dir)?;
        let receipts = read_receipts(&data.join(".gui/application-state.json"));
        let receipt = &receipts[&key];
        // Touching or copying a file does not turn our own patch into a game update.
        let known = receipt["identity"].is_array()
            && [0, 1, 4, 5]
                .iter()
                .all(|i| receipt["identity"][*i] == identity[*i]);
        let mut saved = Value::Null;
        let state = if destination.exists() {
            let original = self.baselines.resolve(live, root, control)?;
            let entry = region::parse(&fs::read(&original.catalog)?)?;
            saved = baseline::info(&original)?;
            if (entry.size, entry.crc) == (live.entry.size, live.entry.crc) || known {
                "ready"
            } else {
                "changed"
            }
        } else if known && receipt["kind"] != "restored" {
            "blocked"
        } else if fs::read_dir(destination.parent().unwrap())
            .ok()
            .is_some_and(|entries| {
                entries.flatten().any(|e| {
                    model::is_version(&e.file_name().to_string_lossy())
                        && e.path().join("baseline.json").is_file()
                })
            })
        {
            "update"
        } else {
            "missing"
        };
        let token = json!({"game":key,"version":version,"identity":identity,"saved":storage::revision(&record)?});
        let marker = fs::read(live.catalog.with_file_name("TableCatalog.hash"))
            .ok()
            .map(|b| {
                String::from_utf8_lossy(&b)
                    .trim()
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .unwrap_or_else(|| "当前资源版本".into());
        Ok(json!({"state":state,"token":token,"baseline":saved,"game_version":marker}))
    }
    pub fn initialize(
        &mut self,
        game: &str,
        root: &Path,
        data: &Path,
        expected: Option<&Value>,
        control: &Control<'_>,
    ) -> Result<Baseline> {
        let live = region::locate(game)?;
        let _lock = Lock::acquire(&live.catalog)?;
        let status = self.baseline_status_locked(&live, root, data, control)?;
        if expected.is_some_and(|v| v != &status["token"]) {
            return Err(invalid(
                "游戏资源或底稿已变化，请重新检查后确认，未替换原底稿。",
            ));
        }
        match status["state"].as_str() {
            Some("ready") => self.baselines.resolve(&live, root, control),
            Some("missing" | "update" | "changed") => {
                self.close();
                self.baselines
                    .capture(&live, root, status["state"] == "changed", control)
            }
            _ => Err(invalid(
                "当前资源是本工具修改后的内容，不能作为初始底稿。请先通过游戏修复资源，或找回原底稿。",
            )),
        }
    }
    pub fn tables(&mut self, game: &str, root: &Path, control: &Control<'_>) -> Result<Value> {
        self.tables_cached(game, root, None, control)
    }
    pub fn tables_cached(
        &mut self,
        game: &str,
        root: &Path,
        metadata: Option<&Path>,
        control: &Control<'_>,
    ) -> Result<Value> {
        let original = self.baseline(game, root, control)?;
        let live = region::locate(game)?;
        let identity = region::source_identity(&live, &original.bundle)?;
        if let Some(path) = metadata
            && let Some(value) = fs::read(path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            && value["format"] == 2
            && value["identity"] == identity
            && value["tables"]
                .as_array()
                .is_some_and(|a| a.iter().all(|s| s.as_str().is_some_and(|s| !s.is_empty())))
        {
            control.log("来源表已就绪。")?;
            return Ok(value["tables"].clone());
        }
        self.open(&original, control)?;
        let tables = execute(&mut self.session, json!({"op":"tables"}), control)?;
        if region::source_identity(&region::locate(game)?, &original.bundle)? != identity {
            return Err(invalid("游戏资源在读取期间发生变化，请重试。"));
        }
        if let Some(path) = metadata {
            let _ = storage::atomic_write(
                path,
                &serde_json::to_vec(&json!({"format":2,"identity":identity,"tables":tables}))?,
                None,
            );
        }
        Ok(tables)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn scan(
        &mut self,
        game: &str,
        root: &Path,
        keyword: &str,
        tables: &str,
        limit: usize,
        offset: usize,
        skip_kana: bool,
        control: &Control<'_>,
    ) -> Result<(Value, String)> {
        if keyword.trim().is_empty() {
            return Err(invalid("请输入要查找的文字。"));
        }
        let original = self.baseline(game, root, control)?;
        self.open(&original, control)?;
        let table_pattern = if tables.trim().is_empty() {
            database::default_tables()
        } else {
            format!("^(?:{})$", regex::escape(tables))
        };
        let key = format!("{keyword}\0{tables}\0{limit}\0{offset}\0{skip_kana}");
        let value = if let Some((_, value)) = self.scans.iter().find(|(k, _)| k == &key) {
            value.clone()
        } else {
            let value = execute(
                &mut self.session,
                json!({"op":"scan","keyword":keyword,"tables":table_pattern,"limit":limit,"offset":offset,"skip_kana":skip_kana}),
                control,
            )?;
            remember(&mut self.scans, key, value.clone(), 8);
            value
        };
        if region::version(&region::locate(game)?.catalog)? != original.version {
            return Err(invalid("游戏资源版本已变化，请重新查找。"));
        }
        Ok((value, original.version))
    }
    pub fn export(
        &mut self,
        game: &str,
        root: &Path,
        target: &Path,
        control: &Control<'_>,
    ) -> Result<u64> {
        let original = self.baseline(game, root, control)?;
        self.open(&original, control)?;
        Ok(execute(
            &mut self.session,
            json!({"op":"export","destination":target}),
            control,
        )?["count"]
            .as_u64()
            .unwrap_or(0))
    }
    pub fn preview(
        &mut self,
        game: &str,
        data: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<Value> {
        self.preview_at(game, data, &data.join("backup"), config, control)
    }
    pub fn preview_at(
        &mut self,
        game: &str,
        data: &Path,
        backup_root: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<Value> {
        config.check_documents()?;
        let live = region::locate(game)?;
        let identity = region::resource_identity(&live)?;
        let original = self.baseline(game, backup_root, control)?;
        self.open(&original, control)?;
        let options = config.options(&storage::identity(&live.dir)?, &original.version);
        let key = storage::digest(&serde_json::to_vec(&json!([
            identity,
            config.rules,
            options,
            config.edits
        ]))?);
        let mut stats = if let Some((_, stats)) = self.previews.iter().find(|(k, _)| k == &key) {
            stats.clone()
        } else {
            let mut value = execute(
                &mut self.session,
                json!({"op":"preview","rules":config.rules,"options":options,"current":live.bundle,"current_identity":identity.to_string(),"key":region::KEY}),
                control,
            )?;
            value["comparison_identity"] = identity.clone();
            value["rule_hits"] = json!(
                config
                    .rules
                    .iter()
                    .zip(value["hits"].as_array().unwrap())
                    .map(|(r, h)| json!([r.old, r.new, r.mode, r.source, r.table, h]))
                    .collect::<Vec<_>>()
            );
            value["zero_reasons"] = json!(
                value["zero_reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| json!([m["index"], m["reason"], m["example"], m["table"]]))
                    .collect::<Vec<_>>()
            );
            value["inactive_edits"] = json!(
                config
                    .edits
                    .iter()
                    .filter(
                        |e| e.game == storage::identity(&live.dir).unwrap_or_default()
                            && e.version != original.version
                    )
                    .map(Edit::array)
                    .collect::<Vec<_>>()
            );
            remember(&mut self.previews, key, value.clone(), 4);
            value
        };
        if region::resource_identity(&region::locate(game)?)? != identity {
            return Err(invalid("游戏资源在预览期间发生变化，请重新预览。"));
        }
        config.check_documents()?;
        stats["total_changes"] = json!(total(&stats));
        if total(&stats) == 0
            && stats["parse_failed"] == 0
            && let Err(e) = record(game, data, config, "checked", Some(&identity))
        {
            control.log(&format!("预览完成；应用状态记录未保存：{e}"))?;
        }
        Ok(stats)
    }
    pub fn generate(&mut self, job: PatchJob<'_>, control: &Control<'_>) -> Result<Value> {
        let PatchJob {
            game,
            data,
            backup_root,
            output_root,
            config,
            install,
            expected,
        } = job;
        config.check_documents()?;
        let live = region::locate(game)?;
        let _game_lock = Lock::acquire(&live.catalog)?;
        let identity = region::resource_identity(&live)?;
        if expected.is_some_and(|e| e != &identity) {
            return Err(invalid("游戏资源已变化，请重新预览。"));
        }
        let original = self.baselines.resolve(&live, backup_root, control)?;
        let source_stat = baseline::stamp(&original.bundle)?;
        self.close();
        let stage_root = config.work_dir.clone().unwrap_or_else(|| data.join("work"));
        fs::create_dir_all(&stage_root)?;
        let stage = stage_root.join(format!("patch-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&stage)?;
        crate::cleanup::mark(&stage)?;
        let started = std::time::Instant::now();
        let result = (|| -> Result<Value> {
            let plain = stage.join("working.db");
            control.log("正在从初始底稿重建…")?;
            let key = codec::key(region::KEY)?;
            let info = codec::decrypt(&original.bundle, &plain, &key, control)?;
            let options = config.options(&storage::identity(&live.dir)?, &original.version);
            let stats = execute(
                &mut Session::default(),
                json!({"op":"patch_file","input":plain,"rules":config.rules,"options":options}),
                control,
            )?;
            let connection = rusqlite::Connection::open_with_flags(
                &plain,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let integrity: String =
                connection.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            drop(connection);
            if integrity != "ok" {
                return Err(invalid("生成数据库完整性校验失败，未发布补丁。"));
            }
            let encrypted = stage.join("encrypted.db");
            let salt = B64
                .decode(&info.salt)
                .map_err(|_| invalid("底稿 salt 无效。"))?;
            let ivs = info
                .ivs
                .iter()
                .map(|v| B64.decode(v).map_err(|_| invalid("底稿 IV 无效。")))
                .collect::<Result<Vec<_>>>()?;
            control.log("正在加密并校验 HMAC…")?;
            let (_, checked) =
                codec::encrypt_checked(&plain, &encrypted, &key, &salt, &ivs, control)?;
            if checked.ok == 0 || checked.bad > 0 {
                return Err(invalid("生成资源 HMAC 校验失败，未发布补丁。"));
            }
            let _rules_lock = config
                .rules_path
                .as_ref()
                .map(|p| Lock::acquire(p))
                .transpose()?;
            let _edits_lock = config
                .edits_path
                .as_ref()
                .map(|p| Lock::acquire(p))
                .transpose()?;
            config.check_documents()?;
            if fs::read(&live.catalog)? != live.bytes
                || region::version(&live.catalog)? != original.version
                || region::resource_identity(&region::locate(game)?)? != identity
                || baseline::stamp(&original.bundle)? != source_stat
            {
                return Err(invalid(
                    "处理期间游戏资源或底稿发生变化，未发布补丁，请重新预览。",
                ));
            }
            let prefix = original
                .bundle
                .file_name()
                .unwrap()
                .to_string_lossy()
                .rsplit_once('_')
                .ok_or_else(|| invalid("底稿文件名无效。"))?
                .0
                .to_string();
            let name = format!("{prefix}_{}", checked.crc);
            let target = if install {
                live.dir.join(region::BUNDLES).join(&name)
            } else {
                let folder = output_root.join(format!(
                    "patch-{}-{}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S"),
                    uuid::Uuid::new_v4().simple()
                ));
                fs::create_dir_all(&folder)?;
                folder.join(&name)
            };
            let size = encrypted.metadata()?.len();
            // Cancellation is checked before this commit point; publication then finishes.
            control.check()?;
            let commit_control = Control {
                callback: &|_| true,
            };
            publish_resource(
                &mut self.baselines,
                &encrypted,
                &target,
                size,
                checked.crc,
                &commit_control,
            )?;
            if install {
                region::publish_catalog(&live, size, checked.crc)?;
                if let Err(e) = record(game, data, config, "applied", None) {
                    let _ = control.log(&format!("游戏已应用；状态记录未保存：{e}"));
                }
            } else {
                let mut catalog = live.bytes.clone();
                catalog[live.entry.size_offset..live.entry.size_offset + 8]
                    .copy_from_slice(&(size as i64).to_le_bytes());
                catalog[live.entry.crc_offset..live.entry.crc_offset + 8]
                    .copy_from_slice(&(checked.crc as i64).to_le_bytes());
                storage::write_locked(&target.with_file_name("TableCatalog.bytes"), &catalog)?;
                let marker = live.catalog.with_file_name("TableCatalog.hash");
                if marker.is_file() {
                    baseline::copy_sync(&marker, &target.with_file_name("TableCatalog.hash"))?;
                }
            }
            Ok(
                json!({"output":target,"stats":stats,"verified":true,"processing_seconds":started.elapsed().as_secs_f64()}),
            )
        })();
        // A failed stage is kept for diagnosis/recovery; successful scratch data is retired.
        if result.is_ok() {
            crate::cleanup::retire_success(&stage, control);
        }
        result
    }
    pub fn restore(
        &mut self,
        game: &str,
        data: &Path,
        root: &Path,
        control: &Control<'_>,
    ) -> Result<Baseline> {
        let live = region::locate(game)?;
        let _lock = Lock::acquire(&live.catalog)?;
        let identity = region::resource_identity(&live)?;
        let original = self.baselines.resolve(&live, root, control)?;
        self.close();
        let entry = region::parse(&fs::read(&original.catalog)?)?;
        self.baselines.checked(&original.bundle, &entry, control)?;
        let target = live
            .dir
            .join(region::BUNDLES)
            .join(original.bundle.file_name().unwrap());
        if fs::read(&live.catalog)? != live.bytes
            || region::version(&live.catalog)? != original.version
            || region::resource_identity(&region::locate(game)?)? != identity
        {
            return Err(invalid("恢复期间游戏资源发生变化，请重试。"));
        }
        control.check()?;
        publish_resource(
            &mut self.baselines,
            &original.bundle,
            &target,
            entry.size,
            entry.crc,
            &Control {
                callback: &|_| true,
            },
        )?;
        region::publish_catalog(&live, entry.size, entry.crc)?;
        if let Err(e) = record(
            game,
            data,
            &Config::new(vec![], vec![], true),
            "restored",
            None,
        ) {
            let _ = control.log(&format!("底稿已恢复；状态记录未保存：{e}"));
        }
        Ok(original)
    }
}
fn memory(value: &Value) -> usize {
    match value {
        Value::String(s) => s.len() + 32,
        Value::Array(a) => a.iter().map(memory).sum::<usize>() + 32 * a.len(),
        Value::Object(o) => o.iter().map(|(k, v)| k.len() + memory(v) + 64).sum(),
        _ => 32,
    }
}
fn remember(cache: &mut VecDeque<(String, Value)>, key: String, value: Value, limit: usize) {
    const CAP: usize = 12 * 1024 * 1024;
    let size = memory(&value) + key.len();
    if size > CAP {
        return;
    }
    while cache.len() >= limit
        || cache
            .iter()
            .map(|(k, v)| k.len() + memory(v))
            .sum::<usize>()
            + size
            > CAP
    {
        cache.pop_front();
    }
    cache.push_back((key, value));
}
fn publish_resource(
    checks: &mut Baselines,
    source: &Path,
    target: &Path,
    size: u64,
    crc: u32,
    control: &Control<'_>,
) -> Result<()> {
    let entry = region::Entry {
        size_offset: 0,
        crc_offset: 0,
        size,
        crc,
    };
    if target.exists() {
        checks.checked(target, &entry, control)?;
        return Ok(());
    }
    let pending = target.with_file_name(format!(
        "{}.pending-{}",
        target.file_name().unwrap().to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    baseline::copy_sync(source, &pending)?;
    // Verify the bytes that reached the destination, including restore copies.
    // The encryption-time digest alone cannot detect a damaged copied file.
    checks.checked(&pending, &entry, control)?;
    // All cooperating writers hold the catalog lock. Never replace an unrelated resource.
    if target.exists() {
        return Err(invalid("目标资源在发布前出现，请重新预览。"));
    }
    fs::rename(&pending, target)?;
    Ok(())
}
pub fn total(stats: &Value) -> u64 {
    ["added", "updated", "reverted"]
        .iter()
        .map(|k| stats[k].as_u64().unwrap_or(0))
        .sum()
}
pub fn record(
    game: &str,
    data: &Path,
    config: &Config,
    kind: &str,
    expected: Option<&Value>,
) -> Result<()> {
    let live = region::locate(game)?;
    let identity = region::resource_identity(&live)?;
    if expected.is_some_and(|v| v != &identity) {
        return Err(invalid("游戏资源已变化，请重新预览。"));
    }
    let key = storage::identity(&live.dir)?;
    let edits = config
        .edits
        .iter()
        .filter(|e| e.game == key)
        .cloned()
        .collect::<Vec<_>>();
    let path = data.join(".gui/application-state.json");
    let _lock = Lock::acquire(&path)?;
    let mut values = read_receipts(&path);
    values[&key] = json!({"identity":identity,"fingerprint":model::fingerprint(&config.rules,config.skip_kana,&edits)?,"kind":kind});
    storage::write_locked(
        &path,
        &serde_json::to_vec(&json!({"format":2,"games":values}))?,
    )?;
    Ok(())
}
fn read_receipts(path: &Path) -> Value {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(|v| v["format"] == 2 && v["games"].is_object())
        .map(|v| v["games"].clone())
        .unwrap_or(json!({}))
}
pub fn status(game: &str, data: &Path, config: &Config) -> Result<String> {
    let live = region::locate(game)?;
    let key = storage::identity(&live.dir)?;
    let receipts = read_receipts(&data.join(".gui/application-state.json"));
    let receipt = &receipts[&key];
    if !receipt.is_object() {
        return Ok("应用状态待核对，先预览".into());
    }
    if receipt["identity"] != region::resource_identity(&live)? {
        return Ok("游戏资源已变化，请重新预览".into());
    }
    let edits = config
        .edits
        .iter()
        .filter(|e| e.game == key)
        .cloned()
        .collect::<Vec<_>>();
    Ok(if receipt["kind"] == "restored" {
        if config.rules.is_empty() && edits.is_empty() {
            "已恢复底稿，与当前空词表一致"
        } else {
            "已恢复底稿；词表尚未应用"
        }
    } else if receipt["fingerprint"] != model::fingerprint(&config.rules, config.skip_kana, &edits)?
    {
        "词表已保存，尚未应用"
    } else if receipt["kind"] == "applied" {
        "已应用，与当前词表一致"
    } else {
        "当前游戏文字与词表一致"
    }
    .into())
}
