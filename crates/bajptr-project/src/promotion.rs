use crate::{
    Control, Result, combined,
    model::{self, Edit},
};
use bajptr_core::{
    database,
    rules::{Rule, Rules},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Serialize)]
pub struct Item {
    pub table: String,
    pub label: String,
    pub old: String,
    pub new: String,
    pub count: usize,
    pub reason: String,
}
pub struct Plan {
    pub items: Vec<Item>,
    pub rules: Vec<Rule>,
    pub edits: Vec<Edit>,
}

fn extend_scope(rules: &[Rule], old: &str, new: &str, table: &str) -> Result<Vec<Rule>> {
    let mut rules = rules.to_vec();
    let matching = rules
        .iter()
        .position(|rule| {
            combined::rule_matches(rule, old, new)
                && model::scopes(&rule.table)
                    .unwrap()
                    .unwrap()
                    .iter()
                    .any(|name| name == table)
        })
        .or_else(|| {
            rules
                .iter()
                .position(|rule| combined::rule_matches(rule, old, new))
        });
    if let Some(index) = matching {
        let mut scope = model::scopes(&rules[index].table)?
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        scope.insert(table.into());
        rules[index].table = serde_json::to_string(&scope)?;
    } else {
        rules.push(Rule {
            old: old.into(),
            new: new.into(),
            mode: "sub".into(),
            source: "批量修改整理".into(),
            table: serde_json::to_string(&[table])?,
        });
    }
    model::normalize(rules)
}

