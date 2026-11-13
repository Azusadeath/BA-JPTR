use crate::{Result, require};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Manifest {
    pub protocol: u32,
    pub desktop: String,
    pub cli: String,
}

impl Manifest {
    pub fn read(native: &Path) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(&fs::read(native.join("engine.json"))?)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result {
        require(self.protocol == 1, "Unsupported engine protocol.")?;
        for (relative, name) in [
            (&self.desktop, "bajptr-desktop.exe"),
            (&self.cli, "bajptr-rs.exe"),
        ] {
            let path = Path::new(relative);
            let parts: Vec<_> = relative.split('/').collect();
            require(
                path.components().all(|c| matches!(c, Component::Normal(_)))
                    && parts.len() == 3
                    && parts[0] == "versions"
                    && is_digest(parts[1])
                    && parts[2] == name,
                format!("Invalid binary manifest path: {relative}"),
            )?;
        }
        Ok(())
    }
}

pub fn is_digest(value: &str) -> bool {
    value.len() == 20
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub fn digest(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn write(path: &Path, bytes: &[u8]) -> Result {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result {
    let parent = path.parent().ok_or("Missing output parent.")?;
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending.persist(path)?;
    Ok(())
}

pub fn copy(source: &Path, target: &Path) -> Result {
    fs::create_dir_all(target.parent().ok_or("Missing copy target parent.")?)?;
    fs::copy(source, target)?;
    Ok(())
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result {
    for entry in WalkDir::new(source).sort_by_file_name() {
        let entry = entry?;
        require(
            !entry.file_type().is_symlink() && !crate::native::is_reparse(entry.path())?,
            format!("Refusing linked source: {}", entry.path().display()),
        )?;
        let target = destination.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            copy(entry.path(), &target)?;
        } else {
            return Err(format!("Unsupported file: {}", entry.path().display()).into());
        }
    }
    Ok(())
}

pub fn empty_destination(destination: &Path) -> Result {
    if destination.exists() {
        require(
            destination.is_dir() && fs::read_dir(destination)?.next().is_none(),
            "Use a new or empty output directory.",
        )?;
    }
    Ok(())
}

pub fn guide(markdown: &str) -> String {
    let text = markdown.replace("\r\n", "\n");
    let text = Regex::new(r"(?m)^#{1,6} +").unwrap().replace_all(&text, "");
    let text = text
        .split('\n')
        .filter(|line| !line.contains("docs/DEVELOPMENT.md"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = Regex::new(r"(?m)^> ?").unwrap().replace_all(&text, "");
    let text = Regex::new(r"\[([^\]]+)\]\((https?://[^)]+)\)")
        .unwrap()
        .replace_all(&text, "$1 ($2)");
    let text = Regex::new(r"\[([^\]]+)\]\([^)]+\)")
        .unwrap()
        .replace_all(&text, "$1");
    text.replace("**", "")
        .replace('`', "")
        .replace('\n', "\r\n")
}

pub fn package(root: &Path, destination: &Path) -> Result {
    let destination = std::path::absolute(destination)?;
    let archive = PathBuf::from(format!("{}.zip", destination.display()));
    require(
        !archive.exists(),
        "Package archive already exists; archive the old output first.",
    )?;
    empty_destination(&destination)?;
    let source = root.join("native");
    let manifest = Manifest::read(&source)?;
    for path in [&manifest.desktop, &manifest.cli] {
        copy(&source.join(path), &destination.join("native").join(path))?;
    }
    copy(
        &source.join("engine.json"),
        &destination.join("native/engine.json"),
    )?;
    for name in ["BA-JPTR.exe", "LICENSE", "THIRD_PARTY_NOTICES.md"] {
        copy(&root.join(name), &destination.join(name))?;
    }
    let mut readme = vec![0xef, 0xbb, 0xbf];
    readme.extend_from_slice(guide(&fs::read_to_string(root.join("README.md"))?).as_bytes());
    write(&destination.join("README.txt"), &readme)?;
    copy_tree(&root.join("licenses"), &destination.join("licenses"))?;
    zip_tree(&destination, &archive)?;
    println!(
        "Portable package: {}\nPackage archive: {}",
        destination.display(),
        archive.display()
    );
    Ok(())
}

fn zip_tree(source: &Path, archive: &Path) -> Result {
    let mut pending =
        tempfile::NamedTempFile::new_in(archive.parent().ok_or("Missing archive parent.")?)?;
    let mut writer = zip::ZipWriter::new(pending.as_file_mut());
    // A fixed valid timestamp also handles registry license files dated before 1980.
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for entry in WalkDir::new(source).sort_by_file_name() {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let name = format!("BA-JPTR/{}", relative.to_string_lossy().replace('\\', "/"));
        if entry.file_type().is_dir() {
            writer.add_directory(
                if name.ends_with('/') {
                    name
                } else {
                    format!("{name}/")
                },
                options,
            )?;
        } else {
            writer.start_file(name, options)?;
            std::io::copy(&mut fs::File::open(entry.path())?, &mut writer)?;
        }
    }
    writer.finish()?.sync_all()?;
    pending.persist_noclobber(archive)?;
    Ok(())
}

pub fn source_package(root: &Path, destination: &Path) -> Result {
    let destination = std::path::absolute(destination)?;
    require(!destination.exists(), "Use a new source-package directory.")?;
    let root = fs::canonicalize(root)?;
    let ancestor = destination
        .ancestors()
        .find(|p| p.exists())
        .ok_or("Missing output drive.")?;
    let resolved_ancestor = fs::canonicalize(ancestor)?;
    let resolved_destination = resolved_ancestor.join(destination.strip_prefix(ancestor)?);
    require(
        resolved_destination != root,
        "Source package must use a new directory.",
    )?;
    for name in ["crates", "xtask", ".cargo", "docs", "licenses"] {
        require(
            !resolved_destination.starts_with(root.join(name)),
            "Source package cannot be inside a copied source directory.",
        )?;
    }
    for name in [
        "crates",
        "xtask",
        ".cargo/config.toml",
        "docs",
        "licenses",
        "Cargo.toml",
        "Cargo.lock",
        "README.md",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        ".gitattributes",
        ".gitignore",
    ] {
        let source = root.join(name);
        let target = destination.join(name);
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            copy(&source, &target)?;
        }
    }
    for entry in WalkDir::new(&destination) {
        let entry = entry?;
        let extension = entry
            .path()
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        require(
            !["py", "pyc", "pyd", "exe", "dll", "ps1"].contains(&extension.as_str()),
            format!(
                "Unexpected binary or script in source package: {}",
                entry.path().display()
            ),
        )?;
    }
    println!("Source package: {}", destination.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_windows_and_relative_escape_paths() {
        let mut manifest = Manifest {
            protocol: 1,
            desktop: "versions/01234567890123456789/bajptr-desktop.exe".into(),
            cli: "versions/01234567890123456789/bajptr-rs.exe".into(),
        };
        manifest.validate().unwrap();
        for path in [
            "../outside.exe",
            "versions/../outside.exe",
            "C:/outside.exe",
            "\\\\host\\share\\file.exe",
            "versions/01234567890123456789/..\\evil.exe",
        ] {
            manifest.cli = path.into();
            assert!(manifest.validate().is_err(), "{path}");
        }
    }

    #[test]
    fn package_omits_user_data_and_keeps_unicode_and_zip_root() -> Result {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("项目");
        let destination = temp.path().join("不同的输出名");
        let manifest = Manifest {
            protocol: 1,
            desktop: "versions/01234567890123456789/bajptr-desktop.exe".into(),
            cli: "versions/01234567890123456789/bajptr-rs.exe".into(),
        };
        write(
            &root.join("native/engine.json"),
            &serde_json::to_vec(&manifest)?,
        )?;
        for path in [&manifest.desktop, &manifest.cli] {
            write(&root.join("native").join(path), b"engine")?;
        }
        for name in [
            "BA-JPTR.exe",
            "LICENSE",
            "THIRD_PARTY_NOTICES.md",
            "licenses/中文.txt",
            ".gui/rules.tsv",
            "native/previous-engine.json",
        ] {
            write(&root.join(name), b"fixture")?;
        }
        write(
            &root.join("README.md"),
            "# 标题\n> **说明** `code`\n[网页](https://example.com)\n[开发](docs/DEVELOPMENT.md)\n"
                .as_bytes(),
        )?;
        package(&root, &destination)?;
        let readme = fs::read(destination.join("README.txt"))?;
        assert_eq!(&readme[..3], &[0xef, 0xbb, 0xbf]);
        assert_eq!(
            std::str::from_utf8(&readme[3..])?,
            "标题\r\n说明 code\r\n网页 (https://example.com)\r\n"
        );
        let mut zip =
            zip::ZipArchive::new(fs::File::open(format!("{}.zip", destination.display()))?)?;
        assert!(zip.by_name("BA-JPTR/licenses/中文.txt").is_ok());
        assert!(zip.file_names().all(|name| name.starts_with("BA-JPTR/")
            && !name.contains(".gui")
            && !name.contains("previous-engine")));
        assert!(package(&root, &destination).is_err());
        Ok(())
    }

    #[test]
    fn source_package_keeps_uncommitted_source_and_excludes_local_state() -> Result {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("源码");
        for name in [
            "crates/app/src/main.rs",
            "xtask/src/main.rs",
            ".cargo/config.toml",
            "docs/guide.md",
            "licenses/text.txt",
            "Cargo.toml",
            "Cargo.lock",
            "README.md",
            "LICENSE",
            "THIRD_PARTY_NOTICES.md",
            ".gitattributes",
            ".gitignore",
            ".gui/rules.tsv",
            "native/engine.json",
            "target/local.exe",
            ".toolchain/cargo/config.toml",
            ".cargo/credentials.toml",
        ] {
            write(&root.join(name), b"fixture")?;
        }
        let destination = root.join("work/源码包");
        source_package(&root, &destination)?;
        assert!(destination.join("xtask/src/main.rs").is_file());
        assert!(destination.join(".cargo/config.toml").is_file());
        for name in [
            ".gui",
            "native",
            "target",
            ".toolchain",
            ".cargo/credentials.toml",
        ] {
            assert!(!destination.join(name).exists(), "{name}");
        }
        assert!(source_package(&root, &root.join("crates/recursive-copy")).is_err());
        Ok(())
    }
}
