use crate::{Result, invalid, storage};
use serde_json::{Value, json};
use std::path::PathBuf;
pub fn metadata() -> Value {
    serde_json::from_str(include_str!("../assets/templates.json")).unwrap()
}
fn escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if "()[]{}?*+-|^$\\.&~# \t\n\r\u{b}\u{c}".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
pub fn build(key: &str, values: &[String]) -> Result<(String, String)> {
    let meta = metadata();
    let fields = meta[key]["fields"]
        .as_array()
        .ok_or_else(|| invalid("未知模板。"))?;
    if fields.len() != values.len() || values.iter().any(|s| s.contains(['\t', '\r', '\n'])) {
        return Err(invalid("请完整填写模板，内容不能含制表符或换行。"));
    }
    for i in 0..if key == "between" { 2 } else { 1 } {
        if values[i].is_empty() {
            return Err(invalid(format!(
                "请填写「{}」。",
                fields[i].as_str().unwrap_or("原文")
            )));
        }
    }
    let literal = |s: &str| s.replace('\\', "\\\\");
    let a = &values[0];
    let b = &values[1];
    Ok(match key {
        "aliases" => {
            let mut names = Vec::new();
            for s in a
                .split([';', '；'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if !names.contains(&s) {
                    names.push(s);
                }
            }
            if names.is_empty() {
                return Err(invalid("请至少填写一个旧叫法。"));
            }
            names.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
            (
                format!(
                    "(?:{})",
                    names.into_iter().map(escape).collect::<Vec<_>>().join("|")
                ),
                literal(b),
            )
        }
        "prefix" => (format!(r"\A{}", escape(a)), literal(b)),
        "suffix" => (format!(r"{}\Z", escape(a)), literal(b)),
        "between" => (
            format!(r"({})[^\r\n]*?({})", escape(a), escape(b)),
            format!(r"\g<1>{}\g<2>", literal(&values[2])),
        ),
        "number" => (
            format!(r"{}([0-9０-９]+)", escape(a)),
            format!(r"{}\g<1>", literal(b)),
        ),
        _ => return Err(invalid("未知模板。")),
    })
}
pub struct Templates {
    path: PathBuf,
    revision: Option<String>,
    data: Value,
}
pub(crate) fn parse_document(bytes: &[u8]) -> Result<Value> {
    let value: Value = serde_json::from_slice(bytes)?;
    if value["format"] != 2 || !value["drafts"].is_object() {
        return Err(invalid("模板表单文件格式无效。"));
    }
    Ok(value["drafts"].clone())
}
impl Templates {
    pub(crate) fn from_bytes(path: PathBuf, bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            path,
            revision: Some(storage::digest(bytes)),
            data: parse_document(bytes)?,
        })
    }
    pub fn new(path: PathBuf) -> Self {
        let revision = storage::revision(&path).unwrap_or(None);
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["format"] == 2 && v["drafts"].is_object())
            .map(|v| v["drafts"].clone())
            .unwrap_or(json!({}));
        Self {
            path,
            revision,
            data,
        }
    }
    fn key(old: &str, new: &str) -> String {
        storage::digest(
            serde_json::to_string(&json!([old, new]))
                .unwrap()
                .as_bytes(),
        )
    }
    pub fn lookup(&self, old: &str, new: &str) -> Option<Value> {
        let v = &self.data[Self::key(old, new)];
        let values = serde_json::from_value::<Vec<String>>(v["values"].clone()).ok()?;
        if build(v["key"].as_str()?, &values).ok()? != (old.into(), new.into())
            || !v["sample"].is_string()
        {
            return None;
        }
        Some(v.clone())
    }
    pub fn remember(&mut self, old: &str, new: &str, draft: &Value) -> Result<()> {
        let values = serde_json::from_value::<Vec<String>>(draft["values"].clone())?;
        if build(draft["key"].as_str().unwrap_or(""), &values)? != (old.into(), new.into()) {
            return Err(invalid("模板与词条内容不一致。"));
        }
        let mut data = self.data.clone();
        data[Self::key(old, new)] = draft.clone();
        self.revision = Some(storage::atomic_write(
            &self.path,
            &serde_json::to_vec(&json!({"format":2,"drafts":data}))?,
            Some(&self.revision),
        )?);
        self.data = data;
        Ok(())
    }
    pub fn display(&self, old: &str, new: &str) -> (String, String, Option<Value>) {
        if let Some(draft) = self.lookup(old, new) {
            let a = draft["values"][0].as_str().unwrap();
            let b = draft["values"][1].as_str().unwrap();
            let pair = match draft["key"].as_str().unwrap() {
                "number" => (format!("{a}＋数字"), format!("{b}＋原数字")),
                "between" => (
                    format!("{a}…{b}"),
                    format!("{a}{}{b}", draft["values"][2].as_str().unwrap()),
                ),
                "prefix" => (format!("开头：{a}"), b.into()),
                "suffix" => (format!("结尾：{a}"), b.into()),
                _ => (a.into(), b.into()),
            };
            (pair.0, pair.1, Some(draft))
        } else {
            (old.into(), new.into(), None)
        }
    }
}
