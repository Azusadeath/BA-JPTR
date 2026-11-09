//! Desktop's existing action contract; all project mutation stays on its worker.
use crate::{
    Control, Result, baseline, invalid,
    model::{self, Edit},
    service::{self, Config, Engine, PatchJob},
    sources::{self, Inventory, Notice, Sources},
    storage::{self, Store},
    templates::{self, Templates},
};
use bajptr_core::{
    database::TextRow,
    rules::{Rule, Rules},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};
fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| invalid(format!("缺少文字参数：{key}")))
}
fn index(value: &Value, key: &str) -> Result<usize> {
    value[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid("记录位置无效，请重新打开。"))
}
#[derive(Clone)]
enum Document {
    Rules(Vec<Rule>),
    Edits(Vec<Edit>),
}
pub struct Project {
    pub data: PathBuf,
    pub store: Store,
    pub engine: Engine,
    pub rules: Vec<Rule>,
    pub edits: Vec<Edit>,
    pub game: String,
    pub skip: bool,
    pub revision: u64,
    errors: Value,
    messages: Vec<String>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    pub search: Value,
    searches: VecDeque<Value>,
    preview: Value,
    text_draft: Value,
    batch_draft: Value,
    pub version: Option<String>,
    tables: Value,
    pub application: String,
    templates: Templates,
    source_history: Sources,
    source_current: Option<Vec<String>>,
}
impl Project {
    pub fn new(data: PathBuf) -> Self {
        let root = data.join(".gui");
        Self {
            data,
            store: Store::new(root.clone()),
            engine: Engine::default(),
            rules: vec![],
            edits: vec![],
            game: String::new(),
            skip: true,
            revision: 0,
            errors: json!({}),
            messages: vec![],
            undo: vec![],
            redo: vec![],
            search: Value::Null,
            searches: VecDeque::new(),
            preview: Value::Null,
            text_draft: Value::Null,
            batch_draft: Value::Null,
            version: None,
            tables: json!([]),
            application: "应用状态待核对，先预览".into(),
            templates: Templates::new(root.join("template-drafts.json")),
            source_history: Sources::default(),
            source_current: None,
        }
    }
    fn config(&self) -> Config {
        let mut c = Config::new(self.rules.clone(), self.edits.clone(), self.skip);
        c.rules_path = Some(self.store.root.join("rules.tsv"));
        c.edits_path = Some(self.store.root.join("text-edits.json"));
        c
    }
    fn load(&mut self) -> Result<Value> {
        let settings = self.store.root.join("settings.json");
        if settings.exists() {
            match fs::read(&settings)
                .map_err(crate::Error::Io)
                .and_then(|b| Ok(serde_json::from_slice::<Value>(&b)?))
            {
                Ok(v)
                    if v["format"] == 2
                        && v.get("game").is_none_or(Value::is_string)
                        && v.get("skip_kana").is_none_or(Value::is_boolean) =>
                {
                    self.game = v["game"].as_str().unwrap_or("").into();
                    self.skip = v["skip_kana"].as_bool().unwrap_or(true);
                }
                result => self.messages.push(format!(
                    "设置读取失败，使用默认设置：{}",
                    result
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or("格式无效".into())
                )),
            }
        }
        match self.store.load("rules.tsv", model::parse_rules, vec![]) {
            Ok(v) => self.rules = v,
            Err(e) => self.errors["rules.tsv"] = json!(e.to_string()),
        }
        match self
            .store
            .load("text-edits.json", model::parse_edits, vec![])
        {
            Ok(v) => self.edits = v,
            Err(e) => self.errors["text-edits.json"] = json!(e.to_string()),
        }
        match self
            .store
            .load("source-history.json", Sources::parse, Sources::default())
        {
            Ok(value) => self.source_history = value,
            Err(e) => self.errors["source-history.json"] = json!(e.to_string()),
        }
        self.state()
    }
    fn require(&self, revision: Option<&Value>) -> Result<()> {
        if self.errors.as_object().is_some_and(|o| !o.is_empty()) {
            return Err(invalid(
                "修改记录未能完整读取，已停止保存和替换。原文件已保留，请修复后重新打开工具。",
            ));
        }
        if revision
            .filter(|v| !v.is_null())
            .is_some_and(|v| v.as_u64() != Some(self.revision))
        {
            return Err(invalid("修改记录已变化，请重新打开编辑窗口或重新预览。"));
        }
        Ok(())
    }
    fn changed(&mut self) {
        self.engine.invalidate_prepared();
        self.revision += 1;
        self.preview = Value::Null;
        self.batch_draft = Value::Null;
        self.application = "词表已保存，尚未应用".into();
    }
    fn commit(&mut self, document: Document, record: bool) -> Result<()> {
        self.require(None)?;
        let previous = match document {
            Document::Rules(values) => {
                let values = if let Some(names) = &self.source_current {
                    sources::freeze(&values, &names.iter().cloned().collect())?
                } else {
                    model::normalize(values)?
                };
                self.store
                    .save("rules.tsv", &model::rules_bytes(&values)?)?;
                if values == self.rules {
                    return Ok(());
                }
                Document::Rules(std::mem::replace(&mut self.rules, values))
            }
            Document::Edits(values) => {
                self.store
                    .save("text-edits.json", &model::edits_bytes(&values)?)?;
                if values == self.edits {
                    return Ok(());
                }
                Document::Edits(std::mem::replace(&mut self.edits, values))
            }
        };
        if record {
            self.undo.push(previous);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.changed();
        Ok(())
    }
    fn current_version(&mut self, control: &Control<'_>) -> Result<String> {
        let baseline = self
            .engine
            .baseline(&self.game, &self.data.join("backup"), control)?;
        self.version = Some(baseline.version.clone());
        Ok(baseline.version)
    }
    fn source_notices(&self) -> Result<Vec<Value>> {
        let key = model::game_key(&self.game)?;
        let Some(history) = self.source_history.games.get(&key) else {
            return Ok(vec![]);
        };
        let mut output = Vec::new();
        for notice in &history.notices {
            for (index, rule) in self.rules.iter().enumerate().filter(|(_, r)| {
                (r.old.as_str(), r.new.as_str(), r.mode.as_str())
                    == (
                        notice.old.as_str(),
                        notice.new.as_str(),
                        notice.mode.as_str(),
                    )
            }) {
                if model::scopes(&rule.table)?.is_some_and(|names| names.contains(&notice.table)) {
                    continue;
                }
                output.push(json!({"index":index,"old":notice.old,"new":notice.new,"mode":notice.mode,"table":notice.table,"label":model::title(&notice.table),"fields":notice.fields,"example":notice.example}));
            }
        }
        Ok(output)
    }
    fn synchronize_sources(&mut self, control: &Control<'_>) -> Result<Value> {
        self.require(None)?;
        let version = self.current_version(control)?;
        let key = model::game_key(&self.game)?;
        let metadata = self.store.root.join("tables-cache.json");
        let mut previous = self.source_history.games.get(&key).cloned();
        if previous.is_none() {
            let live = crate::region::locate(&self.game)?;
            let folder = baseline::location(&live, &self.data.join("backup"))?;
            let prefix =
                storage::identity(folder.parent().unwrap())? + std::path::MAIN_SEPARATOR_STR;
            if let Some(cache) = fs::read(&metadata)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                && cache["format"] == 2
                && cache["identity"][0].as_str().is_some_and(|s| {
                    storage::identity(Path::new(s)).is_ok_and(|path| path.starts_with(&prefix))
                })
                && let Some(names) = cache["tables"].as_array().filter(|names| {
                    names
                        .iter()
                        .all(|n| n.as_str().is_some_and(|n| !n.is_empty()))
                })
            {
                previous = Some(Inventory {
                    known: names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    notices: vec![],
                });
            }
        }
        // Do not overwrite the previous cache until migration and notices are durable.
        let names: Vec<String> = serde_json::from_value(self.engine.observed_tables(
            &self.game,
            &self.data.join("backup"),
            &metadata,
            control,
        )?)?;
        let current: BTreeSet<_> = names.iter().cloned().collect();
        if previous.is_none() && self.rules.iter().any(|r| r.table.is_empty()) {
            self.source_current = None;
            return Ok(
                json!({"tables":names.iter().map(|n|model::table_info(n)).collect::<Vec<_>>(),"state":self.state()?,"missing_history":true}),
            );
        }
        let mut history = previous.clone().unwrap_or_else(|| Inventory {
            known: current.clone(),
            notices: vec![],
        });
        let values = sources::freeze(&self.rules, &history.known)?;
        let added: Vec<_> = current.difference(&history.known).cloned().collect();
        if !added.is_empty() && !values.is_empty() {
            let matches = self.engine.source_rule_matches(
                &self.game,
                &self.data.join("backup"),
                &added,
                &values,
                self.skip,
                control,
            )?;
            for found in matches {
                let rule = &values[found.rule_index];
                history.notices.push(Notice {
                    table: found.table,
                    old: rule.old.clone(),
                    new: rule.new.clone(),
                    mode: rule.mode.clone(),
                    fields: found.fields,
                    example: found.example,
                });
            }
        }
        history.known.extend(current);
        let mut next = self.source_history.clone();
        next.games.insert(key, history);
        control.check()?;
        let migrated = self.rules.iter().filter(|r| r.table.is_empty()).count();
        if self.current_version(control)? != version {
            return Err(invalid("来源表核对期间资源版本发生变化，请重新检查底稿。"));
        }
        if values != self.rules {
            self.store.check("rules.tsv")?;
            let bytes = fs::read(self.store.root.join("rules.tsv"))?;
            let recovery = self.store.root.join(format!(
                "rules-before-fixed-sources-{}.tsv",
                &storage::digest(&bytes)[..16]
            ));
            if !recovery.exists() {
                storage::atomic_write(&recovery, &bytes, Some(&None))?;
            }
            self.store
                .save("rules.tsv", &model::rules_bytes(&values)?)?;
            self.rules = values;
            // Migration is a compatibility conversion, not an undoable rule edit.
            self.undo.clear();
            self.redo.clear();
            self.changed();
        }
        self.store
            .save("source-history.json", &serde_json::to_vec(&next)?)?;
        self.source_history = next;
        self.source_current = Some(names.clone());
        self.tables = json!(names);
        // Cache refresh is optional. Source history above is the authoritative inventory.
        let _ = self.engine.tables_cached(
            &self.game,
            &self.data.join("backup"),
            Some(&metadata),
            control,
        );
        Ok(
            json!({"tables":names.iter().map(|n|model::table_info(n)).collect::<Vec<_>>(),"state":self.state()?,"migrated":migrated,"missing_history":false}),
        )
    }
    fn context(&mut self, version: &str, revision: &Value, control: &Control<'_>) -> Result<()> {
        self.require(Some(revision))?;
        if self.current_version(control)? != version {
            return Err(invalid("游戏资源版本已变化，请重新查找对应文本。"));
        }
        Ok(())
    }
    fn search_state(&self) -> Result<Value> {
        if self.search.is_null() || self.search["game"] != model::game_key(&self.game)? {
            return Ok(Value::Null);
        }
        let rules = Rules::new(self.rules.clone())?;
        let key = model::game_key(&self.game)?;
        let version = text(&self.search, "version")?;
        let rows: Vec<TextRow> = serde_json::from_value(self.search["rows"].clone())?;
        let rows=rows.into_iter().map(|r|{let (value,explicit)=model::effective(&r,&self.edits,&key,version,&rules,self.skip)?;Ok(json!({"table":r.0,"label":model::title(&r.0),"row_id":r.1,"slot":r.2,"original":r.3,"state":if explicit{"定点修改"}else if value!=r.3{"词表替换"}else{"原文"},"text":value}))}).collect::<Result<Vec<_>>>()?;
        let source_tables: Vec<_> = self.search["matching_tables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(model::table_info)
            .collect();
        Ok(
            json!({"rows":rows,"source_tables":source_tables,"keyword":self.search["keyword"],"source":self.search["source"],"total":self.search["total"],"version":version,"nonce":self.search["nonce"]}),
        )
    }
    fn batch_entries(&self, id: &str) -> Result<Vec<Edit>> {
        let game = model::game_key(&self.game)?;
        let entries: Vec<_> = self
            .edits
            .iter()
            .filter(|e| {
                e.game == game
                    && e.batch_operation
                        .as_ref()
                        .is_some_and(|operation| operation.id == id)
            })
            .cloned()
            .collect();
        if entries.is_empty() {
            return Err(invalid("这项批量修改已变化，请重新打开。"));
        }
        Ok(entries)
    }
    pub fn state(&self) -> Result<Value> {
        let rules=self.rules.iter().map(|r|{let (old,new,draft)=if r.mode=="re"{self.templates.display(&r.old,&r.new)}else{(r.old.clone(),r.new.clone(),None)};json!({"rule":r,"scope":if r.table.is_empty(){"待选择来源表".into()}else{model::scope_title(&r.table)},"display_old":old,"display_new":new,"draft":draft})}).collect::<Vec<_>>();
        let key = model::game_key(&self.game)?;
        let edits=self.edits.iter().enumerate().filter(|(_,e)|e.game==key).map(|(i,e)|json!({"index":i,"edit":e,"label":model::title(&e.table),"active":self.version.as_ref().is_none_or(|v|v==&e.version)})).collect::<Vec<_>>();
        let mut batches: Vec<Value> = Vec::new();
        let mut positions = std::collections::HashMap::new();
        for (index, edit) in self
            .edits
            .iter()
            .enumerate()
            .filter(|(_, e)| e.game == key && e.batch)
        {
            let active = self.version.as_ref().is_none_or(|v| v == &edit.version);
            if let Some(operation) = &edit.batch_operation {
                if let Some(&position) = positions.get(&operation.id) {
                    let row: &mut Value = &mut batches[position];
                    row["count"] = json!(row["count"].as_u64().unwrap_or(0) + 1);
                    row["active"] = json!(row["active"] == true && active);
                } else {
                    positions.insert(operation.id.clone(), batches.len());
                    batches.push(json!({"id":operation.id,"old":operation.old,"new":operation.new,"count":1,"active":active}));
                }
            } else {
                batches.push(json!({"index":index,"old":edit.original,"new":edit.replacement,"count":1,"active":active,"label":model::title(&edit.table)}));
            }
        }
        Ok(
            json!({"game":self.game,"skip_kana":self.skip,"revision":self.revision,"rules":rules,"edits":edits,"batches":batches,"errors":self.errors,"messages":self.messages,"can_undo":!self.undo.is_empty(),"can_redo":!self.redo.is_empty(),"templates":templates::metadata(),"search":self.search_state()?,"application":self.application,"source_notices":self.source_notices()?}),
        )
    }
    // Only read-operation state is rolled back by cancellation. Persisted edits are never rolled back.
    pub fn snapshot(&self) -> Value {
        json!({"search":self.search,"preview":self.preview,"tables":self.tables,"version":self.version,"application":self.application})
    }
    pub fn restore_snapshot(&mut self, v: &Value) {
        self.search = v["search"].clone();
        self.preview = v["preview"].clone();
        self.tables = v["tables"].clone();
        self.version = v["version"].as_str().map(str::to_string);
        self.application = v["application"].as_str().unwrap_or("").into();
    }
    fn find_search(&self, p: &Value) -> Result<Value> {
        let nonce = text(p, "search_nonce")?;
        self.searches
            .iter()
            .find(|s| {
                s["nonce"] == nonce && s["game"] == model::game_key(&self.game).unwrap_or_default()
            })
            .cloned()
            .ok_or_else(|| invalid("请先重新查找文本。"))
    }
    pub fn dispatch(&mut self, action: &str, p: &Value, control: &Control<'_>) -> Result<Value> {
        if matches!(
            action,
            "tables"
                | "sources"
                | "search"
                | "text_open"
                | "text_save"
                | "batch_preview"
                | "batch_open"
                | "batch_save"
                | "preview"
                | "prepare_patch"
                | "apply"
                | "baseline"
                | "restore"
                | "export_text"
        ) {
            let status = self.engine.baseline_status(
                &self.game,
                &self.data.join("backup"),
                &self.data,
                control,
            )?;
            if status["state"] != "ready" {
                return Err(invalid(
                    "初始底稿尚未就绪或游戏资源已更新，请先检查游戏目录并确认底稿。",
                ));
            }
        }
        match action {
            "load" => self.load(),
            "settings" => {
                if p["revision"].as_u64() != Some(self.revision) {
                    return Err(invalid("设置已变化，请重试。"));
                }
                let game = text(p, "game")?.trim().to_string();
                let skip = p["skip_kana"]
                    .as_bool()
                    .ok_or_else(|| invalid("选项格式无效。"))?;
                storage::atomic_write(
                    &self.store.root.join("settings.json"),
                    &serde_json::to_vec(&json!({"format":2,"game":game,"skip_kana":skip}))?,
                    None,
                )?;
                if (self.game.as_str(), self.skip) != (game.as_str(), skip) {
                    if self.game != game {
                        self.engine.close();
                        self.version = None;
                        self.source_current = None;
                    }
                    self.search = Value::Null;
                    self.searches.clear();
                    self.game = game;
                    self.skip = skip;
                    self.changed();
                }
                self.state()
            }
            "check" => {
                let status = self.engine.baseline_status(
                    &self.game,
                    &self.data.join("backup"),
                    &self.data,
                    control,
                )?;
                self.version = status["baseline"]["version_id"]
                    .as_str()
                    .map(str::to_string);
                self.application = service::status(&self.game, &self.data, &self.config())
                    .unwrap_or("应用状态待核对，先预览".into());
                Ok(
                    json!({"connected":status["state"] == "ready","application":self.application,"baseline_status":status}),
                )
            }
            "baseline_commit" => {
                let token = p
                    .get("token")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| invalid("请先检查游戏目录并确认底稿。"))?;
                let original = self.engine.initialize(
                    &self.game,
                    &self.data.join("backup"),
                    &self.data,
                    Some(token),
                    control,
                )?;
                self.version = Some(original.version.clone());
                self.search = Value::Null;
                self.searches.clear();
                self.preview = Value::Null;
                self.batch_draft = Value::Null;
                self.text_draft = Value::Null;
                self.tables = json!([]);
                self.source_current = None;
                self.application = "底稿已保存，请重新预览".into();
                baseline::info(&original)
            }
            "sources" => self.synchronize_sources(control),
            "sources_ack" => {
                self.require(p.get("revision"))?;
                let key = model::game_key(&self.game)?;
                let mut next = self.source_history.clone();
                if let Some(game) = next.games.get_mut(&key) {
                    game.notices.clear();
                }
                self.store
                    .save("source-history.json", &serde_json::to_vec(&next)?)?;
                self.source_history = next;
                self.state()
            }
            "tables" => {
                let names: Vec<String> = serde_json::from_value(self.engine.tables_cached(
                    &self.game,
                    &self.data.join("backup"),
                    Some(&self.store.root.join("tables-cache.json")),
                    control,
                )?)?;
                self.tables = json!(names);
                Ok(json!(
                    names
                        .iter()
                        .map(|n| model::table_info(n))
                        .collect::<Vec<_>>()
                ))
            }
            "save_rule" => {
                self.require(p.get("revision"))?;
                let rule: Rule = serde_json::from_value(p["rule"].clone())?;
                let mut values = self.rules.clone();
                if p["index"].is_null() {
                    values.push(rule.clone());
                } else {
                    let i = index(p, "index")?;
                    *values
                        .get_mut(i)
                        .ok_or_else(|| invalid("词条位置已变化，请重新打开。"))? = rule.clone();
                }
                self.commit(Document::Rules(values), true)?;
                if p["draft"].is_object()
                    && let Err(e) = self.templates.remember(&rule.old, &rule.new, &p["draft"])
                {
                    control.log(&format!("词条已保存；模板表单未保存：{e}"))?;
                }
                self.state()
            }
            "remove_rule" => {
                self.require(p.get("revision"))?;
                let mut values = self.rules.clone();
                let i = index(p, "index")?;
                if i >= values.len() {
                    return Err(invalid("词条位置已变化。"));
                }
                values.remove(i);
                self.commit(Document::Rules(values), true)?;
                self.state()
            }
            "history" => {
                self.require(None)?;
                let undo = p["undo"]
                    .as_bool()
                    .ok_or_else(|| invalid("撤销方向无效。"))?;
                let value = if undo {
                    self.undo.last()
                } else {
                    self.redo.last()
                }
                .cloned();
                if let Some(value) = value {
                    let current = match &value {
                        Document::Rules(_) => Document::Rules(self.rules.clone()),
                        Document::Edits(_) => Document::Edits(self.edits.clone()),
                    };
                    self.commit(value, false)?;
                    if undo {
                        self.undo.pop();
                        self.redo.push(current);
                    } else {
                        self.redo.pop();
                        self.undo.push(current);
                    }
                }
                self.state()
            }
            "import_read" => {
                let rules = model::parse_rules(&fs::read(text(p, "path")?)?)?;
                if rules.is_empty() {
                    return Err(invalid("词表中没有可用词条。"));
                }
                Ok(json!({"count":rules.len(),"rules":rules,"revision":self.revision}))
            }
            "import_commit" => {
                self.require(p.get("revision"))?;
                self.commit(
                    Document::Rules(serde_json::from_value(p["rules"].clone())?),
                    true,
                )?;
                self.state()
            }
            "export_rules" => {
                self.require(None)?;
                storage::atomic_write(
                    Path::new(text(p, "path")?),
                    &model::rules_bytes(&self.rules)?,
                    None,
                )?;
                Ok(json!({"message":"词表已导出。"}))
            }
            "template" => {
                let key = text(p, "key")?;
                let values: Vec<String> = serde_json::from_value(p["values"].clone())?;
                let (old, new) = templates::build(key, &values)?;
                let sample = p["sample"].as_str().unwrap_or("");
                let (result, count) = bajptr_core::rules::regex_sample(&old, &new, sample)?;
                Ok(
                    json!({"old":old,"new":new,"sample":result,"count":count,"draft":{"key":key,"values":values,"sample":sample}}),
                )
            }
            "search" | "search_more" => {
                let keyword = text(p, "keyword")?;
                let source = p.get("source").and_then(Value::as_str).unwrap_or("");
                let offset = p.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let (result, version) = self.engine.scan(
                    &self.game,
                    &self.data.join("backup"),
                    keyword,
                    source,
                    500,
                    offset,
                    self.skip,
                    control,
                )?;
                control.check()?;
                self.version = Some(version.clone());
                if action == "search_more"
                    && self.search["keyword"] == keyword
                    && self.search["source"] == source
                    && self.search["version"] == version
                {
                    let mut rows = self.search["rows"].as_array().cloned().unwrap_or_default();
                    rows.extend(result["rows"].as_array().cloned().unwrap_or_default());
                    self.search["rows"] = json!(rows);
                    self.search["total"] = result["total"].clone();
                } else {
                    self.search = json!({"game":model::game_key(&self.game)?,"version":version,"rows":result["rows"],"total":result["total"],"nonce":nonce(),"keyword":keyword,"source":source});
                }
                self.search["matching_tables"] = result["matching_tables"].clone();
                if self.searches.len() >= 4 {
                    self.searches.pop_front();
                }
                self.searches.push_back(self.search.clone());
                self.search_state()
            }
            "text_open" => {
                self.require(None)?;
                let (version, row, batch) = if p.get("edit_index").is_some() {
                    let e = self
                        .edits
                        .get(index(p, "edit_index")?)
                        .ok_or_else(|| invalid("修改位置已变化。"))?;
                    if e.game != model::game_key(&self.game)? {
                        return Err(invalid("这项修改不属于当前游戏。"));
                    }
                    (
                        e.version.clone(),
                        (
                            e.table.clone(),
                            e.row_id.clone(),
                            e.slot,
                            e.original.clone(),
                        ),
                        e.batch,
                    )
                } else {
                    let search = self.find_search(p)?;
                    let rows: Vec<TextRow> = serde_json::from_value(search["rows"].clone())?;
                    (
                        text(&search, "version")?.to_string(),
                        rows.get(index(p, "index")?)
                            .ok_or_else(|| invalid("文本位置已变化。"))?
                            .clone(),
                        false,
                    )
                };
                self.context(&version, &json!(self.revision), control)?;
                let key = model::game_key(&self.game)?;
                let related: Vec<_> = self
                    .edits
                    .iter()
                    .filter(|e| {
                        e.game == key
                            && e.version == version
                            && e.table == row.0
                            && e.row_id == row.1
                            && e.slot == row.2
                    })
                    .collect();
                let grouped_batch = related
                    .iter()
                    .find(|e| e.batch && e.batch_operation.is_some());
                let single = related.iter().find(|e| !e.batch);
                let (mut value, _) = model::effective(
                    &row,
                    &self.edits,
                    &model::game_key(&self.game)?,
                    &version,
                    &Rules::new(self.rules.clone())?,
                    self.skip,
                )?;
                if !batch && let Some(batch_edit) = grouped_batch {
                    value = single.map(|e| e.replacement.clone()).unwrap_or_else(|| {
                        batch_edit.batch_operation.as_ref().unwrap().before.clone()
                    });
                }
                let nonce = nonce();
                self.text_draft = json!({"nonce":nonce,"row":row,"version":version,"revision":self.revision,"initial":value,"batch":batch});
                Ok(
                    json!({"nonce":nonce,"original":row.3,"text":value,"label":model::title(&row.0),"row_id":row.1,"slot":row.2,"has_batch":!batch && grouped_batch.is_some()}),
                )
            }
            "text_save" => {
                let draft = self.text_draft.clone();
                if draft.is_null() || p["nonce"] != draft["nonce"] {
                    return Err(invalid("编辑上下文已失效，请重新打开。"));
                }
                let version = text(&draft, "version")?;
                self.context(version, &draft["revision"], control)?;
                let replacement = text(p, "text")?.replace("\r\n", "\n").replace('\r', "\n");
                if json!(replacement) != draft["initial"] {
                    let row: TextRow = serde_json::from_value(draft["row"].clone())?;
                    let e = Edit {
                        game: model::game_key(&self.game)?,
                        version: version.into(),
                        table: row.0,
                        row_id: row.1,
                        slot: row.2,
                        original: row.3,
                        replacement,
                        batch: draft["batch"] == true,
                        batch_operation: None,
                    };
                    self.commit(
                        Document::Edits(model::update_edits(&self.edits, vec![e])?),
                        true,
                    )?;
                }
                self.text_draft = Value::Null;
                self.state()
            }
            "batch_open" => {
                self.require(None)?;
                let entries = self.batch_entries(text(p, "id")?)?;
                self.context(&entries[0].version, &json!(self.revision), control)?;
                let operation = entries[0].batch_operation.as_ref().unwrap();
                Ok(
                    json!({"id":operation.id,"old":operation.old,"new":operation.new,"count":entries.len()}),
                )
            }
            "batch_preview" => {
                self.require(None)?;
                let group_id = p.get("group_id").and_then(Value::as_str).unwrap_or("");
                let existing = if group_id.is_empty() {
                    None
                } else {
                    Some(self.batch_entries(group_id)?)
                };
                let (version, rows) = if let Some(entries) = &existing {
                    (
                        entries[0].version.clone(),
                        entries
                            .iter()
                            .map(|e| {
                                (
                                    e.table.clone(),
                                    e.row_id.clone(),
                                    e.slot,
                                    e.original.clone(),
                                )
                            })
                            .collect::<Vec<TextRow>>(),
                    )
                } else {
                    let search = self.find_search(p)?;
                    (
                        text(&search, "version")?.to_string(),
                        serde_json::from_value::<Vec<TextRow>>(search["rows"].clone())?,
                    )
                };
                self.context(&version, &json!(self.revision), control)?;
                let old = text(p, "old")?;
                let new = text(p, "new")?;
                if old.is_empty() {
                    return Err(invalid("请填写要替换的文字。"));
                }
                let mut indices: Vec<usize> = serde_json::from_value(p["indices"].clone())?;
                indices.sort();
                indices.dedup();
                if existing.is_some() && indices != (0..rows.len()).collect::<Vec<_>>() {
                    return Err(invalid("批量修改的范围已变化，请重新打开。"));
                }
                let rules = Rules::new(self.rules.clone())?;
                let key = model::game_key(&self.game)?;
                let matching: Vec<_> = self
                    .edits
                    .iter()
                    .filter(|e| e.game == key && e.version == version)
                    .filter_map(|e| {
                        e.batch_operation
                            .as_ref()
                            .filter(|operation| operation.old == old && operation.new == new)
                            .map(|operation| (e, operation))
                    })
                    .collect();
                let operation_id = matching
                    .first()
                    .map(|(_, operation)| operation.id.clone())
                    .unwrap_or_else(|| {
                        if group_id.is_empty() {
                            nonce()
                        } else {
                            group_id.to_string()
                        }
                    });
                let mut changes = Vec::new();
                let mut updates = Vec::new();
                for i in &indices {
                    let row = rows
                        .get(*i)
                        .ok_or_else(|| invalid("选中文字位置已变化。"))?;
                    let (before, _) =
                        model::effective(row, &self.edits, &key, &version, &rules, self.skip)?;
                    let previous = existing
                        .as_ref()
                        .map(|entries| entries[*i].batch_operation.as_ref().unwrap())
                        .or_else(|| {
                            matching
                                .iter()
                                .find(|(e, _)| {
                                    e.table == row.0 && e.row_id == row.1 && e.slot == row.2
                                })
                                .map(|(_, operation)| *operation)
                        });
                    let base = previous
                        .map(|operation| operation.before.clone())
                        .unwrap_or_else(|| before.clone());
                    let after = base.replace(old, new);
                    if before != after
                        || previous
                            .is_some_and(|operation| operation.old != old || operation.new != new)
                    {
                        changes.push(json!({"label":model::title(&row.0),"row_id":row.1,"slot":row.2,"before":before,"after":after}));
                        updates.push(Edit {
                            game: key.clone(),
                            version: version.clone(),
                            table: row.0.clone(),
                            row_id: row.1.clone(),
                            slot: row.2,
                            original: row.3.clone(),
                            replacement: after,
                            batch: true,
                            batch_operation: Some(model::BatchOperation {
                                id: operation_id.clone(),
                                old: old.into(),
                                new: new.into(),
                                before: base,
                            }),
                        });
                    }
                }
                let mut edits = model::update_edits(&self.edits, updates)?;
                // Saving the same replacement extends its selected scope. Keep each
                // member's original base, including when earlier saves had separate IDs.
                for edit in &mut edits {
                    if edit.game == key
                        && edit.version == version
                        && let Some(operation) = &mut edit.batch_operation
                        && operation.old == old
                        && operation.new == new
                    {
                        operation.id.clone_from(&operation_id);
                    }
                }
                let nonce = nonce();
                self.batch_draft =
                    json!({"nonce":nonce,"edits":edits,"revision":self.revision,"version":version});
                Ok(
                    json!({"nonce":nonce,"changes":changes,"draft":{"indices":indices,"old":old,"new":new,"group_id":group_id}}),
                )
            }
            "batch_save" => {
                let draft = self.batch_draft.clone();
                if draft.is_null() || p["nonce"] != draft["nonce"] {
                    return Err(invalid("批量预览已失效，请重新预览。"));
                }
                self.context(text(&draft, "version")?, &draft["revision"], control)?;
                self.commit(
                    Document::Edits(serde_json::from_value(draft["edits"].clone())?),
                    true,
                )?;
                self.state()
            }
            "remove_batch_member" => {
                let revision = p
                    .get("revision")
                    .ok_or_else(|| invalid("请重新打开批量文本范围。"))?;
                self.require(Some(revision))?;
                let id = text(p, "id")?;
                let i = index(p, "index")?;
                let entry = self
                    .edits
                    .get(i)
                    .ok_or_else(|| invalid("该文本段已变化，请重新打开范围。"))?;
                if entry.game != model::game_key(&self.game)?
                    || !entry.batch
                    || !entry
                        .batch_operation
                        .as_ref()
                        .is_some_and(|operation| operation.id == id)
                {
                    return Err(invalid("该文本段不属于这项批量修改，未移除任何记录。"));
                }
                let mut values = self.edits.clone();
                values.remove(i);
                self.commit(Document::Edits(values), true)?;
                self.state()
            }
            "remove_batch" => {
                self.require(p.get("revision"))?;
                let id = text(p, "id")?;
                self.batch_entries(id)?;
                let game = model::game_key(&self.game)?;
                let values = self
                    .edits
                    .iter()
                    .filter(|e| {
                        e.game != game
                            || !e
                                .batch_operation
                                .as_ref()
                                .is_some_and(|operation| operation.id == id)
                    })
                    .cloned()
                    .collect();
                self.commit(Document::Edits(values), true)?;
                self.state()
            }
            "remove_edit" => {
                self.require(p.get("revision"))?;
                let mut values = self.edits.clone();
                let i = index(p, "index")?;
                if i >= values.len() {
                    return Err(invalid("修改位置已变化。"));
                }
                let removed = values.remove(i);
                if !removed.batch {
                    let row = (
                        removed.table.clone(),
                        removed.row_id.clone(),
                        removed.slot,
                        removed.original.clone(),
                    );
                    let base = model::effective(
                        &row,
                        &[],
                        &removed.game,
                        &removed.version,
                        &Rules::new(self.rules.clone())?,
                        self.skip,
                    )?
                    .0;
                    for batch in values
                        .iter_mut()
                        .filter(|e| e.batch && e.same_address(&removed))
                    {
                        if let Some(operation) = &mut batch.batch_operation {
                            operation.before.clone_from(&base);
                            batch.replacement = base.replace(&operation.old, &operation.new);
                        }
                    }
                }
                self.commit(Document::Edits(values), true)?;
                self.state()
            }
            "preview" => {
                self.require(None)?;
                let stats = self
                    .engine
                    .preview(&self.game, &self.data, &self.config(), control)?;
                let nonce = nonce();
                self.preview = json!({"nonce":nonce,"stats":stats,"revision":self.revision});
                Ok(json!({"nonce":nonce,"total_changes":service::total(&stats),"stats":stats}))
            }
            "prepare_patch" | "apply" => {
                self.require(None)?;
                let draft = self.preview.clone();
                if draft.is_null()
                    || p["nonce"] != draft["nonce"]
                    || draft["revision"] != self.revision
                {
                    return Err(invalid("替换设置或预览已变化，请重新预览。"));
                }
                let count = service::total(&draft["stats"]);
                if count == 0 || draft["stats"]["parse_failed"] != 0 {
                    return Err(invalid("当前预览无法应用，请检查结果。"));
                }
                let config = self.config();
                let backup_root = self.data.join("backup");
                let output_root = self.data.join("output");
                let job = PatchJob {
                    game: &self.game,
                    data: &self.data,
                    backup_root: &backup_root,
                    output_root: &output_root,
                    config: &config,
                    install: true,
                    expected: Some(&draft["stats"]["comparison_identity"]),
                };
                if action == "prepare_patch" {
                    let mut result = self.engine.prepare(job, control)?;
                    result["nonce"] = draft["nonce"].clone();
                    return Ok(result);
                }
                let mut result = if p["prepared"] == true {
                    self.engine.apply_prepared(job, control)?
                } else {
                    self.engine.generate(job, control)?
                };
                if result["installed"] != true {
                    self.application = "应用未完成，补丁已保存".into();
                    let output = Path::new(text(&result, "output")?)
                        .parent()
                        .unwrap_or(&self.data)
                        .display()
                        .to_string();
                    result["message"] = json!("应用到游戏失败，已自动保存校验通过的补丁。");
                    result["warning"] = json!(format!(
                        "应用到游戏失败：{}\n\n已自动保存校验通过的补丁：\n{output}\n\n本次修改尚未确认应用成功。请退出游戏并检查目录写入权限后重试。",
                        text(&result, "install_error")?
                    ));
                    result["application"] = json!(self.application);
                    return Ok(result);
                }
                self.preview = Value::Null;
                self.application = "已应用，与当前词表一致".into();
                result["message"] = json!(format!("已应用 {count} 处变化，校验通过。"));
                result["application"] = json!(self.application);
                Ok(result)
            }
            "baseline" => {
                let original =
                    self.engine
                        .baseline(&self.game, &self.data.join("backup"), control)?;
                self.version = Some(original.version.clone());
                baseline::info(&original)
            }
            "restore" => {
                self.engine
                    .restore(&self.game, &self.data, &self.data.join("backup"), control)?;
                self.preview = Value::Null;
                self.application = "已恢复底稿；词表尚未应用".into();
                Ok(
                    json!({"message":"已恢复为当前版本初始底稿，词表、单条及批量修改记录已保留。","application":self.application}),
                )
            }
            "export_text" => {
                let path = text(p, "path")?;
                let count = self.engine.export(
                    &self.game,
                    &self.data.join("backup"),
                    Path::new(path),
                    control,
                )?;
                Ok(
                    json!({"output":path,"message":format!("已导出 {count} 条底稿文本。"),"count":count}),
                )
            }
            _ => Err(invalid("未知界面操作。")),
        }
    }
}
