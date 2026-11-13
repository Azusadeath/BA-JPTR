use crate::{
    CargoOptions, Result,
    files::{self, Manifest},
    native, require,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn cargo(root: &Path) -> Result<Command> {
    let tools = root.join(".toolchain");
    let executable = tools.join("cargo/bin/cargo.exe");
    let mut command = Command::new(if executable.exists() {
        executable.as_os_str()
    } else {
        std::ffi::OsStr::new("cargo")
    });
    command.current_dir(root);
    if executable.exists() {
        let runtime = tools.join(
            "rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin",
        );
        let linker = runtime.join("self-contained");
        let mut paths = vec![
            linker.clone(),
            tools.join("cargo/bin"),
            runtime,
            tools.join("w64devkit/bin"),
        ];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command
            .env("CARGO_HOME", tools.join("cargo"))
            .env("RUSTUP_HOME", tools.join("rustup"))
            .env("PATH", std::env::join_paths(paths)?)
            .env(
                "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER",
                linker.join("x86_64-w64-mingw32-gcc.exe"),
            )
            .env(
                "RUSTFLAGS",
                format!(
                    "-C dlltool={}",
                    tools
                        .join("w64devkit")
                        .join("bin")
                        .join("dlltool.exe")
                        .display()
                ),
            )
            .env("CC", tools.join("w64devkit/bin/gcc.exe"))
            .env("AR", tools.join("w64devkit/bin/ar.exe"))
            .env(
                "WINDRES",
                tools.join("w64devkit/bin/x86_64-w64-mingw32-windres.exe"),
            );
    }
    Ok(command)
}

fn execute(command: &mut Command) -> Result {
    let status = command.status()?;
    require(status.success(), format!("Cargo failed: {status}"))
}

pub fn cargo_task(
    root: &Path,
    task: &str,
    options: &CargoOptions,
    test: Option<&str>,
    filter: Option<&str>,
) -> Result {
    let mut command = cargo(root)?;
    command.arg(task);
    if let Some(package) = &options.package {
        require(
            task != "build" || package != "xtask",
            "Cannot replace the running xtask executable; use cargo build -p xtask separately.",
        )?;
        command.args(["--package", package]);
    } else {
        command.arg("--workspace");
        if task == "build" {
            command.args(["--exclude", "xtask"]);
        }
    }
    if !options.update_lock {
        command.arg("--locked");
    }
    if !options.debug {
        command.arg("--release");
    }
    if let Some(test) = test {
        command.args(["--test", test]);
    }
    if let Some(filter) = filter {
        command.args(["--", filter]);
    }
    execute(&mut command)
}

pub fn fmt(root: &Path, check: bool) -> Result {
    let mut command = cargo(root)?;
    command.args(["fmt", "--all"]);
    if check {
        command.args(["--", "--check"]);
    }
    execute(&mut command)
}

pub fn lint(root: &Path, fix: bool) -> Result {
    let mut command = cargo(root)?;
    command.args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--release",
    ]);
    if fix {
        command.args(["--fix", "--allow-dirty", "--allow-staged"]);
    } else {
        command.args(["--", "-D", "warnings"]);
    }
    execute(&mut command)
}

