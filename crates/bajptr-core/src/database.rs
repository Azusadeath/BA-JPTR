use crate::{
    Control, Error, Result,
    fields::{self, BlobStats},
    invalid,
    rules::{Evidence, Miss, Rules, View},
};
use rusqlite::{
    Connection, OpenFlags,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowId {
    Integer(i64),
    Text(String),
}
impl RowId {
    fn from_sql(v: ValueRef<'_>) -> Result<Self> {
        match v {
            ValueRef::Integer(i) => Ok(Self::Integer(i)),
            ValueRef::Text(t) => Ok(Self::Text(
                std::str::from_utf8(t)
                    .map_err(|_| invalid("记录 ID 不是有效文本。"))?
                    .into(),
            )),
            _ => Err(invalid("记录 ID 必须是整数或文本。")),
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::Integer(i) => Value::Integer(*i),
            Self::Text(s) => Value::Text(s.clone()),
        }
    }
    pub fn display(&self) -> String {
        match self {
            Self::Integer(i) => i.to_string(),
            Self::Text(s) => s.clone(),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TextEdit {
    pub table: String,
    pub row_id: RowId,
    pub slot: usize,
    pub original: String,
    pub replacement: String,
    #[serde(default)]
    pub wordlist_applied: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub kind: String,
    pub table: String,
    pub row_id: RowId,
    pub slot: usize,
    pub original: String,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Default, Serialize)]
pub struct Stats {
    pub rows_seen: u64,
    pub rows_changed: u64,
    pub fields_changed: u64,
    pub inplace: u64,
    pub appended: u64,
    pub bytes_appended: u64,
    pub kana_skipped: u64,
    pub parse_failed: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub tables_skipped: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub fields_skipped: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    pub tables: Vec<String>,
    pub hits: Vec<u64>,
    pub zero_reasons: Vec<Miss>,
    pub added: u64,
    pub updated: u64,
    pub reverted: u64,
    pub changes: Vec<Change>,
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub kind: &'static str,
    pub table: String,
    pub row_id: Option<RowId>,
    pub slot: Option<usize>,
    pub message: String,
}
impl Diagnostic {
    fn display(&self) -> String {
        let mut address = format!("来源表 {}", self.table);
        if let Some(id) = &self.row_id {
            address.push_str(&format!("，记录 {}", id.display()));
        }
        if let Some(slot) = self.slot {
            address.push_str(&format!("，字段 {slot}"));
        }
        format!("{address}：{}", self.message)
    }
}
fn note(
    stats: &mut Stats,
    kind: &'static str,
    table: &str,
    rid: Option<&RowId>,
    slot: Option<usize>,
    message: String,
    control: &Control<'_>,
) -> Result<()> {
    if stats.diagnostics.len() >= 20
        && kind == "parse"
        && let Some(index) = stats.diagnostics.iter().position(|d| d.kind != "parse")
    {
        stats.diagnostics.remove(index);
    }
    if stats.diagnostics.len() < 20 {
        let diagnostic = Diagnostic {
            kind,
            table: table.into(),
            row_id: rid.cloned(),
            slot,
            message,
        };
        control.log(&diagnostic.display())?;
        stats.diagnostics.push(diagnostic);
    }
    Ok(())
}
fn record_error(table: &str, rid: &RowId, error: Error) -> Error {
    if matches!(error, Error::Cancelled) {
        error
    } else {
        Error::Record {
            table: table.into(),
            record: rid.display(),
            source: Box::new(error),
        }
    }
}
fn field_error(slot: usize, error: Error) -> Error {
    if matches!(error, Error::Cancelled) {
        error
    } else {
        Error::Field {
            slot,
            source: Box::new(error),
        }
    }
}
fn relative_offset(spos: usize, fpos: usize) -> Result<u32> {
    spos.checked_sub(fpos)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("FlatBuffer 偏移超出支持范围。"))
}
#[derive(Debug, Deserialize)]
pub struct Options {
    #[serde(default = "yes")]
    pub skip_kana: bool,
    #[serde(default = "yes")]
    pub ascii_exact: bool,
    #[serde(default)]
    pub explain: bool,
    #[serde(default = "default_tables")]
    pub tables: String,
    #[serde(default)]
    pub edits: Vec<TextEdit>,
}
fn yes() -> bool {
    true
}
pub fn default_tables() -> String {
    "Localize|Dialog|Scenario|Subtitle|Talk|Mail|Name|Emoji|Voice|Messanger|SNSProfile".into()
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn key_name(con: &Connection, table: &str) -> Result<String> {
    let mut schema = con.prepare(&format!("PRAGMA table_xinfo({})", quote(table)))?;
    let columns = schema
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !columns
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("Bytes"))
    {
        return Err(Error::UnsupportedTable(
            "没有 Bytes 文本列，已跳过此表。".into(),
        ));
    }
    let without_rowid: bool = con.query_row(
        "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?",
        [table],
        |row| row.get(0),
    )?;
    // A real column may shadow any rowid spelling. Never treat that column as
    // a unique identity merely because preparing SELECT rowid succeeds.
    for alias in ["rowid", "_rowid_", "oid"] {
        if !without_rowid
            && !columns
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(alias))
        {
            return Ok(alias.into());
        }
    }
    let mut primary = columns
        .iter()
        .filter(|(_, order)| *order > 0)
        .collect::<Vec<_>>();
    primary.sort_by_key(|(_, order)| *order);
    match primary.as_slice() {
        [] => Err(Error::UnsupportedTable(
            "没有可唯一定位记录的 rowid 或主键，已跳过此表。".into(),
        )),
        [(name, _)] => Ok(quote(name)),
        _ => {
            // JSON preserves the complete ordered key and its scalar types in
            // the existing text-ID contract, including targeted edits/exports.
            Ok(format!(
                "json_array({})",
                primary
                    .iter()
                    .map(|(name, _)| quote(name))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        }
    }
}
pub fn tables(con: &Connection, pattern: &str) -> Result<Vec<String>> {
    let regex = regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|_| invalid("文本表筛选表达式无效或不受支持。"))?;
    let mut query =
        con.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let names = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names.into_iter().filter(|n| regex.is_match(n)).collect())
}

pub struct Database {
    pub con: Connection,
    _temporary: Option<tempfile::TempDir>,
}
impl Database {
    pub fn open(
        path: &Path,
        encrypted: bool,
        key: &[u8; 32],
        control: &Control<'_>,
    ) -> Result<Self> {
        let temporary = if encrypted {
            Some(tempfile::tempdir()?)
        } else {
            None
        };
        let plain = if let Some(tmp) = &temporary {
            let plain = tmp.path().join("read.db");
            crate::codec::decrypt(path, &plain, key, control)?;
            plain
        } else {
            path.to_path_buf()
        };
        let con = Connection::open_with_flags(&plain, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // Keep the decrypted database on the temporary file and let SQLite's
        // page cache do the bounded caching. Copying a 200+ MiB database into
        // a second in-memory connection adds latency and resident memory while
        // providing no reliable scan benefit on the target workload.
        con.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-65536;")?;
        Ok(Self {
            con,
            _temporary: temporary,
        })
    }
}

pub fn patch_blob(
    blob: &[u8],
    rules: &Rules,
    view: &View,
    evidence: &mut Evidence,
    options: &Options,
    overrides: &HashMap<usize, String>,
) -> Result<(Option<Vec<u8>>, BlobStats)> {
    patch_blob_composed(
        blob,
        rules,
        view,
        evidence,
        options,
        overrides,
        &std::collections::HashSet::new(),
    )
}
fn patch_blob_composed(
    blob: &[u8],
    rules: &Rules,
    view: &View,
    evidence: &mut Evidence,
    options: &Options,
    overrides: &HashMap<usize, String>,
    composed: &std::collections::HashSet<usize>,
) -> Result<(Option<Vec<u8>>, BlobStats)> {
    let parsed = fields::parse_with_issues(blob, options.ascii_exact || !overrides.is_empty())?;
    let fields = parsed.fields;
    let mut shared = HashMap::new();
    if !overrides.is_empty() {
        for f in &fields {
            *shared.entry(f.spos).or_insert(0usize) += 1;
        }
    }
    let mut out: Option<Vec<u8>> = None;
    let mut stats = BlobStats {
        fields_skipped: parsed.skipped,
        issues: parsed.issues,
        ..Default::default()
    };
    let mut appended: HashMap<(usize, String), usize> = HashMap::new();
    for f in &fields {
        let explicit = overrides.get(&f.slot);
        if explicit.is_some() {
            if composed.contains(&f.slot) {
                if options.skip_kana && fields::has_kana(f.text) {
                    view.skipped(rules, f.text, evidence, false)
                        .map_err(|e| field_error(f.slot, e))?;
                } else {
                    view.apply(rules, f.text, evidence, !fields::has_cjk(f.text))
                        .map_err(|e| field_error(f.slot, e))?;
                }
            } else {
                view.skipped(rules, f.text, evidence, true)
                    .map_err(|e| field_error(f.slot, e))?;
            }
        }
        if explicit.is_none() && options.skip_kana && fields::has_kana(f.text) {
            stats.kana_skipped += 1;
            view.skipped(rules, f.text, evidence, false)
                .map_err(|e| field_error(f.slot, e))?;
            continue;
        }
        let new = if let Some(s) = explicit {
            Some(s.clone())
        } else if fields::has_cjk(f.text) {
            view.apply(rules, f.text, evidence, false)
                .map_err(|e| field_error(f.slot, e))?
        } else if options.ascii_exact {
            view.apply(rules, f.text, evidence, true)
                .map_err(|e| field_error(f.slot, e))?
        } else {
            None
        };
        let Some(new) = new else {
            continue;
        };
        if new == f.text {
            continue;
        }
        if new.len() > fields::MAX_STRING || fields::forbidden(&new) {
            return Err(field_error(f.slot, invalid("替换文字过长或包含控制字符。")));
        }
        let b = out.get_or_insert_with(|| blob.to_vec());
        if new.len() == f.text.len() && shared.get(&f.spos).copied().unwrap_or(0) <= 1 {
            b[f.spos + 4..f.spos + 4 + new.len()].copy_from_slice(new.as_bytes());
            stats.inplace += 1;
        } else {
            let key = (f.spos, new.clone());
            if let Some(&spos) = appended.get(&key) {
                b[f.fpos..f.fpos + 4].copy_from_slice(
                    &relative_offset(spos, f.fpos)
                        .map_err(|e| field_error(f.slot, e))?
                        .to_le_bytes(),
                );
                stats.appended += 1;
                continue;
            }
            let pad = (4 - b.len() % 4) % 4;
            b.resize(b.len() + pad, 0);
            let spos = b.len();
            b.extend_from_slice(&(new.len() as u32).to_le_bytes());
            b.extend_from_slice(new.as_bytes());
            b.push(0);
            let offset = relative_offset(spos, f.fpos).map_err(|e| field_error(f.slot, e))?;
            b[f.fpos..f.fpos + 4].copy_from_slice(&offset.to_le_bytes());
            appended.insert(key, spos);
            stats.appended += 1;
            stats.bytes_appended += (pad + 4 + new.len() + 1) as u64;
        }
    }
    Ok((out, stats))
}

type Targets = HashMap<(String, RowId), HashMap<usize, String>>;
pub type CurrentOverrides = Targets;

pub fn current_overrides(
    original: &Connection,
    current: &Connection,
    pattern: &str,
    limit: usize,
    control: &Control<'_>,
) -> Result<Option<CurrentOverrides>> {
    let names = tables(original, pattern)?;
    if tables(current, pattern)? != names {
        return Err(invalid("当前游戏文本表与底稿不同，请检查游戏资源后重试。"));
    }
    let mut output = HashMap::new();
    let mut used = 0;
    for table in &names {
        let key = match key_name(original, table) {
            Ok(key) => key,
            Err(Error::UnsupportedTable(reason)) => {
                control.log(&format!("来源表 {table}：{reason}"))?;
                continue;
            }
            Err(error) => return Err(error),
        };
        let mut left = original.prepare(&format!(
            "SELECT {key},Bytes FROM {} ORDER BY 1",
            quote(table)
        ))?;
        let mut rows = left.query([])?;
        let key = key_name(current, table)?;
        let mut right = current.prepare(&format!(
            "SELECT {key},Bytes FROM {} ORDER BY 1",
            quote(table)
        ))?;
        let mut live = right.query([])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            if count % 128 == 0 {
                control.check()?;
            }
            count += 1;
            let now = live
                .next()?
                .ok_or_else(|| invalid("当前游戏记录与底稿不同，无法可靠比较。"))?;
            let rid = RowId::from_sql(row.get_ref(0)?)?;
            if RowId::from_sql(now.get_ref(0)?)? != rid {
                return Err(invalid("当前游戏记录与底稿不同，无法可靠比较。"));
            }
            let old = row
                .get_ref(1)?
                .as_blob()
                .map_err(|_| invalid("底稿文本记录格式无效。"))?;
            let now = now
                .get_ref(1)?
                .as_blob()
                .map_err(|_| invalid("当前游戏文本记录格式无效。"))?;
            if old == now {
                continue;
            }
            let old = fields::parse(old, true).map_err(|e| record_error(table, &rid, e))?;
            let now = fields::parse(now, true).map_err(|e| record_error(table, &rid, e))?;
            if old.len() != now.len() || old.iter().zip(&now).any(|(a, b)| a.slot != b.slot) {
                return Err(invalid("当前游戏字段与底稿不同，无法可靠比较。"));
            }
            let changed: HashMap<_, _> = old
                .iter()
                .zip(now)
                .filter(|(a, b)| a.text != b.text)
                .map(|(_, b)| (b.slot, b.text.to_string()))
                .collect();
            if !changed.is_empty() {
                used += 200
                    + changed
                        .values()
                        .map(|s| s.chars().count() * 4 + 200)
                        .sum::<usize>();
                if used > limit {
                    return Ok(None);
                }
                output.insert((table.clone(), rid), changed);
            }
        }
        if live.next()?.is_some() {
            return Err(invalid("当前游戏存在底稿之外的记录，无法可靠比较。"));
        }
    }
    Ok(Some(output))
}
fn targets(con: &Connection, edits: &[TextEdit], tables: &[String]) -> Result<Targets> {
    let mut result = HashMap::new();
    for edit in edits {
        if !tables.contains(&edit.table) {
            return Err(invalid("单条修改的来源表已不存在，请重新查找对应记录。"));
        }
        if edit.replacement.len() > fields::MAX_STRING || fields::forbidden(&edit.replacement) {
            return Err(record_error(
                &edit.table,
                &edit.row_id,
                field_error(edit.slot, invalid("单条修改文字过长或包含控制字符。")),
            ));
        }
        let key = key_name(con, &edit.table)?;
        let sql = format!("SELECT Bytes FROM {} WHERE {key}=?", quote(&edit.table));
        let blob: Vec<u8> = con
            .query_row(&sql, [edit.row_id.value()], |r| r.get(0))
            .map_err(|_| invalid("单条修改对应的记录已不存在，请重新查找。"))?;
        let fields =
            fields::parse(&blob, true).map_err(|e| record_error(&edit.table, &edit.row_id, e))?;
        if fields.iter().find(|f| f.slot == edit.slot).map(|f| f.text)
            != Some(edit.original.as_str())
        {
            return Err(invalid(
                "单条修改的底稿原文已变化，请重新查找；未改动游戏。",
            ));
        }
        let slots = result
            .entry((edit.table.clone(), edit.row_id.clone()))
            .or_insert_with(HashMap::new);
        if slots.insert(edit.slot, edit.replacement.clone()).is_some() {
            return Err(invalid("同一段文本有重复修改，请保留一项。"));
        }
    }
    Ok(result)
}

fn sample(text: &str) -> String {
    let mut chars = text.chars();
    let out: String = chars.by_ref().take(8000).collect();
    if chars.next().is_some() {
        out + "…（文本过长，仅显示前8000字）"
    } else {
        out
    }
}
fn compare(
    stats: &mut Stats,
    table: &str,
    rid: &RowId,
    original: &[u8],
    before: &[u8],
    new: Option<&[u8]>,
) -> Result<()> {
    if original == before && new.is_none() {
        return Ok(());
    }
    let old = fields::parse(original, true)?;
    let before = fields::parse(before, true)?;
    let after = fields::parse(new.unwrap_or(original), true)?;
    if old.len() != before.len()
        || old.len() != after.len()
        || old.iter().zip(&before).any(|(a, b)| a.slot != b.slot)
        || old.iter().zip(&after).any(|(a, b)| a.slot != b.slot)
    {
        return Err(invalid("游戏字段结构发生变化，无法可靠比较。"));
    }
    for ((original, before), after) in old.iter().zip(before).zip(after) {
        if before.text == after.text {
            continue;
        }
        let kind = if after.text == original.text {
            stats.reverted += 1;
            "撤销"
        } else if before.text == original.text {
            stats.added += 1;
            "新增"
        } else {
            stats.updated += 1;
            "调整"
        };
        if stats.changes.len() < 200 {
            stats.changes.push(Change {
                kind: kind.into(),
                table: table.into(),
                row_id: rid.clone(),
                slot: after.slot,
                original: sample(original.text),
                before: sample(before.text),
                after: sample(after.text),
            });
        }
    }
    Ok(())
}

fn compare_override(
    stats: &mut Stats,
    table: &str,
    rid: &RowId,
    blob: &[u8],
    new: Option<&[u8]>,
    overrides: &HashMap<usize, String>,
) -> Result<()> {
    if overrides.is_empty() && new.is_none() {
        return Ok(());
    }
    let original = fields::parse(blob, true)?;
    let after = fields::parse(new.unwrap_or(blob), true)?;
    if original.len() != after.len() || original.iter().zip(&after).any(|(a, b)| a.slot != b.slot) {
        return Err(invalid("游戏字段结构发生变化，无法可靠比较。"));
    }
    for (original, after) in original.iter().zip(after) {
        let before = overrides
            .get(&original.slot)
            .map(String::as_str)
            .unwrap_or(original.text);
        if before == after.text {
            continue;
        }
        let kind = if after.text == original.text {
            stats.reverted += 1;
            "撤销"
        } else if before == original.text {
            stats.added += 1;
            "新增"
        } else {
            stats.updated += 1;
            "调整"
        };
        if stats.changes.len() < 200 {
            stats.changes.push(Change {
                kind: kind.into(),
                table: table.into(),
                row_id: rid.clone(),
                slot: after.slot,
                original: sample(original.text),
                before: sample(before),
                after: sample(after.text),
            });
        }
    }
    Ok(())
}

pub fn patch_database(
    con: &Connection,
    current: Option<&Connection>,
    comparison: Option<&CurrentOverrides>,
    rules: &Rules,
    options: &Options,
    write: bool,
    control: &Control<'_>,
) -> Result<Stats> {
    let todo = tables(con, &options.tables)?;
    rules.validate_tables(&todo)?;
    if let Some(current) = current
        && tables(current, &options.tables)? != todo
    {
        return Err(invalid("当前游戏文本表与底稿不同，请检查游戏资源后重试。"));
    }
    let targets = targets(con, &options.edits, &todo)?;
    let mut stats = Stats {
        tables: todo.clone(),
        ..Default::default()
    };
    let mut evidence = Evidence::new(rules.values.len(), options.explain);
    let empty = HashMap::new();
    if write {
        con.execute_batch("BEGIN IMMEDIATE; CREATE TEMP TABLE _bajptr_updates (Bytes BLOB, rid);")?;
    }
    let result = (|| -> Result<()> {
        for (index, table) in todo.iter().enumerate() {
            control.log(&format!("检查文本表 {}/{}：{table}", index + 1, todo.len()))?;
            let view = rules.view(table);
            let key = match key_name(con, table) {
                Ok(k) => k,
                Err(Error::UnsupportedTable(reason)) => {
                    stats.tables_skipped += 1;
                    note(&mut stats, "table", table, None, None, reason, control)?;
                    continue;
                }
                Err(error) => return Err(invalid(format!("来源表 {table}：{error}"))),
            };
            let sql = format!("SELECT {key}, Bytes FROM {} ORDER BY 1", quote(table));
            let mut query = con.prepare(&sql)?;
            let mut rows = query.query([])?;
            let mut current_query = if let Some(current) = current {
                let key = key_name(current, table)?;
                Some(current.prepare(&format!(
                    "SELECT {key}, Bytes FROM {} ORDER BY 1",
                    quote(table)
                ))?)
            } else {
                None
            };
            let mut live = if let Some(query) = &mut current_query {
                Some(query.query([])?)
            } else {
                None
            };
            let mut row_count = 0u64;
            while let Some(row) = rows.next()? {
                if row_count.is_multiple_of(128) {
                    control.check()?;
                }
                row_count += 1;
                stats.rows_seen += 1;
                let rid = RowId::from_sql(row.get_ref(0)?)?;
                let live_row = if let Some(live) = &mut live {
                    let row = live
                        .next()?
                        .ok_or_else(|| invalid("当前游戏记录与底稿不同，无法可靠比较。"))?;
                    if RowId::from_sql(row.get_ref(0)?)? != rid {
                        return Err(invalid("当前游戏记录与底稿不同，无法可靠比较。"));
                    }
                    Some(row)
                } else {
                    None
                };
                let blob = match row.get_ref(1)? {
                    ValueRef::Blob(b) => b,
                    ValueRef::Null => continue,
                    _ => {
                        stats.parse_failed += 1;
                        note(
                            &mut stats,
                            "parse",
                            table,
                            Some(&rid),
                            None,
                            "Bytes 列不是二进制文本数据。".into(),
                            control,
                        )?;
                        continue;
                    }
                };
                let overrides = targets.get(&(table.clone(), rid.clone())).unwrap_or(&empty);
                let composed = options
                    .edits
                    .iter()
                    .filter(|e| e.wordlist_applied && e.table == *table && e.row_id == rid)
                    .map(|e| e.slot)
                    .collect();
                let (new, bs) = match patch_blob_composed(
                    blob,
                    rules,
                    &view,
                    &mut evidence,
                    options,
                    overrides,
                    &composed,
                ) {
                    Ok(v) => v,
                    Err(Error::Parse(reason)) => {
                        stats.parse_failed += 1;
                        note(
                            &mut stats,
                            "parse",
                            table,
                            Some(&rid),
                            None,
                            format!("FlatBuffer 解析失败：{reason}"),
                            control,
                        )?;
                        continue;
                    }
                    Err(error) => return Err(record_error(table, &rid, error)),
                };
                stats.kana_skipped += bs.kana_skipped;
                stats.fields_skipped += bs.fields_skipped;
                for issue in &bs.issues {
                    note(
                        &mut stats,
                        "field",
                        table,
                        Some(&rid),
                        Some(issue.slot),
                        issue.message.clone(),
                        control,
                    )?;
                }
                if let Some(new) = &new {
                    stats.rows_changed += 1;
                    stats.fields_changed += bs.inplace + bs.appended;
                    stats.inplace += bs.inplace;
                    stats.appended += bs.appended;
                    stats.bytes_appended += bs.bytes_appended;
                    if write {
                        con.execute(
                            "INSERT INTO _bajptr_updates VALUES (?,?)",
                            rusqlite::params![new, rid.value()],
                        )?;
                    }
                }
                if options.explain {
                    if let Some(comparison) = comparison {
                        compare_override(
                            &mut stats,
                            table,
                            &rid,
                            blob,
                            new.as_deref(),
                            comparison
                                .get(&(table.clone(), rid.clone()))
                                .unwrap_or(&empty),
                        )
                        .map_err(|e| record_error(table, &rid, e))?;
                    } else {
                        let before = if let Some(live) = live_row {
                            live.get_ref(1)?
                                .as_blob()
                                .map_err(|_| invalid("当前游戏文本记录类型不同。"))?
                        } else {
                            blob
                        };
                        compare(&mut stats, table, &rid, blob, before, new.as_deref())
                            .map_err(|e| record_error(table, &rid, e))?;
                    }
                }
            }
            if let Some(live) = &mut live
                && live.next()?.is_some()
            {
                return Err(invalid("当前游戏存在底稿之外的记录，无法可靠比较。"));
            }
            drop(rows);
            drop(query);
            if write {
                let mut updates = con.prepare("SELECT Bytes, rid FROM _bajptr_updates")?;
                let mut pending = updates.query([])?;
                let mut update = con.prepare(&format!(
                    "UPDATE {} SET Bytes=? WHERE {key}=?",
                    quote(table)
                ))?;
                while let Some(row) = pending.next()? {
                    control.check()?;
                    update.execute(rusqlite::params![
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Value>(1)?
                    ])?;
                }
                drop(pending);
                drop(updates);
                drop(update);
                con.execute("DELETE FROM _bajptr_updates", [])?;
            }
        }
        if write && stats.parse_failed > 0 {
            let details = stats
                .diagnostics
                .iter()
                .filter(|d| d.kind == "parse")
                .map(Diagnostic::display)
                .collect::<Vec<_>>()
                .join("\n");
            return Err(invalid(format!(
                "{} 条记录解析失败，已停止生成，未发布补丁。\n{}",
                stats.parse_failed, details
            )));
        }
        Ok(())
    })();
    if write {
        if result.is_ok() {
            con.execute_batch("DROP TABLE _bajptr_updates; COMMIT;")?;
        } else {
            let _ = con.execute_batch("ROLLBACK;");
        }
    }
    result?;
    stats.hits = evidence.hits.clone();
    stats.zero_reasons = evidence.zero_reasons();
    Ok(stats)
}

pub type TextRow = (String, RowId, usize, String);
fn read_fields<'a>(
    blob: &'a [u8],
    table: &str,
    rid: &RowId,
    diagnostics: &mut Stats,
    control: &Control<'_>,
) -> Result<Option<Vec<fields::Field<'a>>>> {
    match fields::parse_with_issues(blob, false) {
        Ok(parsed) => {
            diagnostics.fields_skipped += parsed.skipped;
            for issue in parsed.issues {
                note(
                    diagnostics,
                    "field",
                    table,
                    Some(rid),
                    Some(issue.slot),
                    issue.message,
                    control,
                )?;
            }
            Ok(Some(parsed.fields))
        }
        Err(Error::Parse(reason)) => {
            diagnostics.parse_failed += 1;
            note(
                diagnostics,
                "parse",
                table,
                Some(rid),
                None,
                format!("FlatBuffer 解析失败：{reason}"),
                control,
            )?;
            Ok(None)
        }
        Err(error) => Err(record_error(table, rid, error)),
    }
}
fn report_skips(stats: &Stats, control: &Control<'_>) -> Result<()> {
    let mut parts = Vec::new();
    if stats.parse_failed > 0 {
        parts.push(format!("{} 条无法解析的记录", stats.parse_failed));
    }
    if stats.tables_skipped > 0 {
        parts.push(format!("{} 张不支持的来源表", stats.tables_skipped));
    }
    if stats.fields_skipped > 0 {
        parts.push(format!("{} 个异常字段", stats.fields_skipped));
    }
    if !parts.is_empty() {
        control.log(&format!(
            "已跳过{}，原始数据保留；上方最多列出 20 项位置与原因。",
            parts.join("、")
        ))?;
    }
    Ok(())
}
pub fn scan(
    con: &Connection,
    pattern: &str,
    keyword: &str,
    limit: usize,
    control: &Control<'_>,
) -> Result<(Vec<TextRow>, u64)> {
    scan_page(con, pattern, keyword, limit, 0, false, control)
}

