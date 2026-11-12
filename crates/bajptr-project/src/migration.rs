//! Match saved overrides by original text, keeping uncertain locations explicit.
use crate::{
    Result, invalid,
    model::{self, Edit},
};
use bajptr_core::database::{LocatedText, RowId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    #[serde(default)]
    pub group_key: String,
    pub indices: Vec<usize>,
    pub table: String,
    pub original: String,
    pub replacement: String,
    pub kind: String,
    pub candidates: Vec<LocatedText>,
    pub suggested: Option<usize>,
}

pub fn plan(edits: &[Edit], game: &str, version: &str, located: &[LocatedText]) -> Vec<Item> {
    let occupied: HashSet<_> = edits
        .iter()
        .filter(|e| e.game == game && e.version == version)
        .map(|e| (&e.table, &e.row_id, e.slot))
        .collect();
    let mut by_original = HashMap::<(&str, &str), Vec<&LocatedText>>::new();
    for candidate in located {
        if !occupied.contains(&(&candidate.row.0, &candidate.row.1, candidate.row.2)) {
            by_original
                .entry((&candidate.row.0, &candidate.row.3))
                .or_default()
                .push(candidate);
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut positions = HashMap::new();
    for (index, edit) in edits
        .iter()
        .enumerate()
        .filter(|(_, e)| e.game == game && e.version != version)
    {
        let key = (&edit.version, &edit.table, &edit.row_id, edit.slot);
        let position = *positions.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[position].push(index);
    }
    let mut items: Vec<Item> = groups
        .into_iter()
        .map(|indices| {
            let edit = indices
                .iter()
                .map(|&i| &edits[i])
                .max_by_key(|e| e.batch)
                .unwrap();
            let candidates: Vec<_> = by_original
                .get(&(edit.table.as_str(), edit.original.as_str()))
                .into_iter()
                .flatten()
                .map(|c| (*c).clone())
                .collect();
            let exact = candidates
                .iter()
                .position(|c| c.row.1 == edit.row_id && c.row.2 == edit.slot);
            let kind = if exact.is_some() {
                "exact"
            } else if candidates.len() == 1 {
                "relocated"
            } else if candidates.is_empty() {
                "unavailable"
            } else {
                "ambiguous"
            };
            Item {
                // Only interchangeable saved layers may share a review decision.
                // Include the source version so stale revisions are never combined.
                group_key: crate::storage::digest(
                    serde_json::to_string(
                        &indices
                            .iter()
                            .map(|&i| {
                                let e = &edits[i];
                                serde_json::json!([
                                    e.game,
                                    e.version,
                                    e.table,
                                    e.original,
                                    e.replacement,
                                    e.batch,
                                    e.batch_operation
                                        .as_ref()
                                        .map(|op| serde_json::json!([op.old, op.new, op.before]))
                                ])
                            })
                            .collect::<Vec<_>>(),
                    )
                    .unwrap()
                    .as_bytes(),
                ),
                indices,
                table: edit.table.clone(),
                original: edit.original.clone(),
                replacement: edit.replacement.clone(),
                kind: kind.into(),
                candidates,
                suggested: exact,
            }
        })
        .collect();
    let mut targets = HashMap::new();
    for item in &items {
        if let Some(index) = item.suggested {
            let row = &item.candidates[index].row;
            *targets.entry((&row.0, &row.1, row.2)).or_insert(0) += 1;
        }
    }
    let conflicts: Vec<_> = items
        .iter()
        .map(|item| {
            item.suggested.is_some_and(|index| {
                let row = &item.candidates[index].row;
                targets[&(&row.0, &row.1, row.2)] > 1
            })
        })
        .collect();
    for (item, conflict) in items.iter_mut().zip(conflicts) {
        if conflict {
            item.kind = "ambiguous".into();
            item.suggested = None;
        }
    }
    items
}

#[derive(Serialize, Deserialize)]
pub struct Choice {
    pub item: usize,
    pub candidate: usize,
}

/// Exact addresses take precedence. Remaining interchangeable records can move
/// as a complete set only when the set has no competing saved modification.
pub fn automatic_choices(items: &[Item]) -> Vec<Choice> {
    type Target = (String, RowId, usize);
    let target = |candidate: &LocatedText| {
        (
            candidate.row.0.clone(),
            candidate.row.1.clone(),
            candidate.row.2,
        )
    };
    let mut choices: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(item, value)| value.suggested.map(|candidate| Choice { item, candidate }))
        .collect();
    let reserved: HashSet<_> = choices
        .iter()
        .map(|c| target(&items[c.item].candidates[c.candidate]))
        .collect();
    let mut groups = HashMap::<(&str, Vec<Target>), Vec<usize>>::new();
    for (index, item) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.suggested.is_none())
    {
        let candidates: Vec<_> = item
            .candidates
            .iter()
            .map(&target)
            .filter(|t| !reserved.contains(t))
            .collect();
        if !candidates.is_empty() {
            groups
                .entry((&item.group_key, candidates))
                .or_default()
                .push(index);
        }
    }
    let mut uses = HashMap::<&Target, usize>::new();
    for (_, targets) in groups.keys() {
        for t in targets {
            *uses.entry(t).or_default() += 1;
        }
    }
    for ((_, targets), members) in &groups {
        if targets.len() != members.len() || targets.iter().any(|t| uses[t] > 1) {
            continue;
        }
        for (&item, t) in members.iter().zip(targets) {
            let candidate = items[item]
                .candidates
                .iter()
                .position(|c| target(c) == *t)
                .unwrap();
            choices.push(Choice { item, candidate });
        }
    }
    choices.sort_by_key(|c| c.item);
    choices
}

