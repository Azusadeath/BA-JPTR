use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").into()
}
pub fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}
pub fn list(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
pub fn slice<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
pub fn search_sources<'a>(all: &'a [Value], search: &'a Value, keyword: &str) -> &'a [Value] {
    if search["keyword"].as_str() == Some(keyword)
        && let Some(tables) = search["source_tables"].as_array()
    {
        tables
    } else {
        all
    }
}
pub fn mode_name(mode: &str) -> &'static str {
    match mode {
        "exact" => "整段相同",
        "re" => "正则模板",
        _ => "包含文字",
    }
}

#[derive(Default)]
pub struct SearchInput {
    composing: bool,
}
impl SearchInput {
    pub fn submit(&mut self, events: &[eframe::egui::Event], focused: bool, enabled: bool) -> bool {
        use eframe::egui::{Event, ImeEvent, Key};
        if !focused {
            self.composing = false;
            return false;
        }
        let mut ime_frame = false;
        for event in events {
            match event {
                Event::Ime(ImeEvent::Preedit { text, .. }) => {
                    self.composing = !text.is_empty();
                    ime_frame = true;
                }
                Event::Ime(ImeEvent::Commit(_)) => {
                    self.composing = false;
                    ime_frame = true;
                }
                _ => {}
            }
        }
        enabled && !self.composing && !ime_frame && events.iter().any(|event| matches!(event,
            Event::Key { key: Key::Enter, pressed: true, repeat: false, modifiers, .. } if modifiers.is_none()))
    }
}

#[derive(Default)]
pub struct EditView {
    pub filter: String,
    pub selected: Option<usize>,
}

pub fn edit_matches(entry: &Value, filter: &str) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    let edit = &entry["edit"];
    [
        text(entry, "label"),
        text(edit, "table"),
        text(edit, "original"),
        text(&edit["batch_operation"], "before"),
        text(edit, "replacement"),
    ]
    .join(" ")
    .replace(['\r', '\n'], " ")
    .to_lowercase()
    .contains(&filter.trim().to_lowercase())
}

#[derive(Default)]
pub struct Selection {
    pub rows: BTreeSet<usize>,
    pub anchor: Option<usize>,
}
impl Selection {
    pub fn click(&mut self, index: usize, ctrl: bool, shift: bool) {
        if shift {
            if !ctrl {
                self.rows.clear();
            }
            let from = self.anchor.unwrap_or(index);
            self.rows.extend(from.min(index)..=from.max(index));
        } else if ctrl {
            if !self.rows.remove(&index) {
                self.rows.insert(index);
            }
            self.anchor = Some(index);
        } else {
            self.rows.clear();
            self.rows.insert(index);
            self.anchor = Some(index);
        }
    }
    pub fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
    }
}

