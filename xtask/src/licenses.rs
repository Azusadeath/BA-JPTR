use crate::{Result, files, require};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    path::Path,
};
use walkdir::WalkDir;

fn packages(metadata: &Value) -> Result<&Vec<Value>> {
    metadata["packages"]
        .as_array()
        .ok_or_else(|| "Cargo metadata has no packages.".into())
}

fn distributed(metadata: &Value) -> Result<HashSet<String>> {
    let nodes: HashMap<_, _> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("Cargo metadata has no dependency graph.")?
        .iter()
        .map(|node| (node["id"].as_str().unwrap_or_default(), node))
        .collect();
    let mut queue = VecDeque::new();
    for package in packages(metadata)? {
        if ["bajptr-desktop", "bajptr-rs", "bajptr-launcher"]
            .contains(&package["name"].as_str().unwrap_or_default())
        {
            queue.push_back(
                package["id"]
                    .as_str()
                    .ok_or("Package has no ID.")?
                    .to_string(),
            );
        }
    }
    require(
        !queue.is_empty(),
        "Cargo metadata has no application roots.",
    )?;
    let mut seen = HashSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .get(id.as_str())
            .ok_or("Dependency node missing from Cargo metadata.")?;
        for dependency in node["deps"]
            .as_array()
            .ok_or("Dependency node has no edges.")?
        {
            if dependency["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"] != "dev"))
            {
                queue.push_back(
                    dependency["pkg"]
                        .as_str()
                        .ok_or("Dependency has no package ID.")?
                        .to_string(),
                );
            }
        }
    }
    Ok(seen)
}

pub fn collect(metadata: &Value, destination: &Path) -> Result {
    let packages = packages(metadata)?;
    let source = |p: &Value| -> Result<std::path::PathBuf> {
        Ok(Path::new(
            p["manifest_path"]
                .as_str()
                .ok_or("Package has no manifest path.")?,
        )
        .parent()
        .ok_or("Manifest has no parent.")?
        .to_path_buf())
    };
    let mut apache = None;
    let mut boost = None;
    for package in packages.iter().filter(|p| !p["source"].is_null()) {
        let directory = source(package)?;
        if apache.is_none() && directory.join("LICENSE-APACHE").is_file() {
            apache = Some(directory.join("LICENSE-APACHE"));
        }
        if boost.is_none() && package["license"] == "BSL-1.0" && directory.join("LICENSE").is_file()
        {
            let text = fs::read_to_string(directory.join("LICENSE"))?;
            require(
                text.starts_with("Boost Software License - Version 1.0"),
                "Unexpected Boost license text.",
            )?;
            boost = Some(text);
        }
    }
    files::copy(
        &apache.ok_or("Apache license text unavailable.")?,
        &destination.join("STANDARD-APACHE-2.0.txt"),
    )?;
    if let Some(text) = &boost {
        files::write(&destination.join("STANDARD-BSL-1.0.txt"), text.as_bytes())?;
    }
    let seen = distributed(metadata)?;
    let mut included: Vec<_> = packages
        .iter()
        .filter(|p| !p["source"].is_null() && p["id"].as_str().is_some_and(|id| seen.contains(id)))
        .collect();
    included.sort_by_key(|p| {
        (
            p["name"].as_str().unwrap_or_default(),
            p["version"].as_str().unwrap_or_default(),
        )
    });
    let pattern = Regex::new(
        r"(?i)^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UNLICENSE|AUTHORS|COPYRIGHT)([._-]|$)",
    )?;
    let mut inventory = Vec::new();
    for package in included {
        let name = package["name"].as_str().ok_or("Package has no name.")?;
        let version = package["version"]
            .as_str()
            .ok_or("Package has no version.")?;
        let source = source(package)?;
        let folder = destination.join(format!("{name}-{version}"));
        let mut texts = BTreeSet::new();
        for entry in WalkDir::new(&source) {
            let entry = entry?;
            if entry.file_type().is_file() && pattern.is_match(&entry.file_name().to_string_lossy())
            {
                texts.insert(entry.path().to_path_buf());
            }
        }
        if let Some(license) = package["license_file"].as_str() {
            texts.insert(source.join(license));
        }
        let mut paths = Vec::new();
        for file in texts {
            let relative = file.strip_prefix(&source)?;
            files::copy(&file, &folder.join(relative))?;
            paths.push(relative.to_string_lossy().replace('\\', "/"));
        }
        let mut selection = package["license"].clone();
        if paths.is_empty() {
            let license = package["license"].as_str().unwrap_or_default();
            if license.contains("Apache-2.0") {
                paths.push("../STANDARD-APACHE-2.0.txt".to_string());
                selection = json!("Apache-2.0");
            } else if license == "BSL-1.0" && boost.is_some() {
                paths.push("../STANDARD-BSL-1.0.txt".to_string());
                selection = json!("BSL-1.0");
            } else {
                return Err(format!("No license text found for {name}.").into());
            }
        }
        inventory.push(json!({"name":name,"version":version,"license":package["license"],"distributed_license":selection,"authors":package["authors"],"repository":package["repository"],"license_texts":paths}));
    }
    files::atomic_write(
        &destination.join("inventory.json"),
        &serde_json::to_vec_pretty(&inventory)?,
    )?;
    println!("Dependency notices collected: {}", inventory.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_follow_application_and_build_deps_excluding_dev_and_xtask() -> Result {
        let metadata = json!({"packages":[{"id":"app","name":"bajptr-rs"},{"id":"tool","name":"xtask"}],"resolve":{"nodes":[
            {"id":"app","deps":[{"pkg":"shared","dep_kinds":[{"kind":null}]},{"pkg":"builder","dep_kinds":[{"kind":"build"}]},{"pkg":"test","dep_kinds":[{"kind":"dev"}]}]},
            {"id":"tool","deps":[{"pkg":"zip","dep_kinds":[{"kind":null}]}]},
            {"id":"shared","deps":[]},{"id":"builder","deps":[]},{"id":"test","deps":[]},{"id":"zip","deps":[]}
        ]}});
        assert_eq!(
            distributed(&metadata)?,
            HashSet::from(["app".into(), "shared".into(), "builder".into()])
        );
        Ok(())
    }
}