pub fn metadata(root: &Path) -> Result<Vec<u8>> {
    let output = cargo(root)?
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            "x86_64-pc-windows-gnu",
        ])
        .output()?;
    require(
        output.status.success(),
        format!(
            "Cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(output.stdout)
}

pub fn publish(root: &Path, debug: bool) -> Result {
    let binaries = root.join(if debug {
        "target/debug"
    } else {
        "target/release"
    });
    let native = root.join("native");
    let previous = if native.join("engine.json").exists() {
        Some(Manifest::read(&native)?)
    } else {
        None
    };
    let mut paths = Vec::new();
    for name in ["bajptr-desktop.exe", "bajptr-rs.exe"] {
        let source = binaries.join(name);
        let digest = files::digest(&source)?;
        let relative = format!("versions/{}/{name}", &digest[..20]);
        let target = native.join(&relative);
        if target.exists() {
            require(
                files::digest(&target)? == digest,
                "An existing native engine does not match its hash.",
            )?;
        } else {
            files::copy(&source, &target)?;
        }
        paths.push(relative);
    }
    let manifest = Manifest {
        protocol: 1,
        desktop: paths[0].clone(),
        cli: paths[1].clone(),
    };
    if let Some(previous) = previous.filter(|p| p != &manifest) {
        files::atomic_write(
            &native.join("previous-engine.json"),
            &serde_json::to_vec_pretty(&previous)?,
        )?;
    }
    files::atomic_write(
        &native.join("engine.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    files::copy(
        &binaries.join("bajptr-launcher.exe"),
        &root.join("BA-JPTR.exe"),
    )?;
    println!("Published: {}", native.join("engine.json").display());
    if let Err(error) = retire_old(&native, &manifest) {
        eprintln!("Warning: kept old native engines: {error}");
    }
    Ok(())
}

fn retire_old(native: &Path, manifest: &Manifest) -> Result {
    let versions = fs::canonicalize(native.join("versions"))?;
    let mut keep = HashSet::new();
    let mut retain = |manifest: &Manifest| -> Result {
        manifest.validate()?;
        for path in [&manifest.desktop, &manifest.cli] {
            keep.insert(path.split('/').nth(1).unwrap().to_string());
        }
        Ok(())
    };
    retain(manifest)?;
    if native.join("previous-engine.json").exists() {
        retain(&serde_json::from_slice(&fs::read(
            native.join("previous-engine.json"),
        )?)?)?;
    }
    for process in native::engines()? {
        if let Some(folder) = process.path.parent()
            && folder
                .parent()
                .is_some_and(|p| native::inside(p, &versions) && native::inside(&versions, p))
        {
            keep.insert(
                folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    for candidate in retirement_candidates(&versions, &keep)? {
        if let Err(error) = native::retire(&candidate) {
            eprintln!("Warning: kept native {}: {error}", candidate.display());
        }
    }
    Ok(())
}

fn retirement_candidates(versions: &Path, keep: &HashSet<String>) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(versions)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.contains(&name)
            || !files::is_digest(&name)
            || !entry.file_type()?.is_dir()
            || native::is_reparse(&entry.path())?
        {
            continue;
        }
        let contents = fs::read_dir(entry.path())?.collect::<std::io::Result<Vec<_>>>()?;
        if contents.len() != 1 {
            continue;
        }
        let file = &contents[0];
        if !file.file_type()?.is_file()
            || native::is_reparse(&file.path())?
            || !["bajptr-desktop.exe", "bajptr-rs.exe"]
                .iter()
                .any(|n| file.file_name() == *n)
            || !files::digest(&file.path())?.starts_with(&name)
        {
            continue;
        }
        candidates.push(entry.path());
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cleanup_preserves_retained_tampered_and_unrecognized_contents() -> Result {
        let temp = tempfile::tempdir()?;
        let versions = temp.path();
        let mut keep = HashSet::new();
        let mut retired = None;
        for (label, bytes) in [
            ("current", b"current".as_slice()),
            ("previous", b"previous"),
            ("running", b"running"),
            ("old", b"old"),
            ("extra", b"extra"),
        ] {
            let staging = versions.join("fixture");
            fs::write(&staging, bytes)?;
            let name = files::digest(&staging)?[..20].to_string();
            let folder = versions.join(&name);
            fs::create_dir_all(&folder)?;
            fs::rename(&staging, folder.join("bajptr-rs.exe"))?;
            if ["current", "previous", "running"].contains(&label) {
                keep.insert(name);
            }
            if label == "old" {
                retired = Some(folder.clone());
            }
            if label == "extra" {
                fs::write(folder.join("user-data.txt"), b"preserve")?;
            }
        }
        files::write(
            &versions.join("01234567890123456789/bajptr-rs.exe"),
            b"tampered",
        )?;
        files::write(&versions.join("user-folder/bajptr-rs.exe"), b"unrecognized")?;
        assert_eq!(
            retirement_candidates(versions, &keep)?,
            vec![retired.unwrap()]
        );
        Ok(())
    }
}