/// Scan a bounded page of matches while still counting the complete result.
/// `offset` is measured in matching text fields, so callers can load the next
/// page without pretending that the first page is the whole search result.
pub fn scan_page(
    con: &Connection,
    pattern: &str,
    keyword: &str,
    limit: usize,
    offset: usize,
    skip_kana: bool,
    control: &Control<'_>,
) -> Result<(Vec<TextRow>, u64)> {
    let mut found = Vec::new();
    let mut total = 0;
    let mut diagnostics = Stats::default();
    for table in tables(con, pattern)? {
        control.log(&format!("扫描文本表：{table}"))?;
        let key = match key_name(con, &table) {
            Ok(v) => v,
            Err(Error::UnsupportedTable(reason)) => {
                diagnostics.tables_skipped += 1;
                note(
                    &mut diagnostics,
                    "table",
                    &table,
                    None,
                    None,
                    reason,
                    control,
                )?;
                continue;
            }
            Err(error) => return Err(invalid(format!("来源表 {table}：{error}"))),
        };
        let mut query = con.prepare(&format!(
            "SELECT {key},Bytes FROM {} ORDER BY 1",
            quote(&table)
        ))?;
        let mut rows = query.query([])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            if count % 128 == 0 {
                control.check()?;
            }
            count += 1;
            let Ok(blob) = row.get_ref(1)?.as_blob() else {
                continue;
            };
            if !keyword.is_empty() && !blob.windows(keyword.len()).any(|b| b == keyword.as_bytes())
            {
                continue;
            }
            let rid = RowId::from_sql(row.get_ref(0)?)?;
            let Some(fields) = read_fields(blob, &table, &rid, &mut diagnostics, control)? else {
                continue;
            };
            for field in fields {
                if field.text.contains(keyword) && !(skip_kana && fields::has_kana(field.text)) {
                    total += 1;
                    if total as usize > offset && found.len() < limit {
                        found.push((table.clone(), rid.clone(), field.slot, field.text.into()));
                    }
                }
            }
        }
    }
    report_skips(&diagnostics, control)?;
    Ok((found, total))
}