pub fn plan(
    con: &rusqlite::Connection,
    rules: &[Rule],
    edits: &[Edit],
    game: &str,
    version: &str,
    skip: bool,
    control: &Control<'_>,
) -> Result<Plan> {
    model::validate_edits(edits)?;
    let mut groups = BTreeMap::<_, Vec<&Edit>>::new();
    for edit in edits
        .iter()
        .filter(|edit| edit.game == game && edit.version == version && edit.batch)
    {
        if let Some(op) = &edit.batch_operation {
            groups
                .entry((edit.table.clone(), op.old.clone(), op.new.clone()))
                .or_default()
                .push(edit);
        }
    }
    let tables = database::tables(con, &database::default_tables())?;
    let mut result = Plan {
        items: vec![],
        rules: rules.to_vec(),
        edits: edits.to_vec(),
    };
    for ((table, old, new), members) in groups {
        control.log(&format!(
            "核对批量范围：{} · {old} → {new}",
            model::title(&table)
        ))?;
        let mut item = Item {
            label: model::title(&table),
            table: table.clone(),
            old: old.clone(),
            new: new.clone(),
            count: members.len(),
            reason: String::new(),
        };
        if old == new || !tables.contains(&table) {
            item.reason = "替换内容未变化，或来源表已不存在。".into();
        } else if members.iter().any(|member| {
            edits
                .iter()
                .any(|edit| !edit.batch && edit.same_address(member))
        }) {
            item.reason = "包含单条修改，保留现有优先级。".into();
        }
        if !item.reason.is_empty() {
            result.items.push(item);
            continue;
        }
        let next_rules = match extend_scope(&result.rules, &old, &new, &table) {
            Ok(rules) => rules,
            Err(crate::Error::Invalid(_)) => {
                item.reason = "与现有词条范围冲突。".into();
                result.items.push(item);
                continue;
            }
            Err(error) => return Err(error),
        };
        let next_edits = result
            .edits
            .iter()
            .filter(|edit| {
                !(edit.table == table && combined::batch_matches(edit, game, version, &old, &new))
            })
            .cloned()
            .collect::<Vec<_>>();
        let before_rules = Rules::new(result.rules.clone())?;
        let after_rules = Rules::new(next_rules.clone())?;
        let scope_rule = Rules::new(vec![Rule {
            old: old.clone(),
            new: new.clone(),
            mode: "sub".into(),
            source: String::new(),
            table: table.clone(),
        }])?;
        let addresses = members
            .iter()
            .map(|edit| ((edit.row_id.clone(), edit.slot), *edit))
            .collect::<HashMap<_, _>>();
        let mut seen = 0;
        let mut coverage = true;
        let mut same = true;
        database::visit_table(con, &table, control, |row| {
            let member = addresses.get(&(row.1.clone(), row.2));
            let expected =
                model::effective(&row, &[], game, version, &scope_rule, skip)?.0 != row.3;
            coverage &= expected == member.is_some();
            if let Some(edit) = member {
                seen += 1;
                let op = edit.batch_operation.as_ref().unwrap();
                coverage &=
                    edit.original == row.3 && edit.replacement == op.before.replace(&old, &new);
            }
            if model::effective(&row, &result.edits, game, version, &before_rules, skip)?.0
                != model::effective(&row, &next_edits, game, version, &after_rules, skip)?.0
            {
                same = false;
            }
            Ok(())
        })?;
        if !coverage || seen != members.len() {
            item.reason = "未完整覆盖全部匹配文本，或记录定位无法核对。".into();
        } else if !same {
            item.reason = "转换会改变当前结果，保留原批量修改。".into();
        } else {
            result.rules = next_rules;
            result.edits = next_edits;
        }
        result.items.push(item);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BatchOperation;
    use database::RowId;
    use rusqlite::Connection;

    fn database(rows: &[(&str, i64, &str)]) -> Connection {
        let con = Connection::open_in_memory().unwrap();
        con.execute_batch("CREATE TABLE LocalizeDBSchema(Id INTEGER PRIMARY KEY,Bytes BLOB);CREATE TABLE LocalizeSkillDBSchema(Id INTEGER PRIMARY KEY,Bytes BLOB);").unwrap();
        for &(table, id, text) in rows {
            let mut blob = vec![0u8; 24];
            blob[0..4].copy_from_slice(&16u32.to_le_bytes());
            blob[4..6].copy_from_slice(&6u16.to_le_bytes());
            blob[6..8].copy_from_slice(&8u16.to_le_bytes());
            blob[8..10].copy_from_slice(&4u16.to_le_bytes());
            blob[16..20].copy_from_slice(&12i32.to_le_bytes());
            blob[20..24].copy_from_slice(&4u32.to_le_bytes());
            blob.extend_from_slice(&(text.len() as u32).to_le_bytes());
            blob.extend_from_slice(text.as_bytes());
            blob.push(0);
            con.execute(
                &format!("INSERT INTO {table} VALUES(?1,?2)"),
                rusqlite::params![id, blob],
            )
            .unwrap();
        }
        con
    }
    fn edit(table: &str, id: i64, text: &str) -> Edit {
        Edit {
            game: "game".into(),
            version: "a".repeat(64),
            table: table.into(),
            row_id: RowId::Integer(id),
            slot: 0,
            original: text.into(),
            replacement: text.replace("任务", "委托"),
            batch: true,
            batch_operation: Some(BatchOperation {
                id: "1".repeat(32),
                old: "任务".into(),
                new: "委托".into(),
                before: text.into(),
            }),
        }
    }
    fn rule(old: &str, new: &str) -> Rule {
        Rule {
            old: old.into(),
            new: new.into(),
            mode: "sub".into(),
            source: String::new(),
            table: "LocalizeDBSchema".into(),
        }
    }
    fn check(con: &Connection, rules: &[Rule], edits: &[Edit], skip: bool) -> Plan {
        plan(
            con,
            rules,
            edits,
            "game",
            &"a".repeat(64),
            skip,
            &Control::new(&|_| true),
        )
        .unwrap()
    }

    #[test]
    fn complete_table_becomes_a_rule_while_partial_scope_and_other_versions_remain() {
        let con = database(&[
            ("LocalizeDBSchema", 1, "任务一"),
            ("LocalizeDBSchema", 2, "任务二"),
            ("LocalizeSkillDBSchema", 1, "任务甲"),
            ("LocalizeSkillDBSchema", 2, "任务乙"),
        ]);
        let mut prior = edit("LocalizeDBSchema", 1, "任务一");
        prior.version = "b".repeat(64);
        prior.batch_operation.as_mut().unwrap().id = "2".repeat(32);
        let edits = vec![
            edit("LocalizeDBSchema", 1, "任务一"),
            edit("LocalizeDBSchema", 2, "任务二"),
            edit("LocalizeSkillDBSchema", 1, "任务甲"),
            prior.clone(),
        ];
        let result = check(&con, &[], &edits, true);
        assert_eq!(result.rules.len(), 1);
        assert_eq!(result.rules[0].table, "LocalizeDBSchema");
        assert_eq!(result.edits, vec![edits[2].clone(), prior]);
        assert!(result.items[0].reason.is_empty());
        assert!(!result.items[1].reason.is_empty());
    }
    #[test]
    fn equivalence_includes_existing_rules_and_rejects_precedence_changes() {
        let con = database(&[("LocalizeDBSchema", 1, "任务甲")]);
        let mut edit = edit("LocalizeDBSchema", 1, "任务甲");
        edit.batch_operation.as_mut().unwrap().before = "任务乙".into();
        edit.replacement = "委托乙".into();
        let compatible = check(&con, &[rule("甲", "乙")], &[edit.clone()], true);
        assert!(compatible.edits.is_empty());
        let conflict = check(&con, &[rule("任务甲", "任务乙")], &[edit.clone()], true);
        assert_eq!(conflict.edits, vec![edit]);
        assert!(conflict.items[0].reason.contains("改变当前结果"));
    }
    #[test]
    fn paired_single_edits_and_kana_overrides_keep_their_existing_behavior() {
        let con = database(&[("LocalizeDBSchema", 1, "任务あ")]);
        let edit = edit("LocalizeDBSchema", 1, "任务あ");
        assert_eq!(
            check(&con, &[], std::slice::from_ref(&edit), true).edits,
            vec![edit.clone()]
        );
        assert!(
            check(&con, &[], std::slice::from_ref(&edit), false)
                .edits
                .is_empty()
        );
        let mut single = edit.clone();
        single.batch = false;
        single.batch_operation = None;
        single.replacement = "单条内容".into();
        let paired = vec![edit, single];
        let result = check(&con, &[], &paired, false);
        assert_eq!(result.edits, paired);
        assert!(result.items[0].reason.contains("单条修改"));
    }
    #[test]
    fn conflicting_scope_and_unclassified_legacy_records_are_not_promoted() {
        let con = database(&[("LocalizeDBSchema", 1, "任务甲")]);
        let edit = edit("LocalizeDBSchema", 1, "任务甲");
        let result = check(
            &con,
            &[rule("任务", "其他")],
            std::slice::from_ref(&edit),
            true,
        );
        assert_eq!(result.edits, vec![edit.clone()]);
        assert!(!result.items[0].reason.is_empty());
        let mut legacy = edit;
        legacy.batch_operation = None;
        let result = check(&con, &[], &[legacy.clone()], true);
        assert!(result.items.is_empty());
        assert_eq!(result.edits, vec![legacy]);
    }
    #[test]
    fn incomplete_text_parsing_refuses_a_promotion_plan() {
        let con = database(&[("LocalizeDBSchema", 1, "任务甲")]);
        con.execute_batch("INSERT INTO LocalizeDBSchema VALUES(2,X'010203');")
            .unwrap();
        assert!(
            plan(
                &con,
                &[],
                &[edit("LocalizeDBSchema", 1, "任务甲")],
                "game",
                &"a".repeat(64),
                true,
                &Control::new(&|_| true)
            )
            .is_err()
        );
    }
}
