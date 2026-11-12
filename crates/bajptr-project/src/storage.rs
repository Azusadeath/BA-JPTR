//! Cross-instance exclusion, compare-and-swap saves and durable atomic publication.
use crate::{Result, invalid};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn validate_data_dir(data: &Path) -> Result<()> {
    let resolved = resolved(data)?;
    if resolved.is_file() {
        return Err(invalid(format!(
            "--data-dir 必须是数据目录，收到文件：{}",
            data.display()
        )));
    }
    let is_gui = resolved
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(".gui"));
    let contains_gui_files = !resolved.join(".gui").is_dir()
        && [
            "settings.json",
            "rules.tsv",
            "text-edits.json",
            "source-history.json",
            "template-drafts.json",
            "application-state.json",
            "tables-cache.json",
        ]
        .iter()
        .any(|name| resolved.join(name).is_file());
    if is_gui || contains_gui_files {
        return Err(invalid(format!(
            "--data-dir 应指向包含 .gui 的父目录。当前目录：{}。请将设置、词表等文件放在数据目录下的 .gui 文件夹中，再传入其父目录。",
            data.display()
        )));
    }
    Ok(())
}
pub fn resolved(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let path = if path.exists() {
        fs::canonicalize(&path)?
    } else {
        match (path.parent(), path.components().next_back()) {
            (Some(parent), Some(Component::Normal(name))) => resolved(parent)?.join(name),
            (Some(parent), Some(Component::ParentDir)) => {
                let parent = resolved(parent)?;
                parent.parent().unwrap_or(&parent).to_path_buf()
            }
            // A missing drive or share is still a valid saved setting. The
            // connection check reports it; project loading must remain usable.
            _ => path,
        }
    };
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        let value = if let Some(s) = value.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{s}")
        } else {
            value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
        };
        Ok(PathBuf::from(value))
    }
    #[cfg(not(windows))]
    {
        Ok(path)
    }
}
pub fn identity(path: &Path) -> Result<String> {
    let value = resolved(path)?.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        Ok(value.replace('/', "\\").to_lowercase())
    }
    #[cfg(not(windows))]
    {
        Ok(value)
    }
}
pub fn revision(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(digest(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub struct Lock {
    _file: File,
}
impl Lock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let root = std::env::temp_dir().join("bajptr-locks");
        fs::create_dir_all(&root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("{}.lock", digest(identity(path)?.as_bytes()))))?;
        file.try_lock_exclusive().map_err(lock_error)?;
        Ok(Self { _file: file })
    }
}
fn lock_error(error: std::io::Error) -> crate::Error {
    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
        invalid("另一个工具实例正在处理这些文件，请稍后重试。")
    } else {
        error.into()
    }
}

