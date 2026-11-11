//! Successful scratch work is retired independently of the interactive process.
//! Failed work stays in place for diagnosis. The helper accepts only marked scratch.
use crate::{Control, Result, invalid, storage};
use std::{fs, path::Path};
const MARKER: &str = ".bajptr-scratch";
pub fn mark(path: &Path) -> Result<()> {
    storage::write_locked(
        &path.join(MARKER),
        uuid::Uuid::new_v4().simple().to_string().as_bytes(),
    )?;
    Ok(())
}
pub fn handle_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if !args.get(1).is_some_and(|s| s == "--retire-scratch") {
        return None;
    }
    let result = (|| -> Result<()> {
        if args.len() != 4 {
            return Err(invalid("清理参数无效。"));
        }
        let path = Path::new(&args[2]);
        let token = args[3].to_string_lossy();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !path.is_absolute()
            || !(name.starts_with("patch-") || name.starts_with("bajptr-standalone-"))
            || token.len() != 32
            || !token.bytes().all(|c| c.is_ascii_hexdigit())
            || fs::symlink_metadata(path)?.file_type().is_symlink()
            || fs::read(path.join(MARKER))? != token.as_bytes()
        {
            return Err(invalid("未接受未标记的清理目录。"));
        }
        storage::retire(path)?;
        Ok(())
    })();
    if let Err(error) = &result {
        eprintln!("{error}");
    }
    Some(if result.is_ok() { 0 } else { 1 })
}
pub fn retire_success(path: &Path, control: &Control<'_>) -> bool {
    let spawn = (|| -> Result<bool> {
        let exe = std::env::current_exe()?;
        // Test harnesses retire synchronously, making cleanup assertions deterministic.
        if !matches!(
            exe.file_stem().and_then(|s| s.to_str()),
            Some("bajptr-desktop" | "bajptr-rs")
        ) {
            return Ok(false);
        }
        let token = fs::read_to_string(path.join(MARKER))?;
        let mut command = std::process::Command::new(exe);
        command
            .arg("--retire-scratch")
            .arg(storage::resolved(path)?)
            .arg(token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command.spawn()?;
        Ok(true)
    })();
    match spawn {
        Ok(true) => true,
        _ => {
            if let Err(e) = storage::retire(path) {
                let _ = control.warn(&e.to_string());
            }
            false
        }
    }
}
