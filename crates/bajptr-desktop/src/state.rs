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
pub fn mode_name(mode: &str) -> &'static str {
    match mode {
        "exact" => "整段相同",
        "re" => "正则表达式",
        _ => "包含文字",
    }
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
    pub available: Vec<Value>,
    pub checked: BTreeSet<String>,
    pub unrestricted: bool,
    pub filter: String,
    pub manual: String,
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
            available,
            checked,
            unrestricted: scope.is_empty(),
            filter: String::new(),
            manual: String::new(),
            error: String::new(),
        }
    }
    pub fn all_names(&self) -> BTreeSet<String> {
        self.available
            .iter()
            .map(|v| text(v, "name"))
            .chain(self.checked.iter().cloned())
            .collect()
    }
    pub fn toggle_unrestricted(&mut self, enabled: bool) {
        if self.unrestricted && !enabled {
            self.checked = self.all_names();
        }
        self.unrestricted = enabled;
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
                        !self.available.iter().any(|v| text(v, "name") == **name)
                            && name.to_lowercase().contains(&needle)
                    })
                    .cloned(),
            )
            .collect()
    }
    pub fn update_visible(&mut self, operation: &str) {
        let visible = self.visible_names();
        self.toggle_unrestricted(false);
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
        if self.unrestricted {
            Ok(String::new())
        } else if self.checked.is_empty() {
            Err("请至少选择一张来源表，或勾选「不限来源」。".into())
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
    fn empty_scope_selection_never_becomes_unrestricted() {
        let mut s = ScopeForm::new("A", vec![json!({"name":"A"}), json!({"name":"B"})]);
        s.checked.clear();
        assert!(s.encode().is_err());
        s.toggle_unrestricted(true);
        assert_eq!(s.encode().unwrap(), "");
        s.toggle_unrestricted(false);
        assert_eq!(s.checked, BTreeSet::from(["A".into(), "B".into()]));
    }
    #[test]
    fn filtered_scope_actions_preserve_hidden_selection() {
        let mut s = ScopeForm::new(
            "",
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
