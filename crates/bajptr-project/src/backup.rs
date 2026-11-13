use crate::{Control, Result, invalid, model, sources::Sources, storage};
use bajptr_core::rules::Rule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

pub const FILES: [&str; 5] = [
    "rules.tsv",
    "text-edits.json",
    "settings.json",
    "source-history.json",
    "template-drafts.json",
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    format: u32,
    kind: String,
    files: BTreeMap<String, String>,
}
pub struct Decoded {
    pub rules: Vec<Rule>,
    pub edits: Vec<model::Edit>,
    pub settings: Value,
    pub sources: Sources,
}
pub struct Snapshot {
    pub files: BTreeMap<String, Option<Vec<u8>>>,
}
pub fn settings(bytes: &[u8]) -> Result<Value> {
    let value: Value = serde_json::from_slice(bytes)?;
    if value["format"] != 2
        || value.get("game").is_some_and(|game| !game.is_string())
        || value
            .get("skip_kana")
            .is_some_and(|skip| !skip.is_boolean())
    {
        return Err(invalid("设置文件格式无效。"));
    }
    Ok(value)
}
pub fn capture(root: &Path, control: &Control<'_>) -> Result<Snapshot> {
    let _locks = FILES
        .iter()
        .map(|name| storage::Lock::acquire(&root.join(name)))
        .collect::<Result<Vec<_>>>()?;
    let mut files = BTreeMap::new();
    for name in FILES {
        control.check()?;
        let bytes = match fs::read(root.join(name)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        files.insert(name.into(), bytes);
    }
    Ok(Snapshot { files })
}
impl Snapshot {
    pub fn revisions(&self) -> HashMap<String, Option<String>> {
        self.files
            .iter()
            .map(|(name, bytes)| (name.clone(), bytes.as_deref().map(storage::digest)))
            .collect()
    }
    pub fn bundle(&self) -> Result<Bundle> {
        let mut files = BTreeMap::new();
        for name in FILES {
            let bytes = match self.files[name].as_ref() {
                Some(bytes) => bytes.clone(),
                None => match name {
                    "rules.tsv" => vec![],
                    "text-edits.json" => model::edits_bytes(&[])?,
                    "settings.json" => {
                        serde_json::to_vec(&json!({"format":2,"game":"","skip_kana":true}))?
                    }
                    "source-history.json" => serde_json::to_vec(&Sources::default())?,
                    _ => serde_json::to_vec(&json!({"format":2,"drafts":{}}))?,
                },
            };
            files.insert(
                name.into(),
                String::from_utf8(bytes)
                    .map_err(|_| invalid(format!("{name} 不是有效 UTF-8 文本，无法创建备份。")))?,
            );
        }
        let result = Bundle {
            format: 1,
            kind: "bajptr-workspace-backup".into(),
            files,
        };
        result.decode().map_err(|source| crate::Error::Context {
            message: "工作区记录或设置无效，未创建备份。请修复后重试，详细原因见日志。".into(),
            source: Box::new(source),
        })?;
        Ok(result)
    }
    pub fn archive(&self, root: &Path, control: &Control<'_>) -> Result<PathBuf> {
        control.check()?;
        let path = root.join("recovery").join(format!(
            "before-backup-restore-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path)?;
        for (name, bytes) in &self.files {
            if let Some(bytes) = bytes {
                storage::atomic_write(&path.join(name), bytes, Some(&None))?;
            }
        }
        storage::atomic_write(
            &path.join("info.json"),
            &serde_json::to_vec_pretty(
                &json!({"format":1,"files":self.files.iter().map(|(name,bytes)|json!({"name":name,"existed":bytes.is_some()})).collect::<Vec<_>>()}),
            )?,
            Some(&None),
        )?;
        if let Ok(bundle) = self.bundle() {
            bundle.write(&path.join("workspace-backup.json"), root, control)?;
        }
        Ok(path)
    }
}
impl Bundle {
    pub fn read(path: &Path) -> Result<Self> {
        let value: Self = serde_json::from_slice(&fs::read(path)?)?;
        value.decode()?;
        Ok(value)
    }
    pub fn documents(&self) -> BTreeMap<String, Vec<u8>> {
        self.files
            .iter()
            .map(|(name, text)| (name.clone(), text.as_bytes().to_vec()))
            .collect()
    }
    pub fn decode(&self) -> Result<Decoded> {
        if self.format != 1
            || self.kind != "bajptr-workspace-backup"
            || self.files.len() != FILES.len()
            || FILES.iter().any(|name| !self.files.contains_key(*name))
        {
            return Err(invalid("这不是完整的 BA-JPTR 备份，或备份版本不受支持。"));
        }
        let check = || -> Result<Decoded> {
            let rules = model::parse_rules(self.files["rules.tsv"].as_bytes())?;
            let edits = model::parse_edits(self.files["text-edits.json"].as_bytes())?;
            let settings = settings(self.files["settings.json"].as_bytes())?;
            let sources = Sources::parse(self.files["source-history.json"].as_bytes())?;
            crate::templates::parse_document(self.files["template-drafts.json"].as_bytes())?;
            Ok(Decoded {
                rules,
                edits,
                settings,
                sources,
            })
        };
        check().map_err(|source| crate::Error::Context {
            message: "备份中的记录或设置无效，未恢复。请检查备份文件，详细原因见日志。".into(),
            source: Box::new(source),
        })
    }
    pub fn write(&self, path: &Path, root: &Path, control: &Control<'_>) -> Result<()> {
        let target = storage::identity(path)?;
        if FILES
            .iter()
            .chain(
                [
                    "application-state.json",
                    "tables-cache.json",
                    "sources-transaction.json",
                    "replacements-transaction.json",
                    "workspace-transaction.json",
                ]
                .iter(),
            )
            .any(|name| storage::identity(&root.join(name)).is_ok_and(|path| path == target))
        {
            return Err(invalid("备份文件不能覆盖工作区记录，请选择其他文件名。"));
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        control.check()?;
        storage::atomic_write(path, &bytes, None)?;
        Ok(())
    }
}
