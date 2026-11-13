use crate::{Result, require};
use std::{
    ffi::OsString,
    fs,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_BAD_LENGTH, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, GetFileAttributesW},
    System::{Diagnostics::ToolHelp::*, Threading::*},
    UI::Shell::*,
};

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn from_wide(buffer: &[u16]) -> PathBuf {
    OsString::from_wide(&buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())])
        .into()
}

pub fn is_reparse(path: &Path) -> Result<bool> {
    let path = wide(path);
    // SAFETY: path is a live, NUL-terminated UTF-16 buffer.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == u32::MAX {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

struct Snapshot(HANDLE);
impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: the snapshot owns a valid handle created below, closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn snapshot(flags: u32, pid: u32) -> Result<Snapshot> {
    for _ in 0..8 {
        // SAFETY: flags and PID are passed by value; successful handle ownership transfers here.
        let handle = unsafe { CreateToolhelp32Snapshot(flags, pid) };
        if handle != INVALID_HANDLE_VALUE {
            return Ok(Snapshot(handle));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_BAD_LENGTH as i32) {
            return Err(error.into());
        }
    }
    Err("Process module list kept changing; could not capture it.".into())
}

pub struct EngineProcess {
    pub pid: u32,
    pub path: PathBuf,
}

pub fn engines() -> Result<Vec<EngineProcess>> {
    let snapshot = snapshot(TH32CS_SNAPPROCESS, 0)?;
    // SAFETY: zero is a valid initial value for this plain Windows C struct.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut result = Vec::new();
    // SAFETY: entry points to an initialized writable struct with its size set.
    let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while available != 0 {
        let name = from_wide(&entry.szExeFile);
        if ["bajptr-desktop.exe", "bajptr-rs.exe"]
            .iter()
            .any(|n| name.as_os_str().eq_ignore_ascii_case(n))
        {
            let handle = open_process(entry.th32ProcessID, PROCESS_QUERY_LIMITED_INFORMATION)?;
            let mut buffer = vec![0u16; 32768];
            let mut length = buffer.len() as u32;
            // SAFETY: handle is live; buffer/length are writable and describe the allocated UTF-16 capacity.
            let ok = unsafe {
                QueryFullProcessImageNameW(
                    handle.as_raw_handle(),
                    0,
                    buffer.as_mut_ptr(),
                    &mut length,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            result.push(EngineProcess {
                pid: entry.th32ProcessID,
                path: OsString::from_wide(&buffer[..length as usize]).into(),
            });
        }
        // SAFETY: same snapshot and correctly sized writable entry remain live.
        available = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    require(
        error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32),
        format!("Process enumeration failed: {error}"),
    )?;
    Ok(result)
}

fn open_process(pid: u32, access: u32) -> Result<OwnedHandle> {
    // SAFETY: scalar arguments; we check failure before adopting sole ownership of the handle.
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: OpenProcess returned a valid owned handle, with no other owner.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub fn modules(pid: u32) -> Result<Vec<PathBuf>> {
    let snapshot = snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?;
    // SAFETY: zero is a valid initial value for this plain Windows C struct.
    let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
    let mut paths = Vec::new();
    // SAFETY: snapshot is live; entry is writable and declares the correct size.
    let mut available = unsafe { Module32FirstW(snapshot.0, &mut entry) };
    while available != 0 {
        paths.push(from_wide(&entry.szExePath));
        // SAFETY: same live snapshot and writable entry.
        available = unsafe { Module32NextW(snapshot.0, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    require(
        error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32),
        format!("Module enumeration failed: {error}"),
    )?;
    Ok(paths)
}

pub struct ObservedProcess(OwnedHandle);
impl ObservedProcess {
    pub fn open(pid: u32) -> Result<Self> {
        Ok(Self(open_process(
            pid,
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        )?))
    }

    pub fn wait(&self, milliseconds: u32) -> Result {
        // SAFETY: the owned process handle remains live throughout the wait.
        require(
            unsafe { WaitForSingleObject(self.0.as_raw_handle(), milliseconds) } == 0,
            "GUI probe did not stop.",
        )
    }
}
impl Drop for ObservedProcess {
    fn drop(&mut self) {
        // SAFETY: this owned handle identifies the observed isolated probe, never a reused PID.
        unsafe {
            if WaitForSingleObject(self.0.as_raw_handle(), 0) == 258 {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), 5000);
            }
        }
    }
}

fn recycle(path: &Path) -> Result {
    let absolute = std::path::absolute(path)?;
    let value = absolute.to_string_lossy();
    // Shell file operations reject the extended prefix returned by canonicalize.
    let ordinary = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
    };
    let mut path = wide(Path::new(&ordinary));
    path.push(0);
    // SAFETY: zero initializes optional pointers/flags, subsequently set below.
    let mut operation: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    operation.wFunc = FO_DELETE;
    operation.pFrom = path.as_ptr();
    operation.fFlags =
        FOF_ALLOWUNDO as u16 | FOF_NOCONFIRMATION as u16 | FOF_NOERRORUI as u16 | FOF_SILENT as u16;
    // SAFETY: pFrom is a live double-NUL-terminated list; no other pointer is required for FO_DELETE.
    let code = unsafe { SHFileOperationW(&mut operation) };
    require(
        code == 0 && operation.fAnyOperationsAborted == 0,
        format!("Recycle Bin operation failed ({code})."),
    )
}

pub fn inside(path: &Path, parent: &Path) -> bool {
    let normalize = |p: &Path| {
        let value = p.to_string_lossy().replace('/', "\\").to_lowercase();
        if let Some(unc) = value.strip_prefix(r"\\?\unc\") {
            format!(r"\\{unc}")
        } else {
            value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
        }
    };
    let path = normalize(path);
    let parent = normalize(parent);
    path == parent || path.starts_with(&format!("{}\\", parent.trim_end_matches('\\')))
}

pub fn retire(source: &Path) -> Result {
    if let Some(trash) = std::env::var_os("BAJPTR_TRASH_DIR") {
        let attempt = (|| -> Result {
            let trash = std::path::absolute(Path::new(&trash))?;
            fs::create_dir_all(&trash)?;
            let trash = fs::canonicalize(&trash)?;
            let versions =
                fs::canonicalize(source.parent().ok_or("Invalid native version parent.")?)?;
            require(
                !inside(&trash, &versions),
                "Trash folder is inside native versions.",
            )?;
            let target = tempfile::Builder::new()
                .prefix("native-")
                .tempdir_in(&trash)?
                .keep();
            // The caller has verified that source contains one plain, hash-matching engine.
            let entries = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
            require(
                entries.len() == 1,
                "Native version changed before retirement.",
            )?;
            let original = entries[0].path();
            let copy = target.join(entries[0].file_name());
            fs::copy(&original, &copy)?;
            require(
                crate::files::digest(&original)? == crate::files::digest(&copy)?,
                "Archived native binary differs.",
            )?;
            require(
                inside(&fs::canonicalize(&target)?, &trash),
                "Archive escaped trash directory.",
            )?;
            fs::remove_file(&original)?;
            fs::remove_dir(source)?;
            require(
                !source.exists() && copy.is_file(),
                "Native retirement did not complete.",
            )
        })();
        if attempt.is_ok() {
            return Ok(());
        }
        eprintln!(
            "Warning: native archive failed; trying Recycle Bin: {}",
            attempt.unwrap_err()
        );
    }
    recycle(source)?;
    require(!source.exists(), "Retired native directory remains.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_containment_checks_directory_boundaries_and_windows_prefixes() {
        assert!(inside(
            Path::new(r"\\?\D:\包\native\a"),
            Path::new(r"d:\包")
        ));
        assert!(!inside(Path::new(r"D:\包副本\a"), Path::new(r"D:\包")));
        assert!(inside(
            Path::new(r"\\?\UNC\host\share\包\a"),
            Path::new(r"\\host\share\包")
        ));
    }

    #[test]
    fn recycle_handles_canonical_unicode_directories() -> Result {
        let temp = tempfile::tempdir()?;
        let directory = temp.path().join("Rust工具回收站验证");
        fs::create_dir(&directory)?;
        fs::write(directory.join("fixture.txt"), b"recycle fixture")?;
        recycle(&fs::canonicalize(&directory)?)?;
        assert!(!directory.exists());
        Ok(())
    }
}
