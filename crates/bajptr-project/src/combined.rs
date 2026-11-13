use crate::{
    Result, invalid,
    model::{self, Edit},
};
use bajptr_core::rules::Rule;
use std::collections::BTreeSet;

struct HitGroup {
    old: String,
    new: String,
    rules: bajptr_core::rules::Rules,
    edits: Vec<Edit>,
    tables: BTreeSet<String>,
    batch_tables: BTreeSet<String>,
    count: u64,
}
impl HitGroup {
    fn candidate(&self, row: &bajptr_core::database::TextRow) -> bool {
        self.batch_tables.contains(&row.0)
            || (self.tables.contains(&row.0) && row.3.contains(&self.old))
    }
}

pub fn hits(
    con: &rusqlite::Connection,
    rules: &[Rule],
    edits: &[Edit],
    game: &str,
    version: &str,
    skip: bool,
    control: &crate::Control<'_>,
) -> Result<serde_json::Value> {
    let pairs = edits
        .iter()
        .filter(|edit| edit.game == game && edit.version == version && edit.batch)
        .filter_map(|edit| edit.batch_operation.as_ref())
        .filter(|op| {
            rules
                .iter()
                .any(|rule| rule_matches(rule, &op.old, &op.new))
        })
        .map(|op| (op.old.clone(), op.new.clone()))
        .collect::<BTreeSet<_>>();
    if pairs.is_empty() {
        return Ok(serde_json::json!([]));
    }
    let compiled = bajptr_core::rules::Rules::new(rules.to_vec())?;
    let mut groups = pairs
        .into_iter()
        .map(|(old, new)| {
            let without_rules = rules
                .iter()
                .filter(|rule| !rule_matches(rule, &old, &new))
                .cloned()
                .collect::<Vec<_>>();
            let without_edits = edits
                .iter()
                .filter(|edit| !batch_matches(edit, game, version, &old, &new))
                .cloned()
                .collect::<Vec<_>>();
            let tables = rules
                .iter()
                .filter(|rule| rule_matches(rule, &old, &new))
                .flat_map(|rule| model::scopes(&rule.table).unwrap().unwrap())
                .collect();
            let batch_tables = edits
                .iter()
                .filter(|edit| batch_matches(edit, game, version, &old, &new))
                .map(|edit| edit.table.clone())
                .collect();
            Ok(HitGroup {
                old,
                new,
                rules: bajptr_core::rules::Rules::new(without_rules)?,
                edits: without_edits,
                tables,
                batch_tables,
                count: 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    bajptr_core::database::visit_matches(
        con,
        &bajptr_core::database::default_tables(),
        "",
        false,
        control,
        |row| {
            if !groups.iter().any(|group| group.candidate(&row)) {
                return Ok(());
            }
            let (final_text, _) = model::effective(&row, edits, game, version, &compiled, skip)?;
            for group in groups.iter_mut().filter(|group| group.candidate(&row)) {
                if model::effective(&row, &group.edits, game, version, &group.rules, skip)?.0
                    != final_text
                {
                    group.count += 1;
                }
            }
            Ok(())
        },
    )?;
    Ok(serde_json::json!(
        groups
            .into_iter()
            .map(|group| serde_json::json!({"old":group.old,"new":group.new,"count":group.count}))
            .collect::<Vec<_>>()
    ))
}

pub fn rule_matches(rule: &Rule, old: &str, new: &str) -> bool {
    rule.mode == "sub"
        && rule.old == old
        && rule.new == new
        && model::scopes(&rule.table)
            .is_ok_and(|scope| scope.is_some_and(|names| !names.is_empty()))
}
pub fn batch_matches(edit: &Edit, game: &str, version: &str, old: &str, new: &str) -> bool {
    edit.game == game
        && edit.version == version
        && edit.batch
        && edit
            .batch_operation
            .as_ref()
            .is_some_and(|op| op.old == old && op.new == new)
}

pub struct Change<'a> {
    pub game: &'a str,
    pub version: &'a str,
    pub old: &'a str,
    pub new: &'a str,
    pub next_old: &'a str,
    pub next_new: &'a str,
    pub tables: BTreeSet<String>,
    pub removed: BTreeSet<usize>,
}
pub fn update(
    rules: &[Rule],
    edits: &[Edit],
    change: Change<'_>,
) -> Result<(Vec<Rule>, Vec<Edit>)> {
    let Change {
        game,
        version,
        old,
        new,
        next_old,
        next_new,
        tables,
        removed,
    } = change;
    if next_old.is_empty() {
        return Err(invalid("请填写要替换的文字。"));
    }
    let rule_indices = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule_matches(rule, old, new))
        .map(|(i, _)| i)
        .collect::<BTreeSet<_>>();
    let batch_indices = edits
        .iter()
        .enumerate()
        .filter(|(_, edit)| batch_matches(edit, game, version, old, new))
        .map(|(i, _)| i)
        .collect::<BTreeSet<_>>();
    if rule_indices.is_empty() || batch_indices.is_empty() || !removed.is_subset(&batch_indices) {
        return Err(invalid("合并替换的范围已变化，请重新打开。"));
    }
    let mut remaining = tables;
    let mut values = rules.to_vec();
    for &index in &rule_indices {
        let scope = model::scopes(&rules[index].table)?.unwrap();
        let assigned = scope
            .into_iter()
            .filter(|table| remaining.remove(table))
            .collect::<BTreeSet<_>>();
        values[index].old = next_old.into();
        values[index].new = next_new.into();
        values[index].table = serde_json::to_string(&assigned)?;
    }
    let first = *rule_indices.first().unwrap();
    let mut scope = model::scopes(&values[first].table)?
        .unwrap()
        .into_iter()
        .collect::<BTreeSet<_>>();
    scope.extend(remaining);
    values[first].table = serde_json::to_string(&scope)?;
    let values = values
        .into_iter()
        .enumerate()
        .filter(|(i, rule)| !rule_indices.contains(i) || rule.table != "[]")
        .map(|(_, rule)| rule)
        .collect::<Vec<_>>();
    let mut next = edits
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed.contains(i))
        .map(|(i, edit)| {
            let mut edit = edit.clone();
            if batch_indices.contains(&i) {
                let op = edit.batch_operation.as_mut().unwrap();
                op.old = next_old.into();
                op.new = next_new.into();
                edit.replacement = op.before.replace(next_old, next_new);
            }
            edit
        })
        .collect::<Vec<_>>();
    // Reuse an existing batch identity when editing into the same replacement.
    let id = next
        .iter()
        .find(|edit| batch_matches(edit, game, version, next_old, next_new))
        .and_then(|edit| edit.batch_operation.as_ref())
        .map(|op| op.id.clone());
    if let Some(id) = id {
        for edit in &mut next {
            if batch_matches(edit, game, version, next_old, next_new) {
                edit.batch_operation.as_mut().unwrap().id.clone_from(&id);
            }
        }
    }
    model::validate_edits(&next)?;
    Ok((model::normalize(values)?, next))
}
