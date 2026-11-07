use crate::{
    Control, Result, invalid, model,
    region::{self, Entry, Game},
    storage,
};
use bajptr_core::codec;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone)]
pub struct Baseline {
    pub bundle: PathBuf,
    pub catalog: PathBuf,
    pub version: String,
    pub origin: String,
}
type Stamp = (u64, SystemTime, SystemTime);
#[derive(Default)]
pub struct Baselines {
    verified: HashMap<PathBuf, (Stamp, u32)>,
}
pub fn stamp(path: &Path) -> Result<Stamp> {
    let m = path.metadata()?;
    Ok((
        m.len(),
        m.modified()?,
        m.created().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}
pub fn location(game: &Game, root: &Path) -> Result<PathBuf> {
    let version = region::version(&game.catalog)?;
    let game_id = storage::digest(storage::identity(&game.dir)?.as_bytes());
    Ok(root.join("_baselines").join(&game_id[..24]).join(version))
}
impl Baselines {
    pub fn checked(&mut self, path: &Path, entry: &Entry, control: &Control<'_>) -> Result<()> {
        let stat = stamp(path)?;
        if stat.0 != entry.size {
            return Err(invalid("初始底稿或资源大小不符，未改动游戏。"));
        }
        let crc = if let Some((cached, crc)) = self.verified.get(path).filter(|(s, _)| *s == stat) {
            let _ = cached;
            *crc
        } else {
            let inspected = codec::inspect(path, &codec::key(region::KEY)?, control)?;
            if inspected.ok == 0 || inspected.bad > 0 {
                return Err(invalid("数据库 HMAC 校验失败，未接受此资源。"));
            }
            if stamp(path)? != stat {
                return Err(invalid("数据库在校验期间发生变化，请重试。"));
            }
            if self.verified.len() >= 32 {
                self.verified.clear();
            }
            self.verified.insert(path.into(), (stat, inspected.crc));
            inspected.crc
        };
        if crc != entry.crc {
            return Err(invalid("数据库 CRC 校验失败，未改动游戏。"));
        }
        Ok(())
    }
    /// Requires the live catalog lock. Never creates or retires a baseline.
    pub fn resolve(&mut self, game: &Game, root: &Path, control: &Control<'_>) -> Result<Baseline> {
        self.resolve_locked(game, root, control, false, false)
    }
    /// Called only after explicit confirmation. The live catalog lock is held by the caller.
    pub fn capture(
        &mut self,
        game: &Game,
        root: &Path,
        replace: bool,
        control: &Control<'_>,
    ) -> Result<Baseline> {
        control.check()?;
        let callback = |message: Option<&str>| {
            if let Some(m) = message {
                let _ = (control.callback)(Some(m));
            }
            true
        };
        let uninterrupted = Control {
            callback: &callback,
        };
        let result = self.resolve_locked(game, root, &uninterrupted, true, replace)?;
        control.check()?;
        Ok(result)
    }
    fn resolve_locked(
        &mut self,
        game: &Game,
        root: &Path,
        control: &Control<'_>,
        create: bool,
        replace: bool,
    ) -> Result<Baseline> {
        let version = region::version(&game.catalog)?;
        let destination = location(game, root)?;
        let record_path = destination.join("baseline.json");
        if record_path.is_file() && !replace {
            let record: Value = serde_json::from_slice(&fs::read(&record_path)?)?;
            let name = record["db_file"]
                .as_str()
                .ok_or_else(|| invalid("底稿数据库记录无效。"))?;
            if record["format"] != 2
                || !storage::safe_name(name)
                || record["version"] != version
                || storage::identity(Path::new(record["game_dir"].as_str().unwrap_or("")))?
                    != storage::identity(&game.dir)?
            {
                return Err(invalid("底稿记录与当前游戏不匹配。"));
            }
            let catalog = destination.join("TableCatalog.bytes");
            if region::version(&catalog)? != version {
                return Err(invalid("底稿资源版本不匹配。"));
            }
            let entry = region::parse(&fs::read(&catalog)?)?;
            if !name.ends_with(&format!("_{}", entry.crc)) {
                return Err(invalid("底稿记录与数据库名称不匹配。"));
            }
            let bundle = destination.join(name);
            self.checked(&bundle, &entry, control)?;
            return Ok(Baseline {
                bundle,
                catalog,
                version,
                origin: record["origin"].as_str().unwrap_or("").into(),
            });
        }
        if destination.exists() && (!replace || !record_path.is_file()) {
            return Err(invalid(
                "初始底稿目录不完整，未用当前已修改内容代替。请检查 backup/_baselines。",
            ));
        }
        if !create {
            return Err(invalid(
                "尚未确认当前版本的初始底稿，请先检查游戏目录并确认保存底稿。",
            ));
        }
        let identity = region::resource_identity(game)?;
        self.checked(&game.bundle, &game.entry, control)?;
        control.log("正在保存本版本初始底稿…")?;
        let (source, catalog, origin) = (
            game.bundle.clone(),
            game.catalog.clone(),
            "用户确认的游戏资源".to_string(),
        );
        let entry = region::parse(&fs::read(&catalog)?)?;
        fs::create_dir_all(destination.parent().unwrap())?;
        let stage = destination.with_file_name(format!(
            "{version}.building-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&stage)?;
        let name = source.file_name().unwrap();
        copy_sync(&source, &stage.join(name))?;
        copy_sync(&catalog, &stage.join("TableCatalog.bytes"))?;
        let marker = catalog.with_file_name("TableCatalog.hash");
        if marker.is_file() {
            copy_sync(&marker, &stage.join("TableCatalog.hash"))?;
        }
        self.checked(&stage.join(name), &entry, control)?;
        control.log("正在验证底稿数据库完整性…")?;
        let db = bajptr_core::database::Database::open(
            &stage.join(name),
            true,
            &codec::key(region::KEY)?,
            control,
        )?;
        let integrity: String = db
            .con
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        drop(db);
        if integrity != "ok" {
            return Err(invalid("底稿数据库完整性校验失败，原底稿未替换。"));
        }
        if region::parse(&fs::read(stage.join("TableCatalog.bytes"))?)? != entry
            || region::version(&stage.join("TableCatalog.bytes"))? != version
            || region::version(&game.catalog)? != version
            || fs::read(&game.catalog)? != game.bytes
            || region::resource_identity(&region::locate(
                game.dir.to_str().ok_or_else(|| invalid("游戏路径无效。"))?,
            )?)? != identity
        {
            return Err(invalid(
                "建立底稿期间资源发生变化，请重试。暂存副本已保留。",
            ));
        }
        storage::write_locked(
            &stage.join("baseline.json"),
            &serde_json::to_vec_pretty(
                &json!({"format":2,"id":uuid::Uuid::new_v4().simple().to_string(),"game_dir":game.dir,"version":version,"created":chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),"db_file":name.to_string_lossy(),"origin":origin}),
            )?,
        )?;
        let previous = destination.with_file_name(format!(
            "{version}.superseded-{}",
            uuid::Uuid::new_v4().simple()
        ));
        if replace {
            fs::rename(&destination, &previous)?;
        }
        if let Err(error) = fs::rename(&stage, &destination) {
            if replace {
                fs::rename(&previous, &destination)?;
            }
            return Err(error.into());
        }
        if replace && let Err(error) = storage::retire(&previous) {
            let _ = control.log(&format!(
                "新底稿已保存，旧副本仍保留在 {}：{error}",
                previous.display()
            ));
        }
        control.log("初始底稿已保存，之后从这份底稿重建。")?;
        retire_old(&destination, control);
        Ok(Baseline {
            bundle: destination.join(name),
            catalog: destination.join("TableCatalog.bytes"),
            version,
            origin,
        })
    }
}
pub fn copy_sync(source: &Path, target: &Path) -> Result<()> {
    fs::copy(source, target)?;
    fs::OpenOptions::new()
        .write(true)
        .open(target)?
        .sync_all()?;
    Ok(())
}
fn retire_old(current: &Path, control: &Control<'_>) {
    if let Ok(entries) = fs::read_dir(current.parent().unwrap()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path == current
                || !model::is_version(&entry.file_name().to_string_lossy())
                || !path.join("baseline.json").is_file()
            {
                continue;
            }
            match storage::retire(&path) {
                Ok(_) => {
                    let _ = control.log("新版底稿已验证，旧版底稿已移出项目。");
                }
                Err(e) => {
                    let _ = control.log(&e.to_string());
                }
            }
        }
    }
}
pub fn info(original: &Baseline) -> Result<Value> {
    let record: Value =
        serde_json::from_slice(&fs::read(original.catalog.with_file_name("baseline.json"))?)?;
    let marker = fs::read(original.catalog.with_file_name("TableCatalog.hash"))
        .ok()
        .map(|b| {
            String::from_utf8_lossy(&b)
                .trim()
                .chars()
                .take(80)
                .collect::<String>()
        })
        .unwrap_or("当前资源版本".into());
    Ok(
        json!({"version":marker,"created":record["created"].as_str().unwrap_or("").replace('T'," "),"verified":true,"folder":original.bundle.parent(),"resource":original.bundle.file_name().unwrap().to_string_lossy(),"origin":original.origin,"version_id":original.version}),
    )
}
