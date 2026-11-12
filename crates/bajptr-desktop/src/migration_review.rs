use crate::state::{slice, text};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

pub struct ReviewGroup {
    pub members: Vec<usize>,
    pub item: Value,
    pub targets: Vec<String>,
    pub chosen: BTreeSet<usize>,
    pub confirmed: bool,
}
impl ReviewGroup {
    pub fn equal_count(&self) -> bool {
        !self.targets.is_empty() && self.targets.len() == self.members.len()
    }
}

pub struct MigrationForm {
    pub open: bool,
    pub plan: Value,
    pub groups: Vec<ReviewGroup>,
    pub selected: Option<usize>,
    pub filter: String,
    pub show_confirmed: bool,
    pub error: String,
}
impl MigrationForm {
    pub fn new(plan: Value) -> Self {
        let mut groups: Vec<ReviewGroup> = Vec::new();
        let mut positions = HashMap::<(String, Vec<String>), usize>::new();
        for (index, item) in slice(&plan, "items").iter().enumerate() {
            let targets: Vec<_> = slice(item, "candidates")
                .iter()
                .map(|c| {
                    let row = &c["row"];
                    json!([row[0], row[1], row[2]]).to_string()
                })
                .collect();
            let key = text(item, "group_key");
            let signature = (
                if key.is_empty() {
                    format!("ungrouped-{index}")
                } else {
                    key
                },
                targets.clone(),
            );
            if let Some(&position) = positions.get(&signature) {
                groups[position].members.push(index);
            } else {
                positions.insert(signature, groups.len());
                groups.push(ReviewGroup {
                    members: vec![index],
                    item: item.clone(),
                    targets,
                    chosen: BTreeSet::new(),
                    confirmed: false,
                });
            }
        }
        let selected = (!groups.is_empty()).then_some(0);
        Self {
            open: true,
            plan,
            groups,
            selected,
            filter: String::new(),
            show_confirmed: false,
            error: String::new(),
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        let needle = self.filter.trim().to_lowercase();
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.confirmed == self.show_confirmed)
            .filter(|(_, g)| {
                format!(
                    "{} {} {}",
                    text(&g.item, "original"),
                    text(&g.item, "replacement"),
                    bajptr_project::model::title(&text(&g.item, "table"))
                )
                .to_lowercase()
                .contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn sync_selection(&mut self) {
        let visible = self.visible();
        if !visible.iter().any(|&i| Some(i) == self.selected) {
            self.selected = visible.first().copied();
        }
    }
    pub fn next(&mut self) {
        let visible = self.visible();
        self.selected = self
            .selected
            .and_then(|old| visible.iter().copied().find(|&i| i > old))
            .or_else(|| visible.first().copied());
    }
    pub fn reserved(&self, group: usize, candidate: usize) -> bool {
        let target = &self.groups[group].targets[candidate];
        self.groups.iter().enumerate().any(|(i, g)| {
            i != group && g.confirmed && g.chosen.iter().any(|&c| &g.targets[c] == target)
        })
    }
    pub fn has_competing(&self, index: usize) -> bool {
        self.groups.iter().enumerate().any(|(i, g)| {
            i != index
                && g.targets
                    .iter()
                    .any(|target| self.groups[index].targets.contains(target))
        })
    }
    pub fn can_confirm(&self, index: usize) -> bool {
        let group = &self.groups[index];
        let selected: Vec<_> = if group.equal_count() {
            (0..group.targets.len()).collect()
        } else {
            group.chosen.iter().copied().collect()
        };
        !group.confirmed
            && !selected.is_empty()
            && selected.len() <= group.members.len()
            && selected.iter().all(|&c| !self.reserved(index, c))
    }
    pub fn confirm(&mut self, index: usize) -> bool {
        if !self.can_confirm(index) {
            return false;
        }
        let group = &mut self.groups[index];
        if group.equal_count() {
            group.chosen = (0..group.targets.len()).collect();
        }
        group.confirmed = true;
        self.error.clear();
        self.next();
        true
    }
    pub fn confirmed_count(&self) -> usize {
        self.groups
            .iter()
            .filter(|g| g.confirmed)
            .map(|g| g.chosen.len())
            .sum()
    }
    pub fn payload(&self) -> Value {
        let choices: Vec<_> = self
            .groups
            .iter()
            .filter(|g| g.confirmed)
            .flat_map(|g| {
                g.members
                    .iter()
                    .zip(&g.chosen)
                    .map(|(&item, &candidate)| json!({"item":item,"candidate":candidate}))
            })
            .collect();
        json!({"nonce":self.plan["nonce"],"revision":self.plan["revision"],"choices":choices})
    }
    pub fn context_groups(&self, index: usize) -> Vec<(String, Vec<usize>)> {
        let group = &self.groups[index];
        let mut buckets: Vec<(String, Vec<usize>)> = Vec::new();
        for (candidate, value) in slice(&group.item, "candidates").iter().enumerate() {
            let context = text(value, "context");
            if let Some((_, indices)) = buckets.iter_mut().find(|(c, _)| c == &context) {
                indices.push(candidate);
            } else {
                buckets.push((context, vec![candidate]));
            }
        }
        buckets
    }
    pub fn toggle_context(&mut self, index: usize, candidates: &[usize], checked: bool) {
        if self.groups[index].confirmed {
            return;
        }
        if !checked {
            for c in candidates {
                self.groups[index].chosen.remove(c);
            }
            return;
        }
        let group = &self.groups[index];
        let mut next = group.chosen.clone();
        next.extend(candidates);
        if next.len() > group.members.len() || candidates.iter().any(|&c| self.reserved(index, c)) {
            return;
        }
        self.groups[index].chosen = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    fn item(key: &str, targets: &[i64]) -> Value {
        json!({"group_key":key,"original":"神秘","replacement":"神奇","table":"Text","candidates":targets.iter().map(|id|json!({"row":["Text",id,0,"神秘"],"context":format!("上下文{id}")})).collect::<Vec<_>>()})
    }
    #[test]
    fn equivalent_records_confirm_once_and_never_expand_the_saved_scope() {
        let mut form = MigrationForm::new(
            json!({"items":[item("same",&[1,2]),item("same",&[1,2]),item("single",&[3]),item("subset",&[4,5,6])]}),
        );
        assert_eq!(form.groups.len(), 3);
        assert!(form.can_confirm(0) && form.can_confirm(1));
        assert!(form.confirm(0));
        assert!(form.confirm(1));
        assert_eq!(form.confirmed_count(), 3);
        assert_eq!(form.selected, Some(2));
        let payload = form.payload();
        assert_eq!(slice(&payload, "choices").len(), 3);
        assert_eq!(payload["choices"][0], json!({"item":0,"candidate":0}));
        assert_eq!(payload["choices"][1], json!({"item":1,"candidate":1}));
        form.toggle_context(2, &[0, 1], true);
        assert!(form.groups[2].chosen.is_empty());
        form.toggle_context(2, &[1], true);
        assert!(form.confirm(2));
        assert_eq!(form.confirmed_count(), 4);
        assert!(form.selected.is_none());
        form.show_confirmed = true;
        form.sync_selection();
        assert_eq!(form.selected, Some(0));
        assert_eq!(
            form.groups
                .iter()
                .filter(|g| g.confirmed)
                .flat_map(|g| g.chosen.iter().map(|&c| &g.targets[c]))
                .collect::<HashSet<_>>()
                .len(),
            4
        );
    }
    #[test]
    fn competing_replacements_are_never_bulk_confirmed_or_assigned_twice() {
        let mut form = MigrationForm::new(json!({"items":[item("one",&[1]),item("other",&[1])]}));
        assert!(form.has_competing(0) && form.has_competing(1));
        assert!(form.confirm(0));
        assert!(!form.confirm(1));
        assert_eq!(form.confirmed_count(), 1);
    }
}