pub fn temporary_path(target: &Path) -> Result<tempfile::TempPath> {
    let parent = target.parent().ok_or_else(|| invalid("保存路径无效。"))?;
    Ok(tempfile::Builder::new()
        .prefix(&format!(
            "{}.pending-",
            target.file_name().unwrap_or_default().to_string_lossy()
        ))
        .tempfile_in(parent)?
        .into_temp_path())
}
pub fn replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let extended = |path: &Path| -> Result<Vec<u16>> {
            let absolute = resolved(path)?.to_string_lossy().into_owned();
            let name = if let Some(unc) = absolute.strip_prefix(r"\\") {
                format!(r"\\?\UNC\{unc}")
            } else {
                format!(r"\\?\{absolute}")
            };
            Ok(std::ffi::OsStr::new(&name)
                .encode_wide()
                .chain(Some(0))
                .collect())
        };
        let a = extended(source)?;
        let b = extended(destination)?;
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination)?;
        Ok(())
    }
}
pub fn check_revision(path: &Path, expected: &Option<String>) -> Result<()> {
    if &revision(path)? != expected {
        return Err(invalid(format!(
            "{} 已被其他程序修改，未覆盖原文件。请重新打开工具。",
            path.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    Ok(())
}
/// Caller holds the corresponding file lock across validation and publication.
pub fn write_locked(path: &Path, bytes: &[u8]) -> Result<String> {
    fs::create_dir_all(path.parent().ok_or_else(|| invalid("保存路径无效。"))?)?;
    let pending = temporary_path(path)?;
    let mut stream = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&pending)?;
    stream.write_all(bytes)?;
    stream.sync_all()?;
    drop(stream);
    replace(&pending, path)?;
    Ok(digest(bytes))
}
pub fn atomic_write(
    path: &Path,
    bytes: &[u8],
    expected: Option<&Option<String>>,
) -> Result<String> {
    let _lock = Lock::acquire(path)?;
    if let Some(expected) = expected {
        check_revision(path, expected)?;
    }
    write_locked(path, bytes)
}
#[derive(Default)]
pub struct Store {
    pub root: PathBuf,
    revisions: HashMap<String, Option<String>>,
    failed: Vec<String>,
}
const SOURCES_TRANSACTION: &str = "sources-transaction.json";
#[derive(Serialize, Deserialize)]
struct SourceWrite {
    name: String,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}
fn source_locks(root: &Path) -> Result<Vec<Lock>> {
    ["rules.tsv", "source-history.json"]
        .iter()
        .map(|name| Lock::acquire(&root.join(name)))
        .collect()
}
fn rollback_sources(root: &Path, writes: &[SourceWrite]) -> Result<()> {
    for write in writes {
        let current = revision(&root.join(&write.name))?;
        let before = write.before.as_deref().map(digest);
        if current != before && current != Some(digest(&write.after)) {
            return Err(invalid(
                "来源表保存中断后文件又被修改，请保留 sources-transaction.json 并检查数据目录，未覆盖文件。",
            ));
        }
    }
    for write in writes {
        let path = root.join(&write.name);
        if revision(&path)? == write.before.as_deref().map(digest) {
            continue;
        }
        if let Some(bytes) = &write.before {
            write_locked(&path, bytes)?;
        } else {
            let archived = root.join(format!(
                "source-sync-aborted-{}-{}",
                uuid::Uuid::new_v4().simple(),
                write.name
            ));
            fs::rename(&path, archived)?;
        }
    }
    fs::remove_file(root.join(SOURCES_TRANSACTION))?;
    Ok(())
}
pub fn recover_sources(root: &Path) -> Result<()> {
    let _locks = source_locks(root)?;
    let bytes = match fs::read(root.join(SOURCES_TRANSACTION)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let writes: Vec<SourceWrite> = serde_json::from_slice(&bytes)?;
    if writes.len() != 2 || writes[0].name != "rules.tsv" || writes[1].name != "source-history.json"
    {
        return Err(invalid("来源表事务记录格式无效，未覆盖原文件。"));
    }
    rollback_sources(root, &writes)
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Self::default()
        }
    }
    pub fn load<T>(
        &mut self,
        name: &str,
        parse: impl FnOnce(&[u8]) -> Result<T>,
        default: T,
    ) -> Result<T> {
        let path = self.root.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                self.failed.push(name.into());
                return Err(e.into());
            }
        };
        self.revisions
            .insert(name.into(), bytes.as_ref().map(|b| digest(b)));
        match bytes {
            None => Ok(default),
            Some(bytes) => parse(&bytes).inspect_err(|_| self.failed.push(name.into())),
        }
    }
    pub fn save(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        if self.failed.iter().any(|n| n == name) {
            return Err(invalid(
                "原文件未能完整读取，已停止保存。请修复后重新打开。",
            ));
        }
        let expected = self
            .revisions
            .get(name)
            .ok_or_else(|| invalid("请先读取修改记录。"))?;
        let revision = atomic_write(&self.root.join(name), bytes, Some(expected))?;
        self.revisions.insert(name.into(), Some(revision));
        Ok(())
    }
    pub fn check(&self, name: &str) -> Result<()> {
        check_revision(
            &self.root.join(name),
            self.revisions
                .get(name)
                .ok_or_else(|| invalid("修改记录尚未读取。"))?,
        )
    }
    pub fn save_sources(&mut self, rules: &[u8], history: &[u8]) -> Result<()> {
        let _locks = source_locks(&self.root)?;
        if self.root.join(SOURCES_TRANSACTION).exists() {
            return Err(invalid(
                "来源表存在未完成的保存，请重新打开工具恢复后重试。",
            ));
        }
        let mut writes = Vec::new();
        for (name, bytes) in [("rules.tsv", rules), ("source-history.json", history)] {
            if self.failed.iter().any(|failed| failed == name) {
                return Err(invalid("来源表记录读取失败，未保存任何变更。"));
            }
            self.check(name)?;
            let before = match fs::read(self.root.join(name)) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            writes.push(SourceWrite {
                name: name.into(),
                before,
                after: bytes.to_vec(),
            });
        }
        write_locked(
            &self.root.join(SOURCES_TRANSACTION),
            &serde_json::to_vec(&writes)?,
        )?;
        let result = (|| -> Result<()> {
            for write in &writes {
                write_locked(&self.root.join(&write.name), &write.after)?;
            }
            fs::remove_file(self.root.join(SOURCES_TRANSACTION))?;
            Ok(())
        })();
        if let Err(error) = result {
            if let Err(rollback) = rollback_sources(&self.root, &writes) {
                self.failed
                    .extend(writes.iter().map(|write| write.name.clone()));
                return Err(invalid(format!(
                    "来源表保存失败：{error}；回滚未完成：{rollback}。请重新打开工具恢复。"
                )));
            }
            return Err(error);
        }
        for write in writes {
            self.revisions
                .insert(write.name, Some(digest(&write.after)));
        }
        Ok(())
    }
    pub fn archive_failed(&mut self, name: &str) -> Result<PathBuf> {
        if !["rules.tsv", "text-edits.json", "source-history.json"].contains(&name) {
            return Err(invalid("这类记录不支持重置，请修复原文件后重新打开工具。"));
        }
        if !self.failed.iter().any(|failed| failed == name)
            || !["rules.tsv", "text-edits.json", "source-history.json"].contains(&name)
        {
            return Err(invalid("这份记录没有读取错误，未进行重置。"));
        }
        let path = self.root.join(name);
        let _lock = Lock::acquire(&path)?;
        self.check(name)?;
        let recovery = self
            .root
            .join("recovery")
            .join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&recovery)?;
        let archived = recovery.join(name);
        fs::rename(&path, &archived)?;
        self.failed.retain(|failed| failed != name);
        self.revisions.insert(name.into(), None);
        Ok(archived)
    }
}
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| "<>:\"/\\|?*".contains(c) || c.is_control())
        && Path::new(name)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
        && name != "."
        && name != ".."
}
pub fn retire(source: &Path) -> Result<()> {
    retire_to(
        source,
        std::env::var_os("BAJPTR_TRASH_DIR")
            .as_deref()
            .map(Path::new),
    )
}
/// An optional explicit destination is useful for automated tests. Ordinary
/// releases use the OS Recycle Bin and never assume a user's drive letters.
pub fn retire_to(source: &Path, destination: Option<&Path>) -> Result<()> {
    let source = resolved(source)?;
    let moved = (|| -> Result<()> {
        let root = destination.ok_or_else(|| invalid("没有指定移出目录。"))?;
        fs::create_dir_all(root)?;
        let root = resolved(root)?;
        let target = root.join(format!(
            "bajptr-{}-{}",
            source.file_name().unwrap_or_default().to_string_lossy(),
            uuid::Uuid::new_v4().simple()
        ));
        if fs::rename(&source, &target).is_err() {
            #[cfg(windows)]
            {
                shell_file_operation(&source, Some(&target), 1)?;
            }
            #[cfg(not(windows))]
            {
                fs::rename(&source, &target)?;
            }
        }
        if source.exists()
            || !target.exists()
            || resolved(&target)?.parent() != Some(root.as_path())
        {
            return Err(invalid("移出后路径检查失败。"));
        }
        Ok(())
    })();
    if moved.is_ok() {
        return moved;
    }
    #[cfg(windows)]
    {
        shell_file_operation(&source, None, 3)?;
        if source.exists() {
            return Err(invalid("移到回收站后检查失败。"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        moved
    }
}
#[cfg(windows)]
fn shell_file_operation(source: &Path, target: Option<&Path>, operation: u32) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let from: Vec<u16> = source.as_os_str().encode_wide().chain([0, 0]).collect();
    let to: Vec<u16> = target
        .map(|p| p.as_os_str().encode_wide().chain([0, 0]).collect())
        .unwrap_or_else(|| vec![0, 0]);
    let mut args = windows_sys::Win32::UI::Shell::SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: operation,
        pFrom: from.as_ptr(),
        pTo: to.as_ptr(),
        fFlags: 0x10 | 0x4 | 0x400 | 0x200 | if operation == 3 { 0x40 } else { 0 },
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let result = unsafe { windows_sys::Win32::UI::Shell::SHFileOperationW(&mut args) };
    if result != 0 || args.fAnyOperationsAborted != 0 {
        return Err(invalid(format!(
            "临时内容暂未移出：{}（系统返回 {result}）",
            source.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    #[test]
    fn lock_errors_distinguish_contention_from_permission_and_other_failures() {
        assert!(
            lock_error(fs2::lock_contended_error())
                .to_string()
                .contains("另一个工具实例")
        );
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidInput,
        ] {
            let error = lock_error(std::io::Error::new(kind, "underlying lock failure"));
            assert!(matches!(error, crate::Error::Io(_)));
            assert!(
                !crate::diagnostics::user_message("save_rule", &error).contains("另一个工具实例")
            );
        }
    }
}
