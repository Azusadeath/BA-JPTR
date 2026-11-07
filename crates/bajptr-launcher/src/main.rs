#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    if let Err(error) = launch() {
        #[cfg(windows)]
        unsafe {
            let message: Vec<u16> = format!("无法启动工具：{error}\0").encode_utf16().collect();
            let title: Vec<u16> = "蔚蓝档案 · 日服译名替换\0".encode_utf16().collect();
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                0x10,
            );
        }
        #[cfg(not(windows))]
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn launch() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_exe()?
        .parent()
        .ok_or("启动器目录无效")?
        .to_path_buf();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("native/engine.json"))?)?;
    let relative = std::path::Path::new(
        manifest["desktop"]
            .as_str()
            .ok_or("缺少桌面程序，请重新构建或解压完整发行包")?,
    );
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("桌面程序路径无效".into());
    }
    let program = root.join("native").join(relative);
    let mut command = std::process::Command::new(program);
    command
        .arg("--project-dir")
        .arg(&root)
        .args(std::env::args_os().skip(1))
        .current_dir(&root);
    #[cfg(windows)]
    {
        // The launcher is a GUI executable, so the desktop inherits no minimized console.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn()?;
    Ok(())
}
