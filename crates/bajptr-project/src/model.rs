use crate::{Result, invalid, storage};
use bajptr_core::{
    database::{RowId, TextEdit, TextRow},
    fields,
    rules::{Evidence, Rule, Rules},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::OnceLock,
};
pub fn scopes(value: &str) -> Result<Option<Vec<String>>> {
    if value.is_empty() {
        return Ok(None);
    }
    let mut names: Vec<String> = if value.starts_with('[') {
        serde_json::from_str(value)?
    } else {
        vec![value.into()]
    };
    if names.is_empty()
        || names
            .iter()
            .any(|s| s.is_empty() || s.trim() != s || s.chars().any(char::is_control))
    {
        return Err(invalid("来源表列表或表名无效。"));
    }
    names.sort();
    names.dedup();
    Ok(Some(names))
}
pub fn normalize(mut values: Vec<Rule>) -> Result<Vec<Rule>> {
    for value in &mut values {
        value.table = match scopes(value.table.trim())? {
            None => String::new(),
            Some(n) if n.len() == 1 => n[0].clone(),
            Some(n) => serde_json::to_string(&n)?,
        };
    }
    Rules::new(values.clone())?;
    Ok(values)
}
pub fn parse_rules(bytes: &[u8]) -> Result<Vec<Rule>> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| invalid("词表不是有效的 UTF-8 文本。"))?
        .trim_start_matches('\u{feff}');
    let mut values = Vec::new();
    for (line, raw) in source.lines().enumerate() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let fields: Vec<_> = raw.trim_end_matches('\r').split('\t').collect();
        if fields.len() != 5 {
            return Err(invalid(format!(
                "词表第 {} 行需要五列：原词、替换词、方式、备注、来源表。",
                line + 1
            )));
        }
        let mode = fields[2].to_string();
        values.push(Rule {
            old: fields[0].into(),
            new: fields[1].into(),
            mode,
            source: fields[3].into(),
            table: fields[4].into(),
        });
    }
    normalize(values)
}
pub fn rules_bytes(rules: &[Rule]) -> Result<Vec<u8>> {
    let rules = normalize(rules.to_vec())?;
    Ok(rules
        .iter()
        .map(|r| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                r.old, r.new, r.mode, r.source, r.table
            )
        })
        .collect::<String>()
        .into_bytes())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchOperation {
    pub id: String,
    pub old: String,
    pub new: String,
    pub before: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    #[serde(flatten)]
    pub kind: OperationKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationKind {
    Local {
        base: String,
        changes: Vec<crate::replay::Change>,
    },
    Batch,
    Override,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub game: String,
    pub version: String,
    pub table: String,
    pub row_id: RowId,
    pub slot: usize,
    pub original: String,
    pub replacement: String,
    #[serde(default)]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_operation: Option<BatchOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
}
impl Edit {
    pub fn operation_id(&self) -> Option<&str> {
        self.operation.as_ref().map(|o| o.id.as_str())
    }
    pub fn apply(&self, current: &str) -> Result<String> {
        let result = match self.operation.as_ref().map(|o| &o.kind) {
            Some(OperationKind::Local { base, changes }) => {
                crate::replay::apply(base, changes, current)?
            }
            Some(OperationKind::Batch) => {
                let batch = self
                    .batch_operation
                    .as_ref()
                    .ok_or_else(|| invalid("批量操作信息缺失。"))?;
                current.replace(&batch.old, &batch.new)
            }
            _ => self.replacement.clone(),
        };
        if result.len() > fields::MAX_STRING || result.chars().any(|c| c < ' ' && c != '\n') {
            return Err(invalid("修改结果过长或包含控制字符。"));
        }
        Ok(result)
    }
    pub fn array(&self) -> Value {
        json!([
            self.game,
            self.version,
            self.table,
            self.row_id,
            self.slot,
            self.original,
            self.replacement
        ])
    }
    pub fn core(&self) -> TextEdit {
        TextEdit {
            table: self.table.clone(),
            row_id: self.row_id.clone(),
            slot: self.slot,
            original: self.original.clone(),
            replacement: self.replacement.clone(),
            wordlist_applied: false,
        }
    }
    pub fn same_address(&self, e: &Edit) -> bool {
        self.game == e.game
            && self.version == e.version
            && self.table == e.table
            && self.row_id == e.row_id
            && self.slot == e.slot
    }
}
pub fn validate_edits(edits: &[Edit]) -> Result<()> {
    let mut seen = HashSet::new();
    let mut batches = HashMap::new();
    for e in edits {
        if e.game.is_empty() || !is_version(&e.version) || e.table.is_empty() {
            return Err(invalid("单条修改的定位信息无效，请重新查找原文。"));
        }
        for text in [&e.original, &e.replacement] {
            if text.len() > fields::MAX_STRING || text.chars().any(|c| c < ' ' && c != '\n') {
                return Err(invalid("文本过长或包含控制字符；换行可使用 Enter。"));
            }
        }
        if let Some(operation) = &e.batch_operation {
            if !e.batch
                || operation.id.len() != 32
                || !operation.id.bytes().all(|c| c.is_ascii_hexdigit())
                || operation.old.is_empty()
            {
                return Err(invalid("批量修改的操作信息无效。"));
            }
            for value in [&operation.old, &operation.new, &operation.before] {
                if value.len() > fields::MAX_STRING || value.chars().any(|c| c < ' ' && c != '\n') {
                    return Err(invalid("批量修改内容过长或包含控制字符。"));
                }
            }
            let signature = (&e.game, &e.version, &operation.old, &operation.new);
            if batches
                .insert(&operation.id, signature)
                .is_some_and(|previous| previous != signature)
            {
                return Err(invalid("同一次批量修改的替换参数不一致。"));
            }
        }
        if let Some(operation) = &e.operation {
            if operation.id.len() != 32 || !operation.id.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(invalid("修改操作标识无效。"));
            }
            match &operation.kind {
                OperationKind::Batch
                    if !e.batch
                        || e.batch_operation
                            .as_ref()
                            .is_none_or(|b| b.id != operation.id) =>
                {
                    return Err(invalid("批量操作信息不一致。"));
                }
                OperationKind::Local { base, changes } => {
                    if e.batch
                        || base.len() > fields::MAX_STRING
                        || base.chars().any(|c| c < ' ' && c != '\n')
                        || changes.is_empty()
                    {
                        return Err(invalid("局部修改信息无效。"));
                    }
                    if crate::replay::apply(base, changes, base)? != e.replacement {
                        return Err(invalid("局部修改与保存结果不一致。"));
                    }
                }
                _ => {}
            }
        }
        if !seen.insert((
            &e.game,
            &e.version,
            &e.table,
            &e.row_id,
            e.slot,
            e.operation_id(),
        )) {
            return Err(invalid("同一段文本有重复修改，请保留一项。"));
        }
    }
    Ok(())
}
pub fn is_version(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn parse_edits(bytes: &[u8]) -> Result<Vec<Edit>> {
    let value: Value = serde_json::from_slice(bytes)?;
    if value["format"] != 2 && value["format"] != 3 {
        return Err(invalid("单条修改记录格式不受支持。"));
    }
    if !value["edits"]
        .as_array()
        .is_some_and(|rows| rows.iter().all(Value::is_object))
    {
        return Err(invalid("单条修改必须使用具名字段格式。"));
    }
    let edits: Vec<Edit> = serde_json::from_value(value["edits"].clone())?;
    if value["format"] == 2 && edits.iter().any(|e| e.operation.is_some()) {
        return Err(invalid("操作记录需要新版格式，不能按旧式覆盖读取。"));
    }
    validate_edits(&edits)?;
    Ok(edits)
}
pub fn edits_bytes(edits: &[Edit]) -> Result<Vec<u8>> {
    validate_edits(edits)?;
    Ok(serde_json::to_vec_pretty(
        &json!({"format":if edits.iter().any(|e|e.operation.is_some()){3}else{2},"edits":edits}),
    )?)
}
pub fn update_edits(existing: &[Edit], updates: Vec<Edit>) -> Result<Vec<Edit>> {
    let mut values = existing.to_vec();
    for e in updates {
        if let Some(index) = values
            .iter()
            .position(|p| p.same_address(&e) && p.operation_id() == e.operation_id())
        {
            values[index] = e;
        } else {
            values.push(e);
        }
    }
    validate_edits(&values)?;
    Ok(values)
}
pub fn effective(
    row: &TextRow,
    edits: &[Edit],
    game: &str,
    version: &str,
    rules: &Rules,
    skip: bool,
) -> Result<(String, bool)> {
    let applicable: Vec<_> = edits
        .iter()
        .filter(|e| {
            e.game == game
                && e.version == version
                && e.table == row.0
                && e.row_id == row.1
                && e.slot == row.2
        })
        .collect();
    for e in &applicable {
        if e.original != row.3 {
            return Err(invalid("定位记录的底稿原文已变化，请重新查找。"));
        }
    }
    let last_override = applicable.iter().rposition(|e| {
        !matches!(
            e.operation.as_ref().map(|o| &o.kind),
            Some(OperationKind::Local { .. } | OperationKind::Batch)
        )
    });
    let result = if last_override.is_some() || (skip && fields::has_kana(&row.3)) {
        None
    } else {
        rules.view(&row.0).apply(
            rules,
            &row.3,
            &mut Evidence::new(rules.values.len(), false),
            !fields::has_cjk(&row.3),
        )?
    };
    let mut value = result.unwrap_or_else(|| row.3.clone());
    for edit in &applicable[last_override.unwrap_or(0)..] {
        value = edit.apply(&value).map_err(|e| {
            invalid(format!(
                "{} / 记录 {} / 字段 {}：{e}",
                title(&row.0),
                row.1.display(),
                row.2
            ))
        })?;
    }
    Ok((value, !applicable.is_empty()))
}

pub fn resolved_edits(
    edits: &[Edit],
    game: &str,
    version: &str,
    rules: &Rules,
    skip: bool,
) -> Result<Vec<TextEdit>> {
    validate_edits(edits)?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for edit in edits
        .iter()
        .filter(|e| e.game == game && e.version == version)
    {
        if !seen.insert((&edit.table, &edit.row_id, edit.slot)) {
            continue;
        }
        let row = (
            edit.table.clone(),
            edit.row_id.clone(),
            edit.slot,
            edit.original.clone(),
        );
        let mut target = edit.core();
        target.replacement = effective(&row, edits, game, version, rules, skip)?.0;
        target.wordlist_applied = edits.iter().filter(|e| e.same_address(edit)).all(|e| {
            matches!(
                e.operation.as_ref().map(|o| &o.kind),
                Some(OperationKind::Local { .. } | OperationKind::Batch)
            )
        });
        result.push(target);
    }
    Ok(result)
}
pub fn game_key(game: &str) -> Result<String> {
    if game.trim().is_empty() {
        return Ok(String::new());
    }
    storage::identity(Path::new(game))
}
fn tables() -> &'static Value {
    static TABLES: OnceLock<Value> = OnceLock::new();
    TABLES.get_or_init(|| serde_json::from_str(include_str!("../assets/tables.json")).unwrap())
}
pub fn title(name: &str) -> String {
    if name.is_empty() {
        "全部文本表".into()
    } else {
        tables()[name]["title"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("未分类 · {name}"))
    }
}
pub fn table_info(name: &str) -> Value {
    let value = &tables()[name];
    json!({"name":name,"label":title(name),"purpose":value["purpose"].as_str().unwrap_or("尚未确认用途，保留原表名；可先查找文字确认内容。"),"basis":if value["text_checked"]==true { "已根据文本样例核对用途；不代表已核对整张表。" } else { "用途按表名归类，尚未确认可替换文本内容。" }})
}
pub fn scope_title(scope: &str) -> String {
    match scopes(scope).ok().flatten() {
        None => title(""),
        Some(n) if n.len() > 2 => format!("已选 {} 张表", n.len()),
        Some(n) => n.iter().map(|s| title(s)).collect::<Vec<_>>().join("、"),
    }
}
pub fn fingerprint(rules: &[Rule], skip: bool, edits: &[Edit]) -> Result<String> {
    let rules = rules
        .iter()
        .map(|r| Ok(json!([r.old, r.new, r.mode, scopes(&r.table)?])))
        .collect::<Result<Vec<_>>>()?;
    Ok(storage::digest(
        serde_json::to_string(&json!(["original-spans-v3", skip, edits, rules]))?.as_bytes(),
    ))
}
