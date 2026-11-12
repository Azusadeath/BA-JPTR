use crate::{Control, Error, Result, baseline, invalid, storage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const JOURNAL: &str = "导出未完成.json";
#[derive(Serialize, Deserialize)]
struct ExportFile {
    name: String,
    before: Option<String>,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct ExportTransaction {
    stage: String,
    files: Vec<ExportFile>,
}

fn rollback(target: &Path, stage: &Path, transaction: &ExportTransaction) -> Result<()> {
    for file in &transaction.files {
        let current = storage::revision(&target.join(&file.name))?;
        if current != file.before && current.as_ref() != Some(&file.after) {
            return Err(invalid(
                "导出中断后目标文件又被其他程序修改，未覆盖这些文件。",
            ));
        }
        if file.before.is_some()
            && storage::revision(&stage.join("previous").join(&file.name))? != file.before
        {
            return Err(invalid("导出恢复副本缺失或损坏，未覆盖目标文件。"));
        }
    }
    for file in &transaction.files {
        let path = target.join(&file.name);
        if storage::revision(&path)? == file.before {
            continue;
        }
        if file.before.is_some() {
            let pending = storage::temporary_path(&path)?;
            baseline::copy_sync(&stage.join("previous").join(&file.name), &pending)?;
            storage::replace(&pending, &path)?;
        } else {
            let withdrawn = stage.join("withdrawn");
            fs::create_dir_all(&withdrawn)?;
            fs::rename(&path, withdrawn.join(&file.name))?;
        }
    }
    fs::remove_file(target.join(JOURNAL))?;
    Ok(())
}

fn recover(target: &Path) -> Result<()> {
    let bytes = match fs::read(target.join(JOURNAL)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let transaction: ExportTransaction = serde_json::from_slice(&bytes)?;
    if !storage::safe_name(&transaction.stage)
        || !transaction.stage.starts_with(".bajptr-export-")
        || transaction
            .files
            .iter()
            .any(|file| !storage::safe_name(&file.name) || !file.name.ends_with(".tsv"))
    {
        return Err(invalid("导出恢复记录格式无效，请保留输出目录并检查日志。"));
    }
    let stage = target.join(&transaction.stage);
    if fs::symlink_metadata(&stage)?.file_type().is_symlink() {
        return Err(invalid("导出恢复位置不是普通暂存目录，未改动文件。"));
    }
    let mut names = transaction
        .files
        .iter()
        .map(|file| &file.name)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    let _locks = names
        .into_iter()
        .map(|name| storage::Lock::acquire(&target.join(name)))
        .collect::<Result<Vec<_>>>()?;
    rollback(target, &stage, &transaction)?;
    fs::remove_dir_all(stage)?;
    Ok(())
}

/// Generate every file before publication; retain backups until the whole batch commits.
pub fn export(
    target: &Path,
    control: &Control<'_>,
    generate: impl FnOnce(&Path) -> Result<u64>,
) -> Result<u64> {
    fs::create_dir_all(target)?;
    let _lock = storage::Lock::acquire(&target.join(JOURNAL))?;
    recover(target).map_err(|source| Error::Context {
        message: format!(
            "上次导出尚未恢复完成，请暂勿使用该目录的导出文件。恢复材料保留在 {}，请检查日志。",
            target.display()
        ),
        source: Box::new(source),
    })?;
    let stage = tempfile::Builder::new()
        .prefix(".bajptr-export-")
        .tempdir_in(target)?;
    let generated = stage.path().join("new");
    let previous = stage.path().join("previous");
    fs::create_dir_all(&generated)?;
    fs::create_dir_all(&previous)?;
    let count = generate(&generated)?;
    let mut paths = fs::read_dir(&generated)?
        .map(|entry| Ok(entry?.path()))
        .collect::<Result<Vec<PathBuf>>>()?;
    paths.sort();
    let mut locks = Vec::new();
    let mut files = Vec::new();
    for source in &paths {
        control.check()?;
        let name = source.file_name().unwrap().to_string_lossy().to_string();
        if !storage::safe_name(&name) || !name.ends_with(".tsv") || !source.is_file() {
            return Err(invalid("导出文件名称或类型无效。"));
        }
        let destination = target.join(&name);
        locks.push(storage::Lock::acquire(&destination)?);
        let before = storage::revision(&destination)?;
        if before.is_some() {
            baseline::copy_sync(&destination, &previous.join(&name))?;
        }
        files.push(ExportFile {
            name,
            before,
            after: storage::revision(source)?.unwrap(),
        });
    }
    control.log("正在保存导出文件…")?;
    let transaction = ExportTransaction {
        stage: stage
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        files,
    };
    storage::write_locked(&target.join(JOURNAL), &serde_json::to_vec(&transaction)?)?;
    let outcome = (|| -> Result<()> {
        for file in &transaction.files {
            let destination = target.join(&file.name);
            storage::check_revision(&destination, &file.before)?;
            let pending = storage::temporary_path(&destination)?;
            baseline::copy_sync(&generated.join(&file.name), &pending)?;
            storage::replace(&pending, &destination)?;
        }
        fs::remove_file(target.join(JOURNAL))?;
        Ok(())
    })();
    if let Err(source) = outcome {
        if let Err(rollback_error) = rollback(target, stage.path(), &transaction) {
            let retained = stage.keep();
            return Err(Error::Context {
                message: format!(
                    "导出未完成，部分文件无法回滚。请勿使用当前导出结果；恢复副本保留在 {}。关闭占用文件的程序后向同一目录重新导出，可先尝试恢复。",
                    retained.display()
                ),
                source: Box::new(Error::Context {
                    message: format!("导出失败：{source}"),
                    source: Box::new(rollback_error),
                }),
            });
        }
        return Err(Error::Context {
            message: format!(
                "导出失败，已撤回本次写入，原有文件保留。{} 详细原因见日志。",
                crate::diagnostics::user_message("export_text", &source)
            ),
            source: Box::new(source),
        });
    }
    if let Err(error) = stage.close() {
        let _ = control.warn(&format!("导出已完成，暂存目录未清理：{error}"));
    }
    Ok(count)
}