pub fn apply(
    edits: &[Edit],
    items: &[Item],
    choices: &[Choice],
    game: &str,
    version: &str,
) -> Result<Vec<Edit>> {
    if choices.is_empty() {
        return Err(invalid("请先选择要迁移的文本。"));
    }
    let mut removed = HashSet::new();
    let mut targets: HashSet<(String, RowId, usize)> = HashSet::new();
    let mut moved = Vec::new();
    let mut batches = HashMap::new();
    for edit in edits
        .iter()
        .filter(|e| e.game == game && e.version == version)
    {
        if let Some(op) = &edit.batch_operation {
            batches.insert((op.old.clone(), op.new.clone()), op.id.clone());
        }
    }
    for choice in choices {
        let item = items
            .get(choice.item)
            .ok_or_else(|| invalid("迁移条目无效，请重新核对。"))?;
        let candidate = item
            .candidates
            .get(choice.candidate)
            .ok_or_else(|| invalid("请选择对应的新文本。"))?;
        if !targets.insert((
            candidate.row.0.clone(),
            candidate.row.1.clone(),
            candidate.row.2,
        )) {
            return Err(invalid("多条旧记录指向同一段新文本，请只保留一项。"));
        }
        if edits.iter().any(|e| {
            e.game == game
                && e.version == version
                && e.table == candidate.row.0
                && e.row_id == candidate.row.1
                && e.slot == candidate.row.2
        }) {
            return Err(invalid("目标文本已有新版本修改，未覆盖。"));
        }
        for &index in &item.indices {
            let mut edit = edits
                .get(index)
                .cloned()
                .ok_or_else(|| invalid("旧记录已变化，请重新核对。"))?;
            if !removed.insert(index)
                || edit.game != game
                || edit.version == version
                || edit.table != candidate.row.0
                || edit.original != candidate.row.3
            {
                return Err(invalid("迁移范围已变化，请重新核对。"));
            }
            edit.version = version.into();
            edit.row_id = candidate.row.1.clone();
            edit.slot = candidate.row.2;
            if let Some(op) = &mut edit.batch_operation {
                op.id = batches
                    .entry((op.old.clone(), op.new.clone()))
                    .or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
                    .clone();
            }
            moved.push(edit);
        }
    }
    let mut values: Vec<_> = edits
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed.contains(i))
        .map(|(_, e)| e.clone())
        .collect();
    values.extend(moved);
    model::validate_edits(&values)?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BatchOperation;
    fn edit(id: i64, original: &str) -> Edit {
        Edit {
            game: "game".into(),
            version: "a".repeat(64),
            table: "Text".into(),
            row_id: RowId::Integer(id),
            slot: 0,
            original: original.into(),
            replacement: format!("{original}改"),
            batch: false,
            batch_operation: None,
        }
    }
    fn row(id: i64, original: &str) -> LocatedText {
        LocatedText {
            row: ("Text".into(), RowId::Integer(id), 0, original.into()),
            context: String::new(),
        }
    }

    #[test]
    fn exact_matches_are_selected_while_relocations_duplicates_and_conflicts_need_review() {
        let mut current = edit(5, "已有");
        current.version = "b".repeat(64);
        let edits = vec![
            edit(1, "一致"),
            edit(2, "移位"),
            edit(3, "重复"),
            edit(4, "已变"),
            edit(5, "已有"),
            current,
        ];
        let items = plan(
            &edits,
            "game",
            &"b".repeat(64),
            &[
                row(1, "一致"),
                row(20, "移位"),
                row(30, "重复"),
                row(31, "重复"),
                row(4, "新文"),
                row(5, "已有"),
            ],
        );
        assert_eq!(
            items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(),
            [
                "exact",
                "relocated",
                "ambiguous",
                "unavailable",
                "unavailable"
            ]
        );
        assert_eq!(items.iter().filter(|i| i.suggested.is_some()).count(), 1);
        let updated = apply(
            &edits,
            &items,
            &[
                Choice {
                    item: 0,
                    candidate: 0,
                },
                Choice {
                    item: 1,
                    candidate: 0,
                },
            ],
            "game",
            &"b".repeat(64),
        )
        .unwrap();
        assert_eq!(updated.len(), edits.len());
        assert!(
            updated
                .iter()
                .any(|e| e.version == "b".repeat(64) && e.row_id == RowId::Integer(20))
        );
        assert_eq!(
            updated
                .iter()
                .find(|e| e.original == "已有" && e.version == "b".repeat(64))
                .unwrap(),
            &edits[5]
        );
        let mut older = edit(1, "一致");
        older.version = "c".repeat(64);
        older.replacement = "另一版结果".into();
        let conflicts = plan(
            &[edits[0].clone(), older],
            "game",
            &"b".repeat(64),
            &[row(1, "一致")],
        );
        assert!(
            conflicts
                .iter()
                .all(|item| item.suggested.is_none() && item.kind == "ambiguous")
        );
    }

    #[test]
    fn complete_equivalent_groups_move_automatically_but_changed_scope_and_conflicts_do_not() {
        let edits: Vec<_> = (1..=7).map(|id| edit(id, "原文")).collect();
        let moved: Vec<_> = (11..=17).map(|id| row(id, "原文")).collect();
        let items = plan(&edits, "game", &"b".repeat(64), &moved);
        let choices = automatic_choices(&items);
        assert_eq!(choices.len(), 7);
        let updated = apply(&edits, &items, &choices, "game", &"b".repeat(64)).unwrap();
        assert!(
            updated
                .iter()
                .all(|e| e.version == "b".repeat(64) && e.replacement == "原文改")
        );
        assert_eq!(
            updated
                .iter()
                .map(|e| &e.row_id)
                .collect::<HashSet<_>>()
                .len(),
            7
        );
        let mixed: Vec<_> = [row(1, "原文"), row(2, "原文")]
            .into_iter()
            .chain(moved[2..].iter().cloned())
            .collect();
        let mixed_plan = plan(&edits, "game", &"b".repeat(64), &mixed);
        let mixed_choices = automatic_choices(&mixed_plan);
        assert_eq!(mixed_choices.len(), 7);
        assert!(apply(&edits, &mixed_plan, &mixed_choices, "game", &"b".repeat(64)).is_ok());
        for rows in [
            moved[..6].to_vec(),
            moved.iter().cloned().chain([row(18, "原文")]).collect(),
        ] {
            assert!(automatic_choices(&plan(&edits, "game", &"b".repeat(64), &rows)).is_empty());
        }
        let mut conflict = edits.clone();
        conflict[6].replacement = "不同修改".into();
        assert!(automatic_choices(&plan(&conflict, "game", &"b".repeat(64), &moved)).is_empty());
        conflict[6] = edits[6].clone();
        conflict[6].version = "c".repeat(64);
        assert!(automatic_choices(&plan(&conflict, "game", &"b".repeat(64), &moved)).is_empty());
    }

    #[test]
    fn paired_single_and_partial_batch_migrate_together_without_losing_old_members() {
        let single = edit(1, "原文");
        let mut batch = single.clone();
        batch.batch = true;
        batch.replacement = "新文改".into();
        batch.batch_operation = Some(BatchOperation {
            id: "1".repeat(32),
            old: "原文".into(),
            new: "新文".into(),
            before: single.replacement.clone(),
        });
        let mut remaining = batch.clone();
        remaining.row_id = RowId::Integer(2);
        let edits = vec![single, batch, remaining];
        let items = plan(
            &edits,
            "game",
            &"b".repeat(64),
            &[row(1, "原文"), row(2, "原文")],
        );
        assert_ne!(
            items[0].group_key, items[1].group_key,
            "single+batch and batch-only must stay separate"
        );
        let same = plan(
            &[edits[1].clone(), edits[2].clone()],
            "game",
            &"b".repeat(64),
            &[row(1, "原文"), row(2, "原文")],
        );
        assert_eq!(same[0].group_key, same[1].group_key);
        let updated = apply(
            &edits,
            &items,
            &[Choice {
                item: 0,
                candidate: 0,
            }],
            "game",
            &"b".repeat(64),
        )
        .unwrap();
        assert_eq!(updated.len(), 3);
        model::validate_edits(&updated).unwrap();
        assert_eq!(
            updated
                .iter()
                .filter(|e| e.version == "b".repeat(64))
                .count(),
            2
        );
        let old = updated
            .iter()
            .find(|e| e.version == "a".repeat(64))
            .unwrap();
        let new = updated
            .iter()
            .find(|e| e.version == "b".repeat(64) && e.batch)
            .unwrap();
        assert_ne!(
            old.batch_operation.as_ref().unwrap().id,
            new.batch_operation.as_ref().unwrap().id
        );
        assert_eq!(new.batch_operation.as_ref().unwrap().before, "原文改");
        assert_eq!(
            model::final_edits(&updated, "game", &"b".repeat(64)).unwrap()[0].replacement,
            "新文改"
        );
        let followup = plan(
            &updated,
            "game",
            &"b".repeat(64),
            &[row(1, "原文"), row(2, "原文")],
        );
        let merged = apply(
            &updated,
            &followup,
            &[Choice {
                item: 0,
                candidate: 0,
            }],
            "game",
            &"b".repeat(64),
        )
        .unwrap();
        assert!(merged.iter().all(|e| e.version == "b".repeat(64)));
        assert_eq!(
            merged
                .iter()
                .filter_map(|e| e.batch_operation.as_ref().map(|op| &op.id))
                .collect::<HashSet<_>>()
                .len(),
            1
        );
        let duplicate = vec![
            Choice {
                item: 0,
                candidate: 0,
            },
            Choice {
                item: 1,
                candidate: 0,
            },
        ];
        assert!(apply(&edits, &items, &duplicate, "game", &"b".repeat(64)).is_err());
    }
}
