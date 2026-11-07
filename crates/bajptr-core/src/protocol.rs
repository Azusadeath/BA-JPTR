use crate::{
    Control, Error, Result, codec,
    database::{self, Database, Options},
    fields, invalid,
    rules::{Evidence, Rule, Rules},
};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

#[derive(Default)]
pub struct Session {
    pub database: Option<Database>,
    pub comparison: Option<(String, database::CurrentOverrides)>,
}
fn text<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("缺少文本参数：{name}")))
}
fn bytes(v: &Value, name: &str) -> Result<Vec<u8>> {
    B64.decode(text(v, name)?)
        .map_err(|_| invalid(format!("无效字节参数：{name}")))
}
fn rules(v: &Value) -> Result<Rules> {
    Rules::new(serde_json::from_value::<Vec<Rule>>(
        v.get("rules").cloned().unwrap_or(json!([])),
    )?)
}
fn options(v: &Value) -> Result<Options> {
    Ok(serde_json::from_value(
        v.get("options").cloned().unwrap_or(json!({})),
    )?)
}
fn connection(session: &Session) -> Result<&rusqlite::Connection> {
    session
        .database
        .as_ref()
        .map(|d| &d.con)
        .ok_or_else(|| invalid("请先读取数据库。"))
}

pub fn execute(session: &mut Session, v: Value, control: &Control<'_>) -> Result<Value> {
    control.check()?;
    match text(&v, "op")? {
        "version" => Ok(json!({"protocol":1,"engine":"rust","version":env!("CARGO_PKG_VERSION")})),
        "capabilities" => {
            let _ = rules(&v)?;
            Ok(json!({"supported":true}))
        }
        "fields" => {
            let data = bytes(&v, "blob")?;
            Ok(serde_json::to_value(fields::parse(
                &data,
                v.get("allow_ascii")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )?)?)
        }
        "blob" => {
            let data = bytes(&v, "blob")?;
            let rules = rules(&v)?;
            let options = options(&v)?;
            let table = v.get("table").and_then(Value::as_str).unwrap_or("");
            let view = rules.view(table);
            let mut evidence = Evidence::new(rules.values.len(), options.explain);
            let overrides: HashMap<usize, String> =
                serde_json::from_value(v.get("overrides").cloned().unwrap_or(json!({})))?;
            let (blob, stats) =
                database::patch_blob(&data, &rules, &view, &mut evidence, &options, &overrides)?;
            Ok(
                json!({"blob":blob.map(|b|B64.encode(b)),"stats":stats,"hits":evidence.hits,"zero_reasons":evidence.zero_reasons()}),
            )
        }
        "inspect" => Ok(serde_json::to_value(codec::inspect(
            Path::new(text(&v, "input")?),
            &codec::key(text(&v, "key")?)?,
            control,
        )?)?),
        "decrypt" => Ok(serde_json::to_value(codec::decrypt(
            Path::new(text(&v, "input")?),
            Path::new(text(&v, "output")?),
            &codec::key(text(&v, "key")?)?,
            control,
        )?)?),
        "encrypt" => {
            let ivs: Vec<String> =
                serde_json::from_value(v.get("ivs").cloned().unwrap_or(json!([])))?;
            let ivs = ivs
                .iter()
                .map(|iv| B64.decode(iv).map_err(|_| invalid("IV 编码无效。")))
                .collect::<Result<Vec<_>>>()?;
            Ok(serde_json::to_value(codec::encrypt(
                Path::new(text(&v, "input")?),
                Path::new(text(&v, "output")?),
                &codec::key(text(&v, "key")?)?,
                &bytes(&v, "salt")?,
                &ivs,
                control,
            )?)?)
        }
        "open" => {
            let key = codec::key(text(&v, "key")?)?;
            session.database = None;
            session.comparison = None;
            session.database = Some(Database::open(
                Path::new(text(&v, "input")?),
                v.get("encrypted").and_then(Value::as_bool).unwrap_or(true),
                &key,
                v.get("max_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(384 * 1024 * 1024),
                control,
            )?);
            Ok(
                json!({"ready":true,"cached_in_memory":session.database.as_ref().unwrap().in_memory}),
            )
        }
        "close" => {
            session.database = None;
            session.comparison = None;
            Ok(json!({"ready":false}))
        }
        "tables" => Ok(json!(database::tables(
            connection(session)?,
            v.get("tables")
                .and_then(Value::as_str)
                .unwrap_or(&database::default_tables())
        )?)),
        "scan" => {
            let (rows, total) = database::scan(
                connection(session)?,
                v.get("tables")
                    .and_then(Value::as_str)
                    .unwrap_or(&database::default_tables()),
                text(&v, "keyword")?,
                v.get("limit").and_then(Value::as_u64).unwrap_or(500) as usize,
                control,
            )?;
            Ok(json!({"rows":rows,"total":total}))
        }
        "export" => Ok(
            json!({"count":database::export(connection(session)?,v.get("tables").and_then(Value::as_str).unwrap_or(&database::default_tables()),Path::new(text(&v,"destination")?),control)?}),
        ),
        "preview" => {
            let rules = rules(&v)?;
            let mut options = options(&v)?;
            options.explain = true;
            let original = &session
                .database
                .as_ref()
                .ok_or_else(|| invalid("请先读取数据库。"))?
                .con;
            let mut current = None;
            let mut comparison = None;
            if let Some(path) = v.get("current").and_then(Value::as_str) {
                let identity = v
                    .get("current_identity")
                    .and_then(Value::as_str)
                    .map(|id| format!("{id}\0{}", options.tables));
                let cached = identity.as_ref().is_some_and(|id| {
                    session
                        .comparison
                        .as_ref()
                        .is_some_and(|(key, _)| key == id)
                });
                if !cached {
                    control.log("正在比较当前游戏和初始底稿…")?;
                    let opened = Database::open(
                        Path::new(path),
                        true,
                        &codec::key(text(&v, "key")?)?,
                        0,
                        control,
                    )?;
                    let overrides = database::current_overrides(
                        original,
                        &opened.con,
                        &options.tables,
                        16 * 1024 * 1024,
                        control,
                    )?;
                    session.comparison = None;
                    if let (Some(identity), Some(overrides)) = (identity, overrides) {
                        session.comparison = Some((identity, overrides));
                    } else {
                        current = Some(opened);
                    }
                }
                if current.is_none() {
                    comparison = session.comparison.as_ref().map(|(_, map)| map);
                }
            }
            Ok(serde_json::to_value(database::patch_database(
                original,
                current.as_ref().map(|d| &d.con),
                comparison,
                &rules,
                &options,
                false,
                control,
            )?)?)
        }
        "patch_file" => {
            let rules = rules(&v)?;
            let options = options(&v)?;
            let con = rusqlite::Connection::open_with_flags(
                Path::new(text(&v, "input")?),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            )?;
            Ok(serde_json::to_value(database::patch_database(
                &con,
                None,
                None,
                &rules,
                &options,
                !v.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
                control,
            )?)?)
        }
        _ => Err(invalid("未知内核操作。")),
    }
}

pub fn envelope(result: Result<Value>) -> Value {
    match result {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => {
            let code = match &error {
                Error::Cancelled => "cancelled",
                Error::Sql(_) => "sqlite",
                Error::Io(_) => "io",
                _ => "invalid",
            };
            json!({"ok":false,"code":code,"error":error.to_string()})
        }
    }
}