pub struct RuleForm {
    pub index: Option<usize>,
    pub revision: u64,
    pub old: String,
    pub new: String,
    pub mode: String,
    pub source: String,
    pub scope: String,
    pub draft: Value,
    pub error: String,
    pub raw: bool,
}
impl RuleForm {
    pub fn new(index: Option<usize>, revision: u64, value: Option<&Value>) -> Self {
        let rule = value
            .and_then(|v| v.get("rule"))
            .cloned()
            .unwrap_or(json!({"mode":"sub","source":"-"}));
        let draft = value
            .and_then(|v| v.get("draft"))
            .cloned()
            .unwrap_or(Value::Null);
        let raw = text(&rule, "mode") == "re" && draft.is_null();
        Self {
            index,
            revision,
            old: text(&rule, "old"),
            new: text(&rule, "new"),
            mode: text(&rule, "mode"),
            source: text(&rule, "source"),
            scope: text(&rule, "table"),
            draft,
            error: String::new(),
            raw,
        }
    }
    pub fn payload(&self) -> Value {
        json!({"index":self.index,"revision":self.revision,"rule":{"old":self.old,"new":self.new,"mode":self.mode,"source":self.source,"table":self.scope},"draft":self.draft})
    }
}
pub struct ScopeForm {
    pub old: String,
    pub mode: String,
    pub request: Option<u64>,
    pub available: Vec<Value>,
    pub checked: BTreeSet<String>,
    pub filter: String,
    pub error: String,
}
impl ScopeForm {
    pub fn new(scope: &str, available: Vec<Value>) -> Self {
        let checked = if scope.starts_with('[') {
            serde_json::from_str::<Vec<String>>(scope)
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else if scope.is_empty() {
            BTreeSet::new()
        } else {
            BTreeSet::from([scope.into()])
        };
        Self {
            old: String::new(),
            mode: "sub".into(),
            request: None,
            available,
            checked,
            filter: String::new(),
            error: String::new(),
        }
    }
    pub fn visible_names(&self) -> BTreeSet<String> {
        let needle = self.filter.to_lowercase();
        self.available
            .iter()
            .filter(|v| {
                format!("{} {}", text(v, "name"), text(v, "label"))
                    .to_lowercase()
                    .contains(&needle)
            })
            .map(|v| text(v, "name"))
            .chain(
                self.checked
                    .iter()
                    .filter(|name| {
                        self.old.is_empty()
                            && !self.available.iter().any(|v| text(v, "name") == **name)
                            && name.to_lowercase().contains(&needle)
                    })
                    .cloned(),
            )
            .collect()
    }
    pub fn update_visible(&mut self, operation: &str) {
        let visible = self.visible_names();
        for name in visible {
            match operation {
                "select" => {
                    self.checked.insert(name);
                }
                "clear" => {
                    self.checked.remove(&name);
                }
                _ => {
                    if !self.checked.remove(&name) {
                        self.checked.insert(name);
                    }
                }
            }
        }
        self.error.clear();
    }
    pub fn encode(&self) -> Result<String, String> {
        if self.checked.is_empty() {
            Err("请至少选择一张来源表。".into())
        } else if self.checked.len() == 1 {
            Ok(self.checked.first().unwrap().clone())
        } else {
            Ok(serde_json::to_string(&self.checked).unwrap())
        }
    }
}
pub struct TextForm {
    pub nonce: String,
    pub label: String,
    pub original: String,
    pub value: String,
    pub has_batch: bool,
    pub show_original: bool,
    pub error: String,
    pub notice: String,
}
pub struct BatchForm {
    pub group_id: String,
    pub indices: Vec<usize>,
    pub old: String,
    pub new: String,
    pub nonce: String,
    pub changes: Vec<Value>,
    pub error: String,
}
#[derive(Default)]
pub struct BatchScopeForm {
    pub id: String,
    pub filter: String,
    pub selected: Option<usize>,
    pub error: String,
}
impl BatchScopeForm {
    pub fn contains(&self, entry: &Value) -> bool {
        entry["edit"]["batch"] == true
            && entry["edit"]["batch_operation"]["id"].as_str() == Some(self.id.as_str())
    }
}
impl BatchForm {
    pub fn matches(&self, draft: &Value) -> bool {
        draft["indices"] == json!(self.indices)
            && text(draft, "group_id") == self.group_id
            && text(draft, "old") == self.old
            && text(draft, "new") == self.new
    }
}
pub struct TemplateForm {
    pub key: String,
    pub values: Vec<String>,
    pub sample: String,
    pub result: String,
    pub built: Option<Value>,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_enter_respects_focus_busy_and_ime_composition() {
        use eframe::egui::{Event, ImeEvent, Key, Modifiers};
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut input = SearchInput::default();
        assert!(input.submit(std::slice::from_ref(&enter), true, true));
        assert!(!input.submit(std::slice::from_ref(&enter), false, true));
        assert!(!input.submit(std::slice::from_ref(&enter), true, false));
        assert!(!input.submit(
            &[Event::Ime(ImeEvent::Preedit {
                text: "shen".into(),
                active_range_chars: None
            })],
            true,
            true
        ));
        assert!(!input.submit(std::slice::from_ref(&enter), true, true));
        assert!(!input.submit(
            &[Event::Ime(ImeEvent::Commit("神秘".into())), enter.clone()],
            true,
            true
        ));
        assert!(input.submit(&[enter], true, true));
    }
    #[test]
    fn edit_filter_finds_text_and_source_without_internal_record_ids() {
        let entry = json!({"index":8,"label":"技能说明","edit":{"table":"Skill","row_id":9606,"slot":2,"original":"第一行\n第二行","replacement":"新内容"}});
        for needle in ["skill", "技能", "第一行 第二行", "新内容"] {
            assert!(edit_matches(&entry, needle));
        }
        assert!(!edit_matches(&entry, "不存在"));
        assert!(!edit_matches(&entry, "9606"));
    }
    #[test]
    fn search_sources_use_all_before_search_and_complete_hits_after_search() {
        let all = vec![
            json!({"name":"A"}),
            json!({"name":"B"}),
            json!({"name":"C"}),
        ];
        assert_eq!(search_sources(&all, &Value::Null, "任务"), all.as_slice());
        let result = json!({"keyword":"任务","source":"A","rows":[{"table":"A"}],"source_tables":[{"name":"A"},{"name":"B"}]});
        assert_eq!(search_sources(&all, &result, "任务"), &all[..2]);
        assert_eq!(search_sources(&all, &result, "另一个词"), all.as_slice());
        let empty = json!({"keyword":"任务","rows":[],"source_tables":[]});
        assert!(search_sources(&all, &empty, "任务").is_empty());
    }
    #[test]
    fn batch_scope_finds_one_of_a_hundred_by_text_and_source() {
        let entries: Vec<_>=(0..100).map(|i|json!({"index":i,"label":"技能说明","edit":{"batch":true,"table":"LocalizeSkillDBSchema","row_id":1000+i,"slot":5,"original":format!("原文第{i}段"),"replacement":if i==99{"独有锚点\n替换结果"}else{"其他结果"},"batch_operation":{"id":"group","before":"批量之前的文字"}}})).collect();
        let mut form = BatchScopeForm {
            id: "group".into(),
            filter: "独有锚点 替换结果".into(),
            ..Default::default()
        };
        assert_eq!(
            entries
                .iter()
                .filter(|e| form.contains(e) && edit_matches(e, &form.filter))
                .map(|e| count(e, "index"))
                .collect::<Vec<_>>(),
            vec![99]
        );
        for filter in ["LOCALIZESKILLDBSCHEMA", "技能说明", "批量之前的文字"] {
            form.filter = filter.into();
            assert!(form.contains(&entries[99]) && edit_matches(&entries[99], &form.filter));
        }
        assert!(!edit_matches(&entries[99], "1099"));
        form.id = "another-group".into();
        form.filter.clear();
        assert!(!form.contains(&entries[99]));
    }
    #[test]
    fn ctrl_shift_selection_preserves_anchor_and_visible_bounds() {
        let mut s = Selection::default();
        s.click(2, false, false);
        s.click(5, true, false);
        assert_eq!(s.rows, BTreeSet::from([2, 5]));
        s.click(8, true, true);
        assert_eq!(s.rows, BTreeSet::from([2, 5, 6, 7, 8]));
        s.click(3, false, true);
        assert_eq!(s.rows, BTreeSet::from([3, 4, 5]));
        s.clear();
        assert!(s.anchor.is_none());
    }
    #[test]
    fn source_selection_requires_explicit_tables() {
        let mut s = ScopeForm::new("A", vec![json!({"name":"A"}), json!({"name":"B"})]);
        s.checked.clear();
        assert!(s.encode().is_err());
        s.update_visible("select");
        assert_eq!(s.encode().unwrap(), r#"["A","B"]"#);
        assert!(
            ScopeForm::new("", vec![json!({"name":"New"})])
                .encode()
                .is_err()
        );
    }
    #[test]
    fn filtered_scope_actions_preserve_hidden_selection() {
        let mut s = ScopeForm::new(
            r#"["A","B"]"#,
            vec![
                json!({"name":"A","label":"剧情"}),
                json!({"name":"B","label":"技能"}),
            ],
        );
        s.filter = "剧情".into();
        s.update_visible("clear");
        assert_eq!(s.checked, BTreeSet::from(["B".into()]));
        s.update_visible("invert");
        assert_eq!(s.checked.len(), 2);
        s.update_visible("invert");
        assert_eq!(s.checked, BTreeSet::from(["B".into()]));
        s.old = "任务".into();
        s.available.retain(|v| text(v, "name") == "A");
        s.filter.clear();
        assert_eq!(s.visible_names(), BTreeSet::from(["A".into()]));
        s.update_visible("select");
        s.update_visible("clear");
        assert_eq!(s.encode().unwrap(), "B");
    }
    #[test]
    fn late_batch_preview_cannot_reenable_save_for_changed_inputs() {
        let mut f = BatchForm {
            group_id: String::new(),
            indices: vec![0, 2],
            old: "攻击".into(),
            new: "爆炸".into(),
            nonce: String::new(),
            changes: vec![],
            error: String::new(),
        };
        let draft = json!({"indices":[0,2],"old":"攻击","new":"爆炸"});
        assert!(f.matches(&draft));
        f.new = "袭击".into();
        assert!(!f.matches(&draft));
    }
}
