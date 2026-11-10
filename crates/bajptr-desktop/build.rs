use std::{env, path::PathBuf, process::Command};

fn command_text(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    bajptr_brand::embed_windows_icon();
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    for path in [
        "crates",
        "Cargo.toml",
        "Cargo.lock",
        ".git/HEAD",
        ".git/refs/heads",
    ] {
        let path = root.join(path);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let built_at = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y-%m-%d %H:%M UTC+8");
    println!("cargo:rustc-env=BAJPTR_BUILD_TIME={built_at}");
    for (key, value) in [
        ("TARGET", "BAJPTR_BUILD_TARGET"),
        ("PROFILE", "BAJPTR_BUILD_PROFILE"),
    ] {
        println!("cargo:rustc-env={value}={}", env::var(key).unwrap());
    }
    let rustc = command_text(Command::new(env::var_os("RUSTC").unwrap()).arg("--version"))
        .unwrap_or_default();
    println!("cargo:rustc-env=BAJPTR_BUILD_RUSTC={rustc}");
    let commit = command_text(Command::new("git").arg("-C").arg(&root).args([
        "describe",
        "--always",
        "--dirty=+modified",
    ]))
    .unwrap_or_default();
    println!("cargo:rustc-env=BAJPTR_BUILD_COMMIT={commit}");
}