pub fn export(
    con: &Connection,
    pattern: &str,
    destination: &Path,
    control: &Control<'_>,
) -> Result<u64> {
    std::fs::create_dir_all(destination)?;
    let mut total = 0;
    let mut diagnostics = Stats::default();
    for table in tables(con, pattern)? {
        control.log(&format!("导出来源表：{table}"))?;
        if table.chars().any(|c| "<>:\"/\\|?*".contains(c))
            || PathBuf::from(&table).file_name().is_none()
        {
            return Err(invalid("来源表名称不能作为导出文件名。"));
        }
        let key = match key_name(con, &table) {
            Ok(k) => k,
            Err(Error::UnsupportedTable(reason)) => {
                diagnostics.tables_skipped += 1;
                note(
                    &mut diagnostics,
                    "table",
                    &table,
                    None,
                    None,
                    reason,
                    control,
                )?;
                continue;
            }
            Err(error) => return Err(invalid(format!("来源表 {table}：{error}"))),
        };
        let mut query = con.prepare(&format!(
            "SELECT {key},Bytes FROM {} ORDER BY 1",
            quote(&table)
        ))?;
        let mut rows = query.query([])?;
        let mut output: Option<BufWriter<File>> = None;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            if count % 128 == 0 {
                control.check()?;
            }
            count += 1;
            let Ok(blob) = row.get_ref(1)?.as_blob() else {
                continue;
            };
            let rid = RowId::from_sql(row.get_ref(0)?)?;
            let Some(fields) = read_fields(blob, &table, &rid, &mut diagnostics, control)? else {
                continue;
            };
            for field in fields {
                if output.is_none() {
                    let mut file =
                        BufWriter::new(File::create(destination.join(format!("{table}.tsv")))?);
                    file.write_all(b"rowid\tfield\ttext\n")?;
                    output = Some(file);
                }
                let text = field
                    .text
                    .replace('\r', "\\r")
                    .replace('\n', "\\n")
                    .replace('\t', "\\t");
                writeln!(
                    output.as_mut().unwrap(),
                    "{}\t{}\t{}",
                    rid.display(),
                    field.slot,
                    text
                )?;
                total += 1;
            }
        }
        if let Some(mut file) = output {
            file.flush()?;
        }
    }
    report_skips(&diagnostics, control)?;
    Ok(total)
}
