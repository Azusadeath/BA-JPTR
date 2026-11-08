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
}
impl Edit {
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
    let mut addresses = HashMap::new();
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
        let address = (&e.game, &e.version, &e.table, &e.row_id, e.slot);
        if !seen.insert((address, e.batch)) {
            return Err(invalid("同一段文本有重复的同类修改，请保留一项。"));
        }
        if let Some(previous) = addresses.insert(address, e)
            && (previous.original != e.original
                || (previous.batch && previous.batch_operation.is_none())
                || (e.batch && e.batch_operation.is_none()))
        {
            return Err(invalid("单条与批量修改的原文或操作信息不一致。"));
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
    if value["format"] != 2 {
        return Err(invalid("单条修改记录格式不受支持。"));
    }
    if !value["edits"]
        .as_array()
        .is_some_and(|rows| rows.iter().all(Value::is_object))
    {
        return Err(invalid("单条修改必须使用具名字段格式。"));
    }
    let edits: Vec<Edit> = serde_json::from_value(value["edits"].clone())?;
    validate_edits(&edits)?;
    Ok(edits)
}
pub fn edits_bytes(edits: &[Edit]) -> Result<Vec<u8>> {
    validate_edits(edits)?;
    Ok(serde_json::to_vec_pretty(
        &json!({"format":2,"edits":edits}),
    )?)
}
pub fn update_edits(existing: &[Edit], updates: Vec<Edit>) -> Result<Vec<Edit>> {
    let mut values = existing.to_vec();
    for e in updates {
        if !e.batch {
            for batch in values.iter_mut().filter(|p| p.batch && p.same_address(&e)) {
                if let Some(operation) = &mut batch.batch_operation {
                    operation.before.clone_from(&e.replacement);
                    batch.replacement = operation.before.replace(&operation.old, &operation.new);
                }
            }
        }
        if let Some(index) = values
            .iter()
            .position(|p| p.same_address(&e) && p.batch == e.batch)
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
    if let Some(e) = edits
        .iter()
        .filter(|e| {
            e.game == game
                && e.version == version
                && e.table == row.0
                && e.row_id == row.1
                && e.slot == row.2
        })
        .max_by_key(|e| e.batch)
    {
        if e.original != row.3 {
            return Err(invalid("定位记录的底稿原文已变化，请重新查找。"));
        }
        return Ok((e.replacement.clone(), true));
    }
    if skip && fields::has_kana(&row.3) {
        return Ok((row.3.clone(), false));
    }
    let result = rules.view(&row.0).apply(
        rules,
        &row.3,
        &mut Evidence::new(rules.values.len(), false),
        !fields::has_cjk(&row.3),
    )?;
    Ok((result.unwrap_or_else(|| row.3.clone()), false))
}
/// The kernel receives one final override per field; paired single records stay
/// in the project so removing their batch can reveal the saved single text.
pub fn final_edits(edits: &[Edit], game: &str, version: &str) -> Result<Vec<TextEdit>> {
    validate_edits(edits)?;
    let mut seen = HashSet::new();
    Ok([true, false]
        .into_iter()
        .flat_map(|batch| {
            edits
                .iter()
                .filter(move |e| e.game == game && e.version == version && e.batch == batch)
        })
        .filter(|e| seen.insert((&e.table, &e.row_id, e.slot)))
        .map(Edit::core)
        .collect())
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
