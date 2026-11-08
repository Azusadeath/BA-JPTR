//! Desktop's existing action contract; all project mutation stays on its worker.
use crate::{
    Control, Result, baseline, invalid,
    model::{self, Edit},
    service::{self, Config, Engine, PatchJob},
    storage::{self, Store},
    templates::{self, Templates},
};
use bajptr_core::{
    database::TextRow,
    rules::{Rule, Rules},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
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
        self.revision += 1;
        self.preview = Value::Null;
        self.batch_draft = Value::Null;
        self.application = "词表已保存，尚未应用".into();
    }
    fn commit(&mut self, document: Document, record: bool) -> Result<()> {
        self.require(None)?;
        let previous = match document {
            Document::Rules(values) => {
                let values = model::normalize(values)?;
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
        Ok(
            json!({"rows":rows,"keyword":self.search["keyword"],"source":self.search["source"],"total":self.search["total"],"version":version,"nonce":self.search["nonce"]}),
        )
    }
    pub fn state(&self) -> Result<Value> {
        let rules=self.rules.iter().map(|r|{let (old,new,draft)=if r.mode=="re"{self.templates.display(&r.old,&r.new)}else{(r.old.clone(),r.new.clone(),None)};json!({"rule":r,"scope":model::scope_title(&r.table),"display_old":old,"display_new":new,"draft":draft})}).collect::<Vec<_>>();
        let key = model::game_key(&self.game)?;
        let edits=self.edits.iter().enumerate().filter(|(_,e)|e.game==key).map(|(i,e)|json!({"index":i,"edit":e,"label":model::title(&e.table),"active":self.version.as_ref().is_none_or(|v|v==&e.version)})).collect::<Vec<_>>();
        Ok(
            json!({"game":self.game,"skip_kana":self.skip,"revision":self.revision,"rules":rules,"edits":edits,"errors":self.errors,"messages":self.messages,"can_undo":!self.undo.is_empty(),"can_redo":!self.redo.is_empty(),"templates":templates::metadata(),"search":self.search_state()?,"application":self.application}),
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
                | "search"
                | "text_open"
                | "text_save"
                | "batch_preview"
                | "batch_save"
                | "preview"
                | "generate"
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
                        self.search = Value::Null;
                        self.searches.clear();
                        self.version = None;
                    }
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
                self.application = "底稿已保存，请重新预览".into();
                baseline::info(&original)
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
                if self.searches.len() >= 4 {
                    self.searches.pop_front();
                }
                self.searches.push_back(self.search.clone());
                self.search_state()
            }
            "text_open" => {
                self.require(None)?;
                let (version, row) = if p.get("edit_index").is_some() {
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
                    )
                } else {
                    let search = self.find_search(p)?;
                    let rows: Vec<TextRow> = serde_json::from_value(search["rows"].clone())?;
                    (
                        text(&search, "version")?.to_string(),
                        rows.get(index(p, "index")?)
                            .ok_or_else(|| invalid("文本位置已变化。"))?
                            .clone(),
                    )
                };
                self.context(&version, &json!(self.revision), control)?;
                let (value, _) = model::effective(
                    &row,
                    &self.edits,
                    &model::game_key(&self.game)?,
                    &version,
                    &Rules::new(self.rules.clone())?,
                    self.skip,
                )?;
                let nonce = nonce();
                self.text_draft = json!({"nonce":nonce,"row":row,"version":version,"revision":self.revision,"initial":value});
                Ok(
                    json!({"nonce":nonce,"original":row.3,"text":value,"label":model::title(&row.0),"row_id":row.1,"slot":row.2}),
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
                        batch: false,
                    };
                    self.commit(
                        Document::Edits(model::update_edits(&self.edits, vec![e])?),
                        true,
                    )?;
                }
                self.text_draft = Value::Null;
                self.state()
            }
            "batch_preview" => {
                self.require(None)?;
                let search = self.find_search(p)?;
                let version = text(&search, "version")?;
                self.context(version, &json!(self.revision), control)?;
                let old = text(p, "old")?;
                let new = text(p, "new")?;
                if old.is_empty() {
                    return Err(invalid("请填写要替换的文字。"));
                }
                let mut indices: Vec<usize> = serde_json::from_value(p["indices"].clone())?;
                indices.sort();
                indices.dedup();
                let rows: Vec<TextRow> = serde_json::from_value(search["rows"].clone())?;
                let rules = Rules::new(self.rules.clone())?;
                let key = model::game_key(&self.game)?;
                let mut changes = Vec::new();
                let mut updates = Vec::new();
                for i in &indices {
                    let row = rows
                        .get(*i)
                        .ok_or_else(|| invalid("选中文字位置已变化。"))?;
                    let (before, _) =
                        model::effective(row, &self.edits, &key, version, &rules, self.skip)?;
                    let after = before.replace(old, new);
                    if before != after {
                        changes.push(json!({"label":model::title(&row.0),"row_id":row.1,"slot":row.2,"before":before,"after":after}));
                        updates.push(Edit {
                            game: key.clone(),
                            version: version.into(),
                            table: row.0.clone(),
                            row_id: row.1.clone(),
                            slot: row.2,
                            original: row.3.clone(),
                            replacement: after,
                            batch: true,
                        });
                    }
                }
                let edits = model::update_edits(&self.edits, updates)?;
                let nonce = nonce();
                self.batch_draft =
                    json!({"nonce":nonce,"edits":edits,"revision":self.revision,"version":version});
                Ok(
                    json!({"nonce":nonce,"changes":changes,"draft":{"indices":indices,"old":old,"new":new}}),
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
            "remove_edit" => {
                self.require(p.get("revision"))?;
                let mut values = self.edits.clone();
                let i = index(p, "index")?;
                if i >= values.len() {
                    return Err(invalid("修改位置已变化。"));
                }
                values.remove(i);
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
            "generate" => {
                self.require(None)?;
                let mut result = self.engine.generate(
                    PatchJob {
                        game: &self.game,
                        data: &self.data,
                        backup_root: &self.data.join("backup"),
                        output_root: &self.data.join("output"),
                        config: &self.config(),
                        install: false,
                        expected: None,
                    },
                    control,
                )?;
                result["message"] = json!("补丁已生成，校验通过。游戏未改动。");
                Ok(result)
            }
            "apply" => {
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
                let mut result = self.engine.generate(
                    PatchJob {
                        game: &self.game,
                        data: &self.data,
                        backup_root: &self.data.join("backup"),
                        output_root: &self.data.join("output"),
                        config: &self.config(),
                        install: true,
                        expected: Some(&draft["stats"]["comparison_identity"]),
                    },
                    control,
                )?;
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
                    json!({"message":"已恢复为当前版本初始底稿，词表和单条修改已保留。","application":self.application}),
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
