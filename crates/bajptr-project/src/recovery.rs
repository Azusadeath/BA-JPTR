use crate::{
    Control, Error, Result,
    baseline::{self, Baselines},
    invalid,
    region::{self, Game},
    storage,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn baseline_plan(live: &Game, root: &Path, control: &Control<'_>) -> Result<Value> {
    control.check()?;
    let folder = baseline::location(live, root)?;
    if !folder.is_dir() {
        return Err(invalid("没有损坏底稿可重建，请先检查底稿。"));
    }
    if Baselines::default().resolve(live, root, control).is_ok() {
        return Err(invalid("现有底稿可以通过校验，无需重建。"));
    }
    control.check()?;
    let mut catalog = folder.join("TableCatalog.bytes");
    let version = region::version(&live.catalog)?;
    if region::version(&catalog).ok().as_deref() != Some(&version) {
        let record = fs::read(folder.join("baseline.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let matches_original = record["format"] == 2
            && record["version"] == version
            && record["game_dir"].as_str().is_some_and(|dir| {
                storage::identity(Path::new(dir)).ok() == storage::identity(&live.dir).ok()
            })
            && record["db_file"].as_str().is_some_and(|name| {
                storage::safe_name(name) && name.ends_with(&format!("_{}", live.entry.crc))
            });
        if !matches_original {
            return Err(invalid(
                "原底稿的目录记录已损坏，当前资源也无法与原数据库记录核对。请先修复游戏资源再重试，或找回完整底稿。",
            ));
        }
        catalog = live.catalog.clone();
    }
    let marker = catalog.with_file_name("TableCatalog.hash");
    let entry = region::parse(&fs::read(&catalog)?)?;
    let suffix = format!("_{}", entry.crc);
    let mut candidates = Vec::new();
    for directory in [&folder, &live.dir.join(region::BUNDLES)] {
        for candidate in fs::read_dir(directory)? {
            let candidate = candidate?;
            if candidate.file_type()?.is_file()
                && candidate.file_name().to_string_lossy().ends_with(&suffix)
            {
                candidates.push(candidate.path());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    for source in candidates {
        control.check()?;
        let verified = (|| -> Result<()> {
            Baselines::default().checked(&source, &entry, control)?;
            verify_sqlite(&source, control)
        })();
        match verified {
            Ok(()) => {
                return Ok(
                    json!({"state":"repair","folder":folder,"source":source,"catalog":catalog,"marker":marker,
                "token":{"game":storage::identity(&live.dir)?,"version":version,
                    "live":region::resource_identity(live)?,"record":storage::revision(&folder.join("baseline.json"))?,
                    "catalog":storage::revision(&catalog)?,"marker":storage::revision(&marker)?,
                    "damaged_catalog":storage::revision(&folder.join("TableCatalog.bytes"))?,"damaged_marker":storage::revision(&folder.join("TableCatalog.hash"))?,
                    "source":source,"source_identity":region::source_identity(live,&source)?}}),
                );
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => {
                control.warn(&format!("重建候选校验失败 {}：{error}", source.display()))?
            }
        }
    }
    Err(invalid(
        "找不到与原底稿目录记录一致且校验通过的数据库，未改动损坏底稿。请找回原底稿，或修复游戏资源后重试；已替换的文本不能代替原文。",
    ))
}

fn verify_sqlite(path: &Path, control: &Control<'_>) -> Result<()> {
    let database = bajptr_core::database::Database::open(
        path,
        true,
        &bajptr_core::codec::key(region::KEY)?,
        control,
    )?;
    let result: String = database
        .con
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(invalid("重建候选数据库完整性校验失败。"));
    }
    Ok(())
}

pub fn rebuild_baseline(
    live: &Game,
    root: &Path,
    expected: &Value,
    control: &Control<'_>,
) -> Result<Value> {
    let plan = baseline_plan(live, root, control)?;
    if expected != &plan["token"] {
        return Err(invalid("游戏资源或底稿已变化，请重新尝试重建。"));
    }
    let folder = baseline::location(live, root)?;
    let source = Path::new(plan["source"].as_str().unwrap());
    let version = region::version(&live.catalog)?;
    let stage = folder.with_file_name(format!(
        "{version}.rebuilding-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&stage)?;
    let name = source.file_name().unwrap();
    baseline::copy_sync(source, &stage.join(name))?;
    baseline::copy_sync(
        Path::new(plan["catalog"].as_str().unwrap()),
        &stage.join("TableCatalog.bytes"),
    )?;
    if Path::new(plan["marker"].as_str().unwrap()).is_file() {
        baseline::copy_sync(
            Path::new(plan["marker"].as_str().unwrap()),
            &stage.join("TableCatalog.hash"),
        )?;
    }
    let entry = region::parse(&fs::read(stage.join("TableCatalog.bytes"))?)?;
    Baselines::default().checked(&stage.join(name), &entry, control)?;
    verify_sqlite(&stage.join(name), control)?;
    storage::write_locked(
        &stage.join("baseline.json"),
        &serde_json::to_vec_pretty(&json!({
            "format":2,"id":uuid::Uuid::new_v4().simple().to_string(),"game_dir":live.dir,
            "version":version,"created":chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            "db_file":name.to_string_lossy(),"origin":"从校验通过的原始资源重建"
        }))?,
    )?;
    let current = region::locate(live.dir.to_str().ok_or_else(|| invalid("游戏路径无效。"))?)?;
    if baseline_plan(&current, root, control)?["token"] != *expected {
        return Err(invalid(
            "重建期间资源发生变化，暂存副本已保留，原底稿未替换。",
        ));
    }
    control.check()?;
    let damaged = folder.with_file_name(format!(
        "{version}.damaged-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::rename(&folder, &damaged)?;
    if let Err(error) = fs::rename(&stage, &folder) {
        if let Err(rollback) = fs::rename(&damaged, &folder) {
            return Err(invalid(format!(
                "重建发布失败：{error}；旧底稿仍保留在 {}，恢复目录失败：{rollback}",
                damaged.display()
            )));
        }
        return Err(error.into());
    }
    let original = baseline::Baseline {
        bundle: folder.join(name),
        catalog: folder.join("TableCatalog.bytes"),
        version,
        origin: "从校验通过的原始资源重建".into(),
    };
    let mut info = baseline::info(&original)?;
    info["damaged_folder"] = json!(damaged);
    Ok(info)
}
