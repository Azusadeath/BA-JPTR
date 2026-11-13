use bajptr_project::{Control, desktop::Project, region, storage, worker::Worker};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn control() -> Control<'static> {
    Control::new(&|m| {
        if let Some(m) = m {
            eprintln!("{m}");
        }
        true
    })
}
struct Fixture {
    root: PathBuf,
    game: PathBuf,
    data: PathBuf,
    before: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bajptr-rust-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let game = root.join("日服游戏");
        let data = root.join("工具");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1");
        let catalog = game.join(region::CATALOG);
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        let before = fs::read(source.join("TableCatalog.bytes")).unwrap();
        let entry = region::parse(&before).unwrap();
        fs::write(&catalog, &before).unwrap();
        fs::copy(
            source.join("TableCatalog.hash"),
            catalog.with_file_name("TableCatalog.hash"),
        )
        .unwrap();
        fs::copy(
            source.join("encrypted.db"),
            game.join(region::BUNDLES)
                .join(format!("123456_{}", entry.crc)),
        )
        .unwrap();
        fs::create_dir_all(data.join(".gui")).unwrap();
        fs::copy(source.join("rules.tsv"), data.join(".gui/rules.tsv")).unwrap();
        fs::write(
            data.join(".gui/settings.json"),
            serde_json::to_vec(&json!({"format":2,"game":game,"skip_kana":true})).unwrap(),
        )
        .unwrap();
        Self {
            root,
            game,
            data,
            before,
        }
    }
    fn project(&self) -> Project {
        let mut p = Project::new(self.data.clone());
        p.dispatch("load", &json!({}), &control()).unwrap();
        p
    }
    fn ready_project(&self) -> Project {
        let mut p = self.project();
        if !self.data.join("backup/_baselines").exists() {
            let status = call(&mut p, "check", json!({}));
            call(
                &mut p,
                "baseline_commit",
                json!({"token":status["baseline_status"]["token"]}),
            );
        }
        p
    }
    fn unchanged(&self) {
        assert_eq!(
            fs::read(self.game.join(region::CATALOG)).unwrap(),
            self.before
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(e) = storage::retire(&self.root) {
            eprintln!("{e}");
        }
    }
}
#[track_caller]
fn call(p: &mut Project, action: &str, payload: Value) -> Value {
    p.dispatch(action, &payload, &control())
        .unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/v1/oracle.json")).unwrap()
}

fn combined_fixture() -> (Fixture, Project) {
    let mut f = Fixture::new();
    add_new_source(&mut f, false);
    fs::write(
        f.data.join(".gui/rules.tsv"),
        "任务\t委托\tsub\t保留备注\tLocalizeSkillDBSchema\n",
    )
    .unwrap();
    let mut p = f.ready_project();
    let search = call(
        &mut p,
        "search",
        json!({"keyword":"任务","source":"LocalizeDBSchema"}),
    );
    let draft = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"任务","new":"委托"}),
    );
    call(&mut p, "batch_save", json!({"nonce":draft["nonce"]}));
    (f, p)
}

#[test]
fn workspace_backup_restores_rules_text_edits_paths_options_and_source_metadata() {
    let (f, mut p) = combined_fixture();
    call(&mut p, "sources", json!({}));
    let revision = p.revision;
    call(
        &mut p,
        "settings",
        json!({"revision":revision,"game":f.game,"skip_kana":false}),
    );
    let rules = p.rules.clone();
    let edits = p.edits.clone();
    let source = fs::read(f.data.join(".gui/source-history.json")).unwrap();
    let templates=br#"{"format":2,"drafts":{"saved-form":{"key":"number","values":["level","rank"],"sample":"level12"}}}"#;
    fs::write(f.data.join(".gui/template-drafts.json"), templates).unwrap();
    let path = f.root.join("我的工作区备份.json");
    call(&mut p, "backup_create", json!({"path":path}));
    let revision = p.revision;
    call(
        &mut p,
        "combined_remove",
        json!({"revision":revision,"old":"任务","new":"委托"}),
    );
    let revision = p.revision;
    call(
        &mut p,
        "settings",
        json!({"revision":revision,"game":f.root.join("其他位置"),"skip_kana":true}),
    );
    let previous_settings = fs::read(f.data.join(".gui/settings.json")).unwrap();
    fs::write(
        f.data.join(".gui/template-drafts.json"),
        br#"{"format":2,"drafts":{}}"#,
    )
    .unwrap();
    let preview = call(&mut p, "backup_read", json!({"path":path}));
    assert_eq!(preview["rule_count"], rules.len());
    assert_eq!(preview["edit_count"], edits.len());
    assert_eq!(preview["game"], f.game.to_str().unwrap());
    assert_eq!(preview["skip_kana"], false);
    assert!(p.rules.is_empty());
    let saved = call(&mut p, "backup_restore", json!({"nonce":preview["nonce"]}));
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    assert_eq!(p.game, f.game.to_str().unwrap());
    assert!(!p.skip);
    assert_eq!(
        fs::read(f.data.join(".gui/source-history.json")).unwrap(),
        source
    );
    assert_eq!(
        fs::read(f.data.join(".gui/template-drafts.json")).unwrap(),
        templates
    );
    let recovery = Path::new(saved["recovery"].as_str().unwrap());
    assert_eq!(
        fs::read(recovery.join("settings.json")).unwrap(),
        previous_settings
    );
    let reopened = f.project();
    assert_eq!(reopened.rules, rules);
    assert_eq!(reopened.edits, edits);
    assert_eq!(reopened.game, p.game);
    assert!(!reopened.skip);
    f.unchanged();
}

#[cfg(windows)]
#[test]
fn workspace_backup_restore_rolls_back_all_files_when_settings_publication_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    let (f, mut p) = combined_fixture();
    let path = f.root.join("完整备份.json");
    call(&mut p, "backup_create", json!({"path":path}));
    let revision = p.revision;
    call(
        &mut p,
        "combined_remove",
        json!({"revision":revision,"old":"任务","new":"委托"}),
    );
    let names = [
        "rules.tsv",
        "text-edits.json",
        "settings.json",
        "source-history.json",
        "template-drafts.json",
    ];
    let before = names.map(|name| fs::read(f.data.join(".gui").join(name)).ok());
    let preview = call(&mut p, "backup_read", json!({"path":path}));
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(f.data.join(".gui/settings.json"))
        .unwrap();
    assert!(
        p.dispatch(
            "backup_restore",
            &json!({"nonce":preview["nonce"]}),
            &control()
        )
        .is_err()
    );
    assert_eq!(
        names.map(|name| fs::read(f.data.join(".gui").join(name)).ok()),
        before
    );
    assert!(p.rules.is_empty());
    assert!(p.edits.is_empty());
    assert!(!f.data.join(".gui/workspace-transaction.json").exists());
    drop(held);
    call(&mut p, "backup_restore", json!({"nonce":preview["nonce"]}));
    assert_eq!(p.rules.len(), 1);
    assert_eq!(p.edits.len(), 1);
    f.unchanged();
}

#[test]
fn workspace_backup_rejects_invalid_cancelled_and_stale_restore_requests() {
    let (f, mut p) = combined_fixture();
    let path = f.root.join("备份.json");
    call(&mut p, "backup_create", json!({"path":path}));
    let bytes = fs::read(&path).unwrap();
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    for bad in [
        json!({"format":2}),
        {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["files"]["settings.json"] = json!("{broken");
            value
        },
        {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["files"]["../settings.json"] = json!("{}");
            value
        },
    ] {
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(
            p.dispatch("backup_read", &json!({"path":path}), &control())
                .is_err()
        );
    }
    fs::write(&path, bytes).unwrap();
    let preview = call(&mut p, "backup_read", json!({"path":path}));
    assert!(
        p.dispatch(
            "backup_restore",
            &json!({"nonce":preview["nonce"]}),
            &Control::new(&|_| false)
        )
        .is_err()
    );
    assert!(!f.data.join(".gui/recovery").exists());
    fs::write(
        f.data.join(".gui/template-drafts.json"),
        br#"{"format":2,"drafts":{}}"#,
    )
    .unwrap();
    assert!(
        p.dispatch(
            "backup_restore",
            &json!({"nonce":preview["nonce"]}),
            &control()
        )
        .is_err()
    );
    assert!(!f.data.join(".gui/recovery").exists());
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    f.unchanged();
}

#[test]
fn workspace_backup_can_restore_damaged_records_and_empty_workspaces_can_be_backed_up() {
    let (f, mut p) = combined_fixture();
    let path = f.root.join("有效备份.json");
    call(&mut p, "backup_create", json!({"path":path}));
    let expected = p.rules.clone();
    fs::write(f.data.join(".gui/rules.tsv"), [255, 0, 1]).unwrap();
    let mut broken = f.project();
    let preview = call(&mut broken, "backup_read", json!({"path":path}));
    call(
        &mut broken,
        "backup_restore",
        json!({"nonce":preview["nonce"]}),
    );
    assert_eq!(broken.rules, expected);
    assert_eq!(call(&mut broken, "load", json!({}))["errors"], json!({}));
    let empty = f.root.join("空工作区");
    let mut clean = Project::new(empty.clone());
    call(&mut clean, "load", json!({}));
    let path = f.root.join("空备份.json");
    call(&mut clean, "backup_create", json!({"path":path}));
    let preview = call(&mut clean, "backup_read", json!({"path":path}));
    assert_eq!(preview["rule_count"], 0);
    assert_eq!(preview["edit_count"], 0);
    assert_eq!(preview["skip_kana"], true);
    assert!(
        clean
            .dispatch(
                "backup_create",
                &json!({"path":empty.join(".gui/settings.json")}),
                &control()
            )
            .is_err()
    );
    f.unchanged();
}

#[test]
fn workspace_backup_interrupted_restore_recovers_all_five_documents_on_restart() {
    let (f, p) = combined_fixture();
    let root = f.data.join(".gui");
    let mut writes = vec![];
    for name in [
        "rules.tsv",
        "text-edits.json",
        "settings.json",
        "source-history.json",
        "template-drafts.json",
    ] {
        let before = fs::read(root.join(name)).ok();
        let after = match name {
            "rules.tsv" => b"".to_vec(),
            "text-edits.json" => bajptr_project::model::edits_bytes(&[]).unwrap(),
            "settings.json" => br#"{"format":2,"game":"elsewhere","skip_kana":false}"#.to_vec(),
            "source-history.json" => br#"{"format":1,"games":{}}"#.to_vec(),
            _ => br#"{"format":2,"drafts":{}}"#.to_vec(),
        };
        if writes.len() < 3 {
            fs::write(root.join(name), &after).unwrap();
        }
        writes.push(json!({"name":name,"before":before,"after":after}));
    }
    fs::write(
        root.join("workspace-transaction.json"),
        serde_json::to_vec(&writes).unwrap(),
    )
    .unwrap();
    let reopened = f.project();
    assert_eq!(reopened.rules, p.rules);
    assert_eq!(reopened.edits, p.edits);
    assert_eq!(reopened.game, p.game);
    assert_eq!(reopened.skip, p.skip);
    assert!(!root.join("workspace-transaction.json").exists());
    f.unchanged();
}

#[test]
fn batch_promotion_preserves_results_backs_up_both_documents_and_undoes_together() {
    let (f, mut p) = combined_fixture();
    let rules = p.rules.clone();
    let edits = p.edits.clone();
    let rule_bytes = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    let edit_bytes = fs::read(f.data.join(".gui/text-edits.json")).unwrap();
    let before = call(&mut p, "search", json!({"keyword":"任务"}));
    let plan = call(&mut p, "promotion_preview", json!({}));
    assert_eq!(plan["count"], 1);
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    let saved = call(&mut p, "promotion_start", json!({}));
    assert!(p.edits.is_empty());
    assert_eq!(p.rules.len(), 1);
    assert_eq!(p.rules[0].source, "保留备注");
    assert_eq!(
        bajptr_project::model::scopes(&p.rules[0].table)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    let after = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(
        before["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| &row["text"])
            .collect::<Vec<_>>(),
        after["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| &row["text"])
            .collect::<Vec<_>>()
    );
    let backup = Path::new(saved["backup"].as_str().unwrap());
    assert_eq!(fs::read(backup.join("rules.tsv")).unwrap(), rule_bytes);
    assert_eq!(
        fs::read(backup.join("text-edits.json")).unwrap(),
        edit_bytes
    );
    let revision = p.revision;
    let repeated = call(&mut p, "promotion_start", json!({}));
    assert_eq!(repeated["count"], 0);
    assert!(repeated["backup"].is_null());
    assert_eq!(p.revision, revision);
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    call(&mut p, "history", json!({"undo":false}));
    let reopened = f.project();
    assert_eq!(reopened.rules, p.rules);
    assert_eq!(reopened.edits, p.edits);
    f.unchanged();
}

#[cfg(windows)]
#[test]
fn batch_promotion_rejects_cancelled_stale_and_failed_saves_without_losing_records() {
    use std::os::windows::fs::OpenOptionsExt;
    let (f, mut p) = combined_fixture();
    let rules = p.rules.clone();
    let edits = p.edits.clone();
    let plan = call(&mut p, "promotion_preview", json!({}));
    let payload = json!({"nonce":plan["nonce"]});
    assert!(
        p.dispatch("promotion_commit", &payload, &Control::new(&|_| false))
            .is_err()
    );
    assert!(!f.data.join(".gui/recovery").exists());
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(f.data.join(".gui/text-edits.json"))
        .unwrap();
    assert!(
        p.dispatch("promotion_commit", &payload, &control())
            .is_err()
    );
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    assert_eq!(
        bajptr_project::model::parse_rules(&fs::read(f.data.join(".gui/rules.tsv")).unwrap())
            .unwrap(),
        rules
    );
    assert_eq!(
        bajptr_project::model::parse_edits(&fs::read(f.data.join(".gui/text-edits.json")).unwrap())
            .unwrap(),
        edits
    );
    drop(held);
    let revision = p.revision;
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"任务","new":"别的","mode":"sub","source":"备注","table":"LocalizeSkillDBSchema"}}),
    );
    assert!(
        p.dispatch("promotion_commit", &payload, &control())
            .is_err()
    );
    call(&mut p, "history", json!({"undo":true}));
    let plan = call(&mut p, "promotion_preview", json!({}));
    call(&mut p, "promotion_commit", json!({"nonce":plan["nonce"]}));
    assert!(p.edits.is_empty());
    f.unchanged();
}

#[test]
fn combined_replacement_saves_both_scopes_and_undoes_as_one_operation() {
    let (f, mut p) = combined_fixture();
    let original_rules = p.rules.clone();
    let original_edits = p.edits.clone();
    let revision = p.revision;
    let opened = call(
        &mut p,
        "combined_open",
        json!({"old":"任务","new":"委托","revision":revision}),
    );
    assert_eq!(opened["tables"], json!(["LocalizeSkillDBSchema"]));
    assert_eq!(opened["members"].as_array().unwrap().len(), 1);
    call(
        &mut p,
        "combined_save",
        json!({"nonce":opened["nonce"],"old":"任务","new":"差事","tables":opened["tables"],"removed":[]}),
    );
    assert_eq!(p.revision, revision + 1);
    assert_eq!(p.rules[0].new, "差事");
    assert_eq!(p.rules[0].source, "保留备注");
    assert_eq!(p.edits[0].batch_operation.as_ref().unwrap().new, "差事");
    assert_eq!(
        p.edits[0].batch_operation.as_ref().unwrap().before,
        original_edits[0].batch_operation.as_ref().unwrap().before
    );
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert!(
        search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["text"].as_str().unwrap().contains("差事"))
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(preview["stats"]["combined_hits"][0]["count"], 2);
    let saved_rules = p.rules.clone();
    let saved_edits = p.edits.clone();
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.rules, original_rules);
    assert_eq!(p.edits, original_edits);
    call(&mut p, "history", json!({"undo":false}));
    assert_eq!(p.rules, saved_rules);
    assert_eq!(p.edits, saved_edits);
    let reopened = f.project();
    assert_eq!(reopened.rules, saved_rules);
    assert_eq!(reopened.edits, saved_edits);
    f.unchanged();
}

#[test]
fn combined_scopes_overlap_without_counting_a_text_twice_or_replaying_replacement() {
    let (f, mut p) = combined_fixture();
    let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
    call(
        &mut p,
        "combined_save",
        json!({"nonce":opened["nonce"],"old":"任务","new":"任务任务","tables":["LocalizeDBSchema","LocalizeSkillDBSchema"],"removed":[]}),
    );
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    for row in search["rows"].as_array().unwrap() {
        assert_eq!(
            row["text"].as_str().unwrap(),
            row["original"]
                .as_str()
                .unwrap()
                .replace("任务", "任务任务")
        );
    }
    assert_eq!(
        call(&mut p, "preview", json!({}))["stats"]["combined_hits"][0]["count"],
        2
    );
    f.unchanged();
}

#[test]
fn combined_range_removal_and_whole_group_removal_keep_single_and_old_version_records() {
    let (f, mut p) = combined_fixture();
    let search = call(
        &mut p,
        "search",
        json!({"keyword":"任务","source":"LocalizeDBSchema"}),
    );
    let single = call(
        &mut p,
        "text_open",
        json!({"search_nonce":search["nonce"],"index":0}),
    );
    call(
        &mut p,
        "text_save",
        json!({"nonce":single["nonce"],"text":"任务【单条】"}),
    );
    let mut prior = p.edits[0].clone();
    prior.version = "a".repeat(64);
    prior.batch_operation.as_mut().unwrap().id = "b".repeat(32);
    p.edits.push(prior.clone());
    fs::write(
        f.data.join(".gui/text-edits.json"),
        bajptr_project::model::edits_bytes(&p.edits).unwrap(),
    )
    .unwrap();
    let mut p = f.ready_project();
    let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
    let index = opened["members"][0]["index"].as_u64().unwrap();
    call(
        &mut p,
        "combined_save",
        json!({"nonce":opened["nonce"],"old":"任务","new":"新委托","tables":opened["tables"],"removed":[index]}),
    );
    assert_eq!(p.edits.len(), 2);
    assert!(p.edits.contains(&prior));
    assert!(
        p.edits
            .iter()
            .any(|edit| !edit.batch && edit.replacement == "任务【单条】")
    );
    call(&mut p, "history", json!({"undo":true}));
    let revision = p.revision;
    call(
        &mut p,
        "combined_remove",
        json!({"old":"任务","new":"委托","revision":revision}),
    );
    assert!(p.rules.is_empty());
    assert_eq!(p.edits.len(), 2);
    assert!(p.edits.contains(&prior));
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.rules.len(), 1);
    assert_eq!(p.edits.len(), 3);
    f.unchanged();
}

#[test]
fn combined_save_rejects_stale_cancelled_and_invalid_contexts_without_writing() {
    let (f, mut p) = combined_fixture();
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    let edits = fs::read(f.data.join(".gui/text-edits.json")).unwrap();
    let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
    let payload = json!({"nonce":opened["nonce"],"old":"任务","new":"差事","tables":opened["tables"],"removed":[]});
    assert!(
        p.dispatch("combined_save", &payload, &Control::new(&|_| false))
            .is_err()
    );
    let mut invalid = payload.clone();
    invalid["removed"] = json!([999]);
    assert!(p.dispatch("combined_save", &invalid, &control()).is_err());
    invalid = payload.clone();
    invalid["tables"] = json!(["missing"]);
    assert!(p.dispatch("combined_save", &invalid, &control()).is_err());
    let revision = p.revision;
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"任务","new":"其他","mode":"sub","source":"备注","table":"LocalizeSkillDBSchema"}}),
    );
    assert!(p.dispatch("combined_save", &payload, &control()).is_err());
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    assert_eq!(
        fs::read(f.data.join(".gui/text-edits.json")).unwrap(),
        edits
    );
    f.unchanged();
}

#[cfg(windows)]
#[test]
fn combined_save_rolls_back_both_files_when_the_edit_document_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    let (f, mut p) = combined_fixture();
    let rules = p.rules.clone();
    let edits = p.edits.clone();
    let revision = p.revision;
    let rules_bytes = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    let edit_bytes = fs::read(f.data.join(".gui/text-edits.json")).unwrap();
    let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(f.data.join(".gui/text-edits.json"))
        .unwrap();
    let payload = json!({"nonce":opened["nonce"],"old":"任务","new":"差事","tables":opened["tables"],"removed":[]});
    assert!(p.dispatch("combined_save", &payload, &control()).is_err());
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    assert_eq!(p.revision, revision);
    assert_eq!(
        fs::read(f.data.join(".gui/rules.tsv")).unwrap(),
        rules_bytes
    );
    assert_eq!(
        fs::read(f.data.join(".gui/text-edits.json")).unwrap(),
        edit_bytes
    );
    assert!(!f.data.join(".gui/replacements-transaction.json").exists());
    drop(held);
    call(&mut p, "combined_save", payload);
    let saved_rules = p.rules.clone();
    let saved_edits = p.edits.clone();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(f.data.join(".gui/text-edits.json"))
        .unwrap();
    assert!(
        p.dispatch("history", &json!({"undo":true}), &control())
            .is_err()
    );
    assert_eq!(p.rules, saved_rules);
    assert_eq!(p.edits, saved_edits);
    drop(held);
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.rules, rules);
    assert_eq!(p.edits, edits);
    f.unchanged();
}

#[test]
fn interrupted_combined_save_recovers_both_documents_on_restart() {
    let (f, p) = combined_fixture();
    let root = f.data.join(".gui");
    let rules = fs::read(root.join("rules.tsv")).unwrap();
    let edits = fs::read(root.join("text-edits.json")).unwrap();
    let mut changed_rules = p.rules.clone();
    changed_rules[0].new = "中断后的内容".into();
    let after = bajptr_project::model::rules_bytes(&changed_rules).unwrap();
    fs::write(root.join("rules.tsv"), &after).unwrap();
    fs::write(root.join("replacements-transaction.json"),serde_json::to_vec(&json!([
        {"name":"rules.tsv","before":rules,"after":after},
        {"name":"text-edits.json","before":edits,"after":bajptr_project::model::edits_bytes(&[]).unwrap()}
    ])).unwrap()).unwrap();
    let reopened = f.project();
    assert_eq!(reopened.rules, p.rules);
    assert_eq!(reopened.edits, p.edits);
    assert_eq!(fs::read(root.join("rules.tsv")).unwrap(), rules);
    assert_eq!(fs::read(root.join("text-edits.json")).unwrap(), edits);
    assert!(!root.join("replacements-transaction.json").exists());
    f.unchanged();
}

#[test]
fn combined_editor_can_leave_only_one_scope_type_and_noop_save_does_not_add_history() {
    for keep_tables in [false, true] {
        let (f, mut p) = combined_fixture();
        let rules = p.rules.clone();
        let edits = p.edits.clone();
        let revision = p.revision;
        let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
        call(
            &mut p,
            "combined_save",
            json!({"nonce":opened["nonce"],"old":"任务","new":"委托","tables":opened["tables"],"removed":[]}),
        );
        assert_eq!(p.revision, revision);
        assert_eq!(p.rules, rules);
        assert_eq!(p.edits, edits);
        let opened = call(&mut p, "combined_open", json!({"old":"任务","new":"委托"}));
        let tables = if keep_tables {
            opened["tables"].clone()
        } else {
            json!([])
        };
        let removed = if keep_tables {
            json!([opened["members"][0]["index"]])
        } else {
            json!([])
        };
        call(
            &mut p,
            "combined_save",
            json!({"nonce":opened["nonce"],"old":"任务","new":"委托","tables":tables,"removed":removed}),
        );
        assert_eq!(p.rules.is_empty(), !keep_tables);
        assert_eq!(p.edits.is_empty(), keep_tables);
        call(&mut p, "history", json!({"undo":true}));
        assert_eq!(p.rules, rules);
        assert_eq!(p.edits, edits);
        f.unchanged();
    }
}

#[test]
fn cancellation_preserves_import_and_export_files_and_allows_retry() {
    let f = Fixture::new();
    let mut project = f.ready_project();
    let before = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    let revision = project.revision;
    let cancel = Control::new(&|_| false);
    assert!(matches!(
        project.dispatch(
            "import_commit",
            &json!({"revision":revision,"rules":[]}),
            &cancel
        ),
        Err(bajptr_project::Error::Cancelled)
    ));
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), before);
    assert_eq!(project.revision, revision);
    let export = f.root.join("export");
    fs::create_dir_all(&export).unwrap();
    let existing = export.join("LocalizeDBSchema.tsv");
    fs::write(&existing, b"previous export").unwrap();
    let cancel_export =
        Control::new(&|message| !message.is_some_and(|m| m.starts_with("导出来源表：")));
    assert!(matches!(
        project.dispatch("export_text", &json!({"path":export}), &cancel_export),
        Err(bajptr_project::Error::Cancelled)
    ));
    assert_eq!(fs::read(&existing).unwrap(), b"previous export");
    assert_eq!(fs::read_dir(&export).unwrap().count(), 1);
    assert!(
        call(&mut project, "export_text", json!({"path":export}))["count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_ne!(fs::read(existing).unwrap(), b"previous export");
    f.unchanged();
}

#[test]
fn source_and_baseline_commits_report_success_after_a_late_cancel() {
    let f = Fixture::new();
    let mut project = f.project();
    let candidate = call(&mut project, "check", json!({}));
    let payload = json!({"token":candidate["baseline_status"]["token"]});
    let cancel = Control::new(&|message| message != Some("正在验证底稿数据库完整性…"));
    assert!(matches!(
        project.dispatch("baseline_commit", &payload, &cancel),
        Err(bajptr_project::Error::Cancelled)
    ));
    assert_eq!(
        call(&mut project, "check", json!({}))["baseline_status"]["state"],
        "missing"
    );
    let late = Control::new(&|message| message != Some("初始底稿已保存，之后从这份底稿重建。"));
    project
        .dispatch("baseline_commit", &payload, &late)
        .unwrap();
    let early_sources =
        Control::new(&|message| !message.is_some_and(|m| m.starts_with("正在读取初始底稿")));
    assert!(matches!(
        project.dispatch("sources", &json!({}), &early_sources),
        Err(bajptr_project::Error::Cancelled)
    ));
    assert!(!f.data.join(".gui/source-history.json").exists());
    // Legacy unrestricted rules need a recorded inventory before migration.
    call(&mut project, "tables", json!({}));
    let late_source_cancel = std::cell::Cell::new(false);
    let cancel_after_commit = |message: Option<&str>| {
        if message == Some("来源表已就绪。") {
            late_source_cancel.set(true);
            false
        } else {
            true
        }
    };
    let late_sources = Control::new(&cancel_after_commit);
    let result = project
        .dispatch("sources", &json!({}), &late_sources)
        .unwrap();
    assert_eq!(result["missing_history"], false);
    assert!(late_source_cancel.get());
    assert!(f.data.join(".gui/source-history.json").is_file());
    f.unchanged();
}

#[test]
fn worker_reports_localized_file_errors_with_separate_system_details() {
    let f = Fixture::new();
    let worker = Worker::start(f.data.clone(), Arc::new(|| {})).unwrap();
    worker
        .send(1, "import_read", json!({"path":f.root.join("missing.tsv")}))
        .unwrap();
    let event = worker.events.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(event["event"], "error");
    assert!(event["message"].as_str().unwrap().contains("不存在"));
    assert_ne!(event["message"], event["detail"]);
    assert!(!event["detail"].as_str().unwrap().is_empty());
}

#[test]
fn interrupted_export_restores_the_previous_batch_before_retrying() {
    let f = Fixture::new();
    let target = f.root.join("export-recovery");
    let stage = target.join(".bajptr-export-interrupted");
    fs::create_dir_all(stage.join("previous")).unwrap();
    for name in ["a.tsv", "b.tsv"] {
        fs::write(stage.join("previous").join(name), b"old").unwrap();
    }
    fs::write(target.join("a.tsv"), b"new").unwrap();
    fs::write(target.join("b.tsv"), b"old").unwrap();
    fs::write(
        target.join("导出未完成.json"),
        serde_json::to_vec(&json!({"stage":".bajptr-export-interrupted","files":[
            {"name":"a.tsv","before":storage::digest(b"old"),"after":storage::digest(b"new")},
            {"name":"b.tsv","before":storage::digest(b"old"),"after":storage::digest(b"new")}
        ]}))
        .unwrap(),
    )
    .unwrap();
    assert!(
        bajptr_project::text_export::export(&target, &control(), |_| Err(
            bajptr_project::Error::Cancelled
        ))
        .is_err()
    );
    assert_eq!(fs::read(target.join("a.tsv")).unwrap(), b"old");
    assert_eq!(fs::read(target.join("b.tsv")).unwrap(), b"old");
    assert!(!target.join("导出未完成.json").exists());
    assert!(!stage.exists());
}

#[test]
fn export_recovery_preserves_files_changed_by_another_program() {
    let f = Fixture::new();
    let target = f.root.join("export-recovery");
    let stage = target.join(".bajptr-export-interrupted");
    fs::create_dir_all(stage.join("previous")).unwrap();
    fs::write(stage.join("previous/a.tsv"), b"old").unwrap();
    fs::write(target.join("a.tsv"), b"external change").unwrap();
    fs::write(
        target.join("导出未完成.json"),
        serde_json::to_vec(&json!({"stage":".bajptr-export-interrupted","files":[
            {"name":"a.tsv","before":storage::digest(b"old"),"after":storage::digest(b"new")}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let entered = std::cell::Cell::new(false);
    let error = bajptr_project::text_export::export(&target, &control(), |_| {
        entered.set(true);
        Ok(0)
    })
    .unwrap_err();
    assert!(error.to_string().contains("尚未恢复完成"));
    assert!(!entered.get());
    assert_eq!(fs::read(target.join("a.tsv")).unwrap(), b"external change");
    assert!(target.join("导出未完成.json").exists());
    assert!(stage.join("previous/a.tsv").is_file());
}

#[cfg(windows)]
#[test]
fn exporting_a_batch_rolls_back_earlier_files_when_a_later_file_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    for first_existed in [false, true] {
        let mut f = Fixture::new();
        add_new_source(&mut f, false);
        let mut p = f.ready_project();
        let target = f.root.join("export-transaction");
        fs::create_dir(&target).unwrap();
        let first = target.join("LocalizeDBSchema.tsv");
        let second = target.join("LocalizeSkillDBSchema.tsv");
        if first_existed {
            fs::write(&first, b"old first").unwrap();
        }
        fs::write(&second, b"old second").unwrap();
        fs::write(target.join("keep.txt"), b"unrelated").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1 | 2)
            .open(&second)
            .unwrap();
        let error = p
            .dispatch("export_text", &json!({"path":target}), &control())
            .unwrap_err();
        assert!(error.to_string().contains("已撤回本次写入"), "{error}");
        assert_eq!(fs::read(&second).unwrap(), b"old second");
        if first_existed {
            assert_eq!(fs::read(&first).unwrap(), b"old first");
        } else {
            assert!(!first.exists());
        }
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"unrelated");
        assert_eq!(
            fs::read_dir(&target).unwrap().count(),
            2 + usize::from(first_existed)
        );
        drop(held);
        assert!(
            call(&mut p, "export_text", json!({"path":target}))["count"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_ne!(fs::read(&first).unwrap(), b"old first");
        assert_ne!(fs::read(&second).unwrap(), b"old second");
        assert!(!target.join("导出未完成.json").exists());
        f.unchanged();
    }
}

#[test]
fn invalid_application_state_is_reported_and_never_overwritten() {
    for contents in [
        b"{broken".as_slice(),
        b"{}",
        b"{\"format\":1,\"games\":{}}",
        b"{\"format\":2,\"games\":{\"game\":false}}",
    ] {
        let f = Fixture::new();
        let mut p = f.ready_project();
        let path = f.data.join(".gui/application-state.json");
        fs::write(&path, contents).unwrap();
        let error = p
            .dispatch("check", &json!({}), &control())
            .unwrap_err()
            .to_string();
        assert!(error.contains("应用状态文件"), "{error}");
        assert!(error.contains("损坏") || error.contains("格式"));
        let config = bajptr_project::service::Config::new(p.rules.clone(), vec![], true);
        assert!(
            bajptr_project::service::record(
                f.game.to_str().unwrap(),
                &f.data,
                &config,
                "restored",
                None
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), contents);
        let reopened = f.project();
        assert!(
            reopened.state().unwrap()["errors"]
                .get("application-state.json")
                .is_some()
        );
        f.unchanged();
    }
}

#[test]
fn unreadable_application_state_is_reported_as_a_read_failure() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    fs::create_dir(f.data.join(".gui/application-state.json")).unwrap();
    let error = p
        .dispatch("check", &json!({}), &control())
        .unwrap_err()
        .to_string();
    assert!(error.contains("应用状态文件无法读取"), "{error}");
    f.unchanged();
}

#[cfg(windows)]
#[test]
fn failed_atomic_publication_removes_pending_files_and_keeps_the_old_catalog() {
    let f = Fixture::new();
    let _held = lock_catalog_against_replacement(&f);
    let catalog = f.game.join(region::CATALOG);
    assert!(storage::atomic_write(&catalog, b"replacement", None).is_err());
    assert_eq!(fs::read(&catalog).unwrap(), f.before);
    assert!(
        !fs::read_dir(catalog.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".pending-"))
    );
}

#[test]
fn receipt_maintenance_keeps_missing_applied_games_and_live_restored_games() {
    let f = Fixture::new();
    let project = f.ready_project();
    let missing_restored = storage::identity(&f.root.join("removed-restored-game")).unwrap();
    let missing_applied = storage::identity(&f.root.join("removed-applied-game")).unwrap();
    let other = f.root.join("other-game");
    fs::create_dir(&other).unwrap();
    let live_restored = storage::identity(&other).unwrap();
    let mut games = json!({});
    let identity =
        region::resource_identity(&region::locate(f.game.to_str().unwrap()).unwrap()).unwrap();
    games[&missing_restored] = json!({"kind":"restored","identity":identity,"fingerprint":"prior"});
    games[&missing_applied] =
        json!({"kind":"applied","identity":identity,"fingerprint":"must-preserve"});
    games[&live_restored] = json!({"kind":"restored","identity":identity,"fingerprint":"prior"});
    let path = f.data.join(".gui/application-state.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"format":2,"games":games})).unwrap(),
    )
    .unwrap();
    let config = bajptr_project::service::Config::new(project.rules.clone(), vec![], true);
    bajptr_project::service::record(f.game.to_str().unwrap(), &f.data, &config, "restored", None)
        .unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(saved["games"].get(&missing_restored).is_none());
    assert_eq!(saved["games"][&missing_applied], games[&missing_applied]);
    assert_eq!(saved["games"][&live_restored], games[&live_restored]);
    let current = storage::identity(&f.game).unwrap();
    assert_eq!(saved["games"][&current]["kind"], "restored");
    f.unchanged();
}

fn add_new_source(f: &mut Fixture, malformed: bool) {
    let game = region::locate(f.game.to_str().unwrap()).unwrap();
    let plain = f.root.join("new-source.db");
    let encrypted = f.root.join("new-source.encrypted");
    let key = bajptr_core::codec::key(region::KEY).unwrap();
    bajptr_core::codec::decrypt(&game.bundle, &plain, &key, &control()).unwrap();
    let con = rusqlite::Connection::open(&plain).unwrap();
    con.execute_batch("CREATE TABLE LocalizeSkillDBSchema(Bytes BLOB); INSERT INTO LocalizeSkillDBSchema SELECT Bytes FROM LocalizeDBSchema WHERE rowid=1;").unwrap();
    if malformed {
        con.execute_batch("UPDATE LocalizeSkillDBSchema SET Bytes=X'010203'")
            .unwrap();
    }
    drop(con);
    let (_, inspection) =
        bajptr_core::codec::encrypt_checked(&plain, &encrypted, &key, &[0; 16], &[], &control())
            .unwrap();
    fs::copy(
        &encrypted,
        game.bundle
            .with_file_name(format!("123456_{}", inspection.crc)),
    )
    .unwrap();
    region::publish_catalog(
        &game,
        fs::metadata(&encrypted).unwrap().len(),
        inspection.crc,
    )
    .unwrap();
    f.before = fs::read(&game.catalog).unwrap();
}

#[test]
fn new_source_notices_freeze_legacy_cache_and_survive_restart_until_acknowledged() {
    let mut f = Fixture::new();
    let mut p = f.ready_project();
    call(&mut p, "tables", json!({})); // Last recorded inventory contains only the original source.
    add_new_source(&mut f, false);
    let check = call(&mut p, "check", json!({}));
    call(
        &mut p,
        "baseline_commit",
        json!({"token":check["baseline_status"]["token"]}),
    );
    let sync = call(&mut p, "sources", json!({}));
    assert_eq!(sync["migrated"], 2);
    assert_eq!(sync["missing_history"], false);
    assert!(p.rules.iter().all(|r| r.table == "LocalizeDBSchema"));
    let notices = sync["state"]["source_notices"].as_array().unwrap();
    assert_eq!(notices.len(), 2);
    assert!(
        notices
            .iter()
            .all(|n| n["table"] == "LocalizeSkillDBSchema" && n["fields"] == 1)
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(
        preview["total_changes"], 1,
        "new sources must not join replacement automatically"
    );
    assert!(
        preview["stats"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["table"] == "LocalizeDBSchema")
    );
    let mut reopened = f.project();
    let again = call(&mut reopened, "sources", json!({}));
    assert_eq!(again["migrated"], 0);
    assert_eq!(
        again["state"]["source_notices"].as_array().unwrap().len(),
        2
    );
    let revision = reopened.revision;
    call(&mut reopened, "sources_ack", json!({"revision":revision}));
    let mut reopened = f.project();
    assert!(
        call(&mut reopened, "sources", json!({}))["state"]["source_notices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let revision = reopened.revision;
    call(
        &mut reopened,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"阿罗娜","new":"阿洛娜同学","mode":"sub","table":"[\"LocalizeDBSchema\",\"LocalizeSkillDBSchema\"]"}}),
    );
    assert_eq!(
        call(&mut reopened, "preview", json!({}))["total_changes"],
        2
    );
    f.unchanged();
}

#[test]
fn missing_or_foreign_source_history_never_guesses_a_legacy_scope() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let before = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    assert_eq!(call(&mut p, "sources", json!({}))["missing_history"], true);
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), before);
    let other = Fixture::new();
    let mut other_p = other.ready_project();
    call(&mut other_p, "tables", json!({}));
    fs::copy(
        other.data.join(".gui/tables-cache.json"),
        f.data.join(".gui/tables-cache.json"),
    )
    .unwrap();
    assert_eq!(call(&mut p, "sources", json!({}))["missing_history"], true);
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), before);
    f.unchanged();
}

#[test]
fn damaged_baseline_rebuild_preserves_evidence_and_restores_preview() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let baseline = call(&mut p, "baseline", json!({}));
    let folder = PathBuf::from(baseline["folder"].as_str().unwrap());
    fs::write(folder.join("baseline.json"), b"{broken").unwrap();
    assert!(p.dispatch("check", &json!({}), &control()).is_err());
    assert!(p.dispatch("preview", &json!({}), &control()).is_err());
    let plan = call(&mut p, "baseline_rebuild_preview", json!({}));
    assert_eq!(fs::read(folder.join("baseline.json")).unwrap(), b"{broken");
    assert!(
        p.dispatch(
            "baseline_rebuild_commit",
            &json!({"token":plan["token"]}),
            &Control::new(&|_| false)
        )
        .is_err()
    );
    let rebuilt = call(
        &mut p,
        "baseline_rebuild_commit",
        json!({"token":plan["token"]}),
    );
    let archived = PathBuf::from(rebuilt["damaged_folder"].as_str().unwrap());
    assert_eq!(
        fs::read(archived.join("baseline.json")).unwrap(),
        b"{broken"
    );
    assert_eq!(call(&mut p, "check", json!({}))["connected"], true);
    assert!(
        call(&mut p, "preview", json!({}))["total_changes"]
            .as_u64()
            .unwrap()
            > 0
    );
    f.unchanged();
}

#[test]
fn rebuild_refuses_modified_game_text_when_original_database_is_unavailable() {
    let f = Fixture::new();
    let original = region::locate(f.game.to_str().unwrap()).unwrap().bundle;
    let mut p = f.ready_project();
    let baseline = call(&mut p, "baseline", json!({}));
    let folder = PathBuf::from(baseline["folder"].as_str().unwrap());
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    if original.exists() {
        storage::retire(&original).unwrap();
    }
    fs::write(
        folder.join(baseline["resource"].as_str().unwrap()),
        b"broken database",
    )
    .unwrap();
    let catalog = fs::read(f.game.join(region::CATALOG)).unwrap();
    assert!(
        p.dispatch("baseline_rebuild_preview", &json!({}), &control())
            .is_err()
    );
    assert_eq!(
        fs::read(folder.join(baseline["resource"].as_str().unwrap())).unwrap(),
        b"broken database"
    );
    assert_eq!(fs::read(f.game.join(region::CATALOG)).unwrap(), catalog);
}

#[test]
fn damaged_records_can_be_archived_individually_without_losing_healthy_data() {
    let f = Fixture::new();
    fs::write(f.data.join(".gui/text-edits.json"), b"{broken edits").unwrap();
    fs::write(f.data.join(".gui/source-history.json"), b"{broken sources").unwrap();
    let mut p = f.project();
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    for (name, bytes) in [
        ("text-edits.json", b"{broken edits".as_slice()),
        ("source-history.json", b"{broken sources".as_slice()),
    ] {
        let revision = p.revision;
        let reset = call(
            &mut p,
            "reset_record",
            json!({"name":name,"revision":revision}),
        );
        assert_eq!(
            fs::read(reset["archived"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(!f.data.join(".gui").join(name).exists());
        assert!(reset["state"]["errors"].get(name).is_none());
        assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    }
    let revision = p.revision;
    assert!(
        p.dispatch(
            "reset_record",
            &json!({"name":"rules.tsv","revision":revision}),
            &control()
        )
        .is_err()
    );
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":null,"rule":{"old":"测试词","new":"修复后","mode":"sub","table":"LocalizeDBSchema"}}),
    );
    f.unchanged();
}

#[test]
fn reset_rejects_records_changed_after_the_error_was_reported() {
    let f = Fixture::new();
    let path = f.data.join(".gui/text-edits.json");
    fs::write(&path, b"{broken").unwrap();
    let mut p = f.project();
    fs::write(&path, b"{another change").unwrap();
    let revision = p.revision;
    assert!(
        p.dispatch(
            "reset_record",
            &json!({"name":"text-edits.json","revision":revision}),
            &control()
        )
        .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), b"{another change");
}

#[test]
fn corrupt_settings_report_a_visible_warning_and_preserve_the_file_until_saved() {
    let f = Fixture::new();
    let settings = f.data.join(".gui/settings.json");
    fs::write(&settings, b"{broken settings").unwrap();
    let mut p = f.project();
    let state = p.state().unwrap();
    assert!(
        state["settings_warning"]
            .as_str()
            .unwrap()
            .contains("设置读取失败")
    );
    assert_eq!(fs::read(&settings).unwrap(), b"{broken settings");
    assert_eq!(state["game"], "");
    let revision = p.revision;
    let saved = call(
        &mut p,
        "settings",
        json!({"game":f.game,"skip_kana":true,"revision":revision}),
    );
    assert!(saved["settings_warning"].is_null());
}

#[cfg(windows)]
#[test]
fn failed_source_history_publication_rolls_back_rules_and_in_memory_state() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let key = storage::identity(&f.game).unwrap();
    let history = f.data.join(".gui/source-history.json");
    fs::write(
        &history,
        serde_json::to_vec(
            &json!({"format":1,"games":{key:{"known":["LocalizeDBSchema"],"notices":[]}}}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut p = f.ready_project();
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    let previous = fs::read(&history).unwrap();
    let state = p.state().unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&history)
        .unwrap();
    assert!(p.dispatch("sources", &json!({}), &control()).is_err());
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    assert_eq!(fs::read(&history).unwrap(), previous);
    assert_eq!(p.state().unwrap(), state);
    drop(held);
    let retry = call(&mut p, "sources", json!({}));
    assert!(retry["migrated"].as_u64().unwrap() > 0);
    assert_eq!(retry["missing_history"], false);
    f.unchanged();
}

#[test]
fn interrupted_source_save_recovers_both_documents_on_restart() {
    for history_published in [false, true] {
        let f = Fixture::new();
        let root = f.data.join(".gui");
        let before = fs::read(root.join("rules.tsv")).unwrap();
        let after = model_rules_with_fixed_scope(&before);
        let history = serde_json::to_vec(&json!({"format":1,"games":{}})).unwrap();
        fs::write(root.join("rules.tsv"), &after).unwrap();
        if history_published {
            fs::write(root.join("source-history.json"), &history).unwrap();
        }
        fs::write(
            root.join("sources-transaction.json"),
            serde_json::to_vec(&json!([
                {"name":"rules.tsv","before":before,"after":after},
                {"name":"source-history.json","before":null,"after":history}
            ]))
            .unwrap(),
        )
        .unwrap();
        let p = f.project();
        assert!(p.state().unwrap()["errors"].as_object().unwrap().is_empty());
        assert_eq!(fs::read(root.join("rules.tsv")).unwrap(), before);
        assert!(!root.join("source-history.json").exists());
        assert!(!root.join("sources-transaction.json").exists());
        f.unchanged();
    }
}

fn model_rules_with_fixed_scope(bytes: &[u8]) -> Vec<u8> {
    let mut rules = bajptr_project::model::parse_rules(bytes).unwrap();
    for rule in &mut rules {
        rule.table = "LocalizeDBSchema".into();
    }
    bajptr_project::model::rules_bytes(&rules).unwrap()
}

#[test]
fn interrupted_source_recovery_refuses_to_overwrite_subsequent_external_changes() {
    let f = Fixture::new();
    let root = f.data.join(".gui");
    let before = fs::read(root.join("rules.tsv")).unwrap();
    let after = model_rules_with_fixed_scope(&before);
    fs::write(
        root.join("sources-transaction.json"),
        serde_json::to_vec(&json!([
            {"name":"rules.tsv","before":before,"after":after},
            {"name":"source-history.json","before":null,"after":[]}
        ]))
        .unwrap(),
    )
    .unwrap();
    fs::write(root.join("rules.tsv"), b"external changes").unwrap();
    let mut p = Project::new(f.data.clone());
    assert!(p.dispatch("load", &json!({}), &control()).is_err());
    assert_eq!(
        fs::read(root.join("rules.tsv")).unwrap(),
        b"external changes"
    );
    assert!(root.join("sources-transaction.json").exists());
}

#[test]
fn damaged_catalog_or_database_can_rebuild_from_matching_game_resources() {
    for name in ["TableCatalog.bytes", "database"] {
        let f = Fixture::new();
        let mut p = f.ready_project();
        let info = call(&mut p, "baseline", json!({}));
        let folder = PathBuf::from(info["folder"].as_str().unwrap());
        let damaged = folder.join(if name == "database" {
            info["resource"].as_str().unwrap()
        } else {
            name
        });
        fs::write(&damaged, b"broken").unwrap();
        let plan = call(&mut p, "baseline_rebuild_preview", json!({}));
        call(
            &mut p,
            "baseline_rebuild_commit",
            json!({"token":plan["token"]}),
        );
        assert_eq!(call(&mut p, "check", json!({}))["connected"], true);
        f.unchanged();
    }
}

#[test]
fn baseline_rebuild_rejects_stale_confirmation() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let info = call(&mut p, "baseline", json!({}));
    let path = PathBuf::from(info["folder"].as_str().unwrap()).join("baseline.json");
    fs::write(&path, b"broken").unwrap();
    let plan = call(&mut p, "baseline_rebuild_preview", json!({}));
    fs::write(&path, b"another change").unwrap();
    assert!(
        p.dispatch(
            "baseline_rebuild_commit",
            &json!({"token":plan["token"]}),
            &control()
        )
        .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), b"another change");
    f.unchanged();
}

#[test]
fn damaged_wordlist_reset_preserves_the_text_edit_document() {
    let f = Fixture::new();
    let edits = serde_json::to_vec(&json!({"format":2,"edits":[]})).unwrap();
    fs::write(f.data.join(".gui/text-edits.json"), &edits).unwrap();
    fs::write(f.data.join(".gui/rules.tsv"), b"broken wordlist").unwrap();
    let mut p = f.project();
    let revision = p.revision;
    let reset = call(
        &mut p,
        "reset_record",
        json!({"name":"rules.tsv","revision":revision}),
    );
    assert_eq!(
        fs::read(reset["archived"].as_str().unwrap()).unwrap(),
        b"broken wordlist"
    );
    assert_eq!(
        fs::read(f.data.join(".gui/text-edits.json")).unwrap(),
        edits
    );
    assert_eq!(reset["state"]["rules"], json!([]));
}

#[test]
fn corrupt_source_history_is_preserved_and_cannot_expand_rules() {
    let f = Fixture::new();
    fs::write(f.data.join(".gui/source-history.json"), b"{broken").unwrap();
    let mut p = f.ready_project();
    let before = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    assert!(p.dispatch("sources", &json!({}), &control()).is_err());
    assert_eq!(
        fs::read(f.data.join(".gui/source-history.json")).unwrap(),
        b"{broken"
    );
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), before);
    f.unchanged();
}

#[test]
fn failed_new_source_scan_preserves_previous_inventory_and_legacy_rules() {
    let mut f = Fixture::new();
    let mut p = f.ready_project();
    call(&mut p, "tables", json!({}));
    let cache = fs::read(f.data.join(".gui/tables-cache.json")).unwrap();
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    add_new_source(&mut f, true);
    let check = call(&mut p, "check", json!({}));
    call(
        &mut p,
        "baseline_commit",
        json!({"token":check["baseline_status"]["token"]}),
    );
    assert!(p.dispatch("sources", &json!({}), &control()).is_err());
    assert_eq!(
        fs::read(f.data.join(".gui/tables-cache.json")).unwrap(),
        cache
    );
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    assert!(!f.data.join(".gui/source-history.json").exists());
    f.unchanged();
}

#[test]
fn unchanged_source_inventory_reuses_metadata_without_opening_the_database() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    call(&mut p, "tables", json!({}));
    call(&mut p, "sources", json!({}));
    let mut reopened = f.project();
    let ready_messages = std::cell::Cell::new(0);
    let sync = reopened
        .dispatch(
            "sources",
            &json!({}),
            &Control::new(&|message| {
                if message == Some("来源表已就绪。") {
                    ready_messages.set(ready_messages.get() + 1);
                }
                !message.is_some_and(|m| m.starts_with("正在读取初始底稿"))
            }),
        )
        .unwrap();
    assert_eq!(sync["missing_history"], false);
    assert_eq!(
        ready_messages.get(),
        1,
        "one source sync should report readiness once"
    );
    assert!(
        sync["state"]["source_notices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.unchanged();
}

fn generate_fixture_patch(p: &mut Project) -> bajptr_project::Result<Value> {
    let config = bajptr_project::service::Config::new(p.rules.clone(), p.edits.clone(), p.skip);
    p.engine.generate(
        bajptr_project::service::PatchJob {
            game: &p.game,
            data: &p.data,
            backup_root: &p.data.join("backup"),
            output_root: &p.data.join("output"),
            config: &config,
            install: false,
            expected: None,
        },
        &control(),
    )
}

#[test]
fn prepared_preview_never_writes_game_and_reopens_without_rebuilding() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    let prepared = call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}));
    assert_eq!(prepared["ready"], true);
    assert_eq!(prepared["reused"], false);
    assert!(!f.data.join("output").exists());
    f.unchanged();
    let reopened = call(&mut p, "preview", json!({}));
    let reused = call(&mut p, "prepare_patch", json!({"nonce":reopened["nonce"]}));
    assert_eq!(reused["cache_id"], prepared["cache_id"]);
    assert_eq!(reused["reused"], true);
    let applied = p
        .dispatch(
            "apply",
            &json!({"nonce":reopened["nonce"],"prepared":true}),
            &Control::new(&|message| {
                !message.is_some_and(|m| {
                    m.starts_with("正在从初始底稿重建") || m.starts_with("正在加密数据库")
                })
            }),
        )
        .unwrap();
    assert_eq!(applied["installed"], true);
    assert_eq!(applied["prepared_reused"], true);
    assert!(!f.data.join("output").exists());
}

#[test]
fn editing_invalidates_prepared_patch_and_confirm_requires_ready_patch() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    assert!(
        p.dispatch(
            "apply",
            &json!({"nonce":preview["nonce"],"prepared":true}),
            &control()
        )
        .is_err()
    );
    let prepared = call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}));
    let revision = p.revision;
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"阿罗娜","new":"不同的新名字","mode":"sub","source":"测试","table":""}}),
    );
    assert!(!Path::new(prepared["prepared_path"].as_str().unwrap()).exists());
    assert!(
        p.dispatch(
            "apply",
            &json!({"nonce":preview["nonce"],"prepared":true}),
            &control()
        )
        .is_err()
    );
    let fresh = call(&mut p, "preview", json!({}));
    let replacement = call(&mut p, "prepare_patch", json!({"nonce":fresh["nonce"]}));
    assert_ne!(replacement["cache_id"], prepared["cache_id"]);
    assert_eq!(replacement["reused"], false);
    f.unchanged();
}

#[test]
fn changed_prepared_file_is_rejected_before_game_write() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    let prepared = call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}));
    let path = Path::new(prepared["prepared_path"].as_str().unwrap());
    let mut bytes = fs::read(path).unwrap();
    bytes[4000] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(
        p.dispatch(
            "apply",
            &json!({"nonce":preview["nonce"],"prepared":true}),
            &control()
        )
        .is_err()
    );
    f.unchanged();
}

#[test]
fn prepared_patch_rejects_external_inputs_and_option_change_clears_cache() {
    for external in ["rules", "game", "baseline", "option"] {
        let f = Fixture::new();
        let mut p = f.ready_project();
        let preview = call(&mut p, "preview", json!({}));
        let prepared = call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}));
        match external {
            "rules" => {
                fs::write(f.data.join(".gui/rules.tsv"), "外部规则\t新内容\tsub\t\t\n").unwrap()
            }
            "game" => {
                let path = region::locate(f.game.to_str().unwrap()).unwrap().bundle;
                let mut bytes = fs::read(&path).unwrap();
                bytes[4000] ^= 1;
                fs::write(path, bytes).unwrap();
            }
            "baseline" => {
                let baseline = call(&mut p, "baseline", json!({}));
                let folder = Path::new(baseline["folder"].as_str().unwrap());
                let path = folder.join("baseline.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["format"] = json!(1);
                fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => {
                let revision = p.revision;
                call(
                    &mut p,
                    "settings",
                    json!({"game":f.game,"revision":revision,"skip_kana":false}),
                );
                assert!(!Path::new(prepared["prepared_path"].as_str().unwrap()).exists());
            }
        }
        assert!(
            p.dispatch(
                "apply",
                &json!({"nonce":preview["nonce"],"prepared":true}),
                &control()
            )
            .is_err(),
            "{external}"
        );
        f.unchanged();
    }
}

#[test]
fn cancelled_preparation_can_retry_without_writing_game_or_exporting_patch() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    let result = p.dispatch(
        "prepare_patch",
        &json!({"nonce":preview["nonce"]}),
        &Control::new(&|message| !message.is_some_and(|m| m.starts_with("正在加密数据库"))),
    );
    assert!(matches!(result, Err(bajptr_project::Error::Cancelled)));
    assert!(!f.data.join("output").exists());
    f.unchanged();
    assert_eq!(
        call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}))["ready"],
        true
    );
}

#[cfg(windows)]
fn lock_catalog_against_replacement(f: &Fixture) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(f.game.join(region::CATALOG))
        .unwrap()
}

#[cfg(windows)]
#[test]
fn prepared_install_failure_exports_patch_and_retries_without_rebuilding() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "prepare_patch", json!({"nonce":preview["nonce"]}));
    let locked = lock_catalog_against_replacement(&f);
    let no_build = Control::new(&|message| {
        !message
            .is_some_and(|m| m.starts_with("正在从初始底稿重建") || m.starts_with("正在加密数据库"))
    });
    let fallback = p
        .dispatch(
            "apply",
            &json!({"nonce":preview["nonce"],"prepared":true}),
            &no_build,
        )
        .unwrap();
    assert_eq!(fallback["installed"], false);
    assert_eq!(fallback["prepared_reused"], true);
    let output = Path::new(fallback["output"].as_str().unwrap());
    let checked = bajptr_core::codec::inspect(
        output,
        &bajptr_core::codec::key(region::KEY).unwrap(),
        &control(),
    )
    .unwrap();
    assert_eq!(checked.bad, 0);
    assert!(output.with_file_name("TableCatalog.bytes").is_file());
    f.unchanged();
    drop(locked);
    assert_eq!(
        p.dispatch(
            "apply",
            &json!({"nonce":preview["nonce"],"prepared":true}),
            &no_build
        )
        .unwrap()["installed"],
        true
    );
}

#[cfg(windows)]
#[test]
fn failed_game_write_saves_a_verified_patch_and_allows_retry() {
    let f = Fixture::new();
    let before_files = fs::read_dir(f.game.join(region::BUNDLES))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    let locked = lock_catalog_against_replacement(&f);
    let fallback = call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    assert_eq!(fallback["installed"], false);
    assert_eq!(fallback["verified"], true);
    assert!(!fallback["install_error"].as_str().unwrap().is_empty());
    assert!(fallback["warning"].as_str().unwrap().contains("补丁"));
    assert_ne!(fallback["application"], "已应用，与当前词表一致");
    let output = PathBuf::from(fallback["output"].as_str().unwrap());
    assert!(output.starts_with(f.data.join("output")));
    let catalog = output.with_file_name("TableCatalog.bytes");
    let entry = region::parse(&fs::read(&catalog).unwrap()).unwrap();
    let checked = bajptr_core::codec::inspect(
        &output,
        &bajptr_core::codec::key(region::KEY).unwrap(),
        &control(),
    )
    .unwrap();
    assert_eq!(checked.bad, 0);
    assert_eq!(checked.crc, entry.crc);
    assert_eq!(fs::metadata(&output).unwrap().len(), entry.size);
    assert!(output.with_file_name("TableCatalog.hash").is_file());
    assert_eq!(
        fs::read_dir(f.game.join(region::BUNDLES))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>(),
        before_files
    );
    assert!(
        !walkdir::WalkDir::new(&f.game)
            .into_iter()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".pending-"))
    );
    f.unchanged();
    drop(locked);
    let applied = call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    assert_eq!(applied["installed"], true);
    assert_eq!(applied["application"], "已应用，与当前词表一致");
    assert_ne!(fs::read(f.game.join(region::CATALOG)).unwrap(), f.before);
    assert_eq!(fs::read_dir(f.data.join("output")).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn failed_fallback_save_reports_both_errors_and_keeps_recovery_files() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    fs::write(f.data.join("output"), b"blocked output directory").unwrap();
    let _locked = lock_catalog_against_replacement(&f);
    let error = p
        .dispatch("apply", &json!({"nonce":preview["nonce"]}), &control())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("应用失败") && error.contains("补丁保存失败"),
        "{error}"
    );
    assert!(error.contains("暂存"), "{error}");
    assert!(
        walkdir::WalkDir::new(f.data.join("work"))
            .into_iter()
            .filter_map(Result::ok)
            .any(|e| e.file_name() == "encrypted.db")
    );
    assert_ne!(p.application, "已应用，与当前词表一致");
    f.unchanged();
}

#[test]
fn first_baseline_requires_confirmation_and_survives_restart() {
    let f = Fixture::new();
    let mut p = f.project();
    let status = call(&mut p, "check", json!({}));
    assert_eq!(status["baseline_status"]["state"], "missing");
    assert!(!f.data.join("backup/_baselines").exists());
    for action in ["tables", "baseline", "preview", "apply", "restore"] {
        assert!(
            p.dispatch(action, &json!({}), &control()).is_err(),
            "{action}"
        );
    }
    assert!(!f.data.join("backup/_baselines").exists());
    let token = status["baseline_status"]["token"].clone();
    let saved = call(&mut p, "baseline_commit", json!({"token":token}));
    assert!(saved["verified"].as_bool().unwrap());
    let mut reopened = f.project();
    assert_eq!(
        call(&mut reopened, "check", json!({}))["baseline_status"]["state"],
        "ready"
    );
    assert!(
        call(&mut reopened, "preview", json!({}))["total_changes"]
            .as_u64()
            .unwrap()
            > 0
    );
    f.unchanged();
}

#[test]
fn game_update_prompts_without_retiring_previous_baseline_until_confirmed() {
    let f = Fixture::new();
    let mut p = f.project();
    let first = call(&mut p, "check", json!({}));
    let saved = call(
        &mut p,
        "baseline_commit",
        json!({"token":first["baseline_status"]["token"]}),
    );
    let previous = PathBuf::from(saved["folder"].as_str().unwrap());
    fs::write(
        f.game
            .join(region::CATALOG)
            .with_file_name("TableCatalog.hash"),
        b"synthetic game update",
    )
    .unwrap();
    let mut reopened = f.project();
    let updated = call(&mut reopened, "check", json!({}));
    assert_eq!(updated["baseline_status"]["state"], "update");
    assert!(previous.join("baseline.json").is_file());
    assert!(
        reopened
            .dispatch("preview", &json!({}), &control())
            .is_err()
    );
    assert!(previous.join("baseline.json").is_file());
    let newest = call(
        &mut reopened,
        "baseline_commit",
        json!({"token":updated["baseline_status"]["token"]}),
    );
    assert_ne!(newest["folder"], saved["folder"]);
    assert!(!previous.exists());
    assert_eq!(
        call(&mut reopened, "check", json!({}))["baseline_status"]["state"],
        "ready"
    );
    f.unchanged();
}

#[test]
fn migration_after_update_preserves_pairs_backs_up_and_rejects_stale_or_cancelled_saves() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let opened = call(
        &mut p,
        "text_open",
        json!({"index":0,"search_nonce":search["nonce"]}),
    );
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"阿罗娜的任务【单条】"}),
    );
    let batch = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"任务","new":"委托"}),
    );
    call(&mut p, "batch_save", json!({"nonce":batch["nonce"]}));
    let edits_path = f.data.join(".gui/text-edits.json");
    let before = fs::read(&edits_path).unwrap();
    let original_edits = p.edits.clone();
    let hash = f
        .game
        .join(region::CATALOG)
        .with_file_name("TableCatalog.hash");
    fs::write(&hash, b"migration update").unwrap();
    let check = call(&mut p, "check", json!({}));
    assert_eq!(check["baseline_status"]["state"], "update");
    assert!(
        p.dispatch("migration_preview", &json!({}), &control())
            .is_err()
    );
    call(
        &mut p,
        "baseline_commit",
        json!({"token":check["baseline_status"]["token"]}),
    );
    assert_eq!(call(&mut p, "check", json!({}))["migration_count"], 2);
    let plan = call(&mut p, "migration_preview", json!({}));
    assert_eq!(plan["items"].as_array().unwrap().len(), 1);
    assert_eq!(plan["items"][0]["kind"], "exact");
    assert_eq!(fs::read(&edits_path).unwrap(), before);
    let payload = json!({"revision":p.revision,"nonce":plan["nonce"],"choices":[{"item":0,"candidate":plan["items"][0]["suggested"]}]});
    assert!(matches!(
        p.dispatch("migration_commit", &payload, &Control::new(&|_| false)),
        Err(bajptr_project::Error::Cancelled)
    ));
    assert_eq!(fs::read(&edits_path).unwrap(), before);
    fs::write(&hash, b"changed after review").unwrap();
    assert!(
        p.dispatch("migration_commit", &payload, &control())
            .is_err()
    );
    assert_eq!(fs::read(&edits_path).unwrap(), before);
    fs::write(&hash, b"migration update").unwrap();
    let saved = call(&mut p, "migration_start", json!({}));
    assert_eq!(saved["automatic_count"], 1);
    assert!(saved["plan"]["items"].as_array().unwrap().is_empty());
    assert_eq!(fs::read(saved["backup"].as_str().unwrap()).unwrap(), before);
    assert_eq!(p.edits.len(), 2);
    assert!(
        p.edits
            .iter()
            .all(|e| Some(&e.version) == p.version.as_ref())
    );
    assert!(
        p.dispatch("migration_commit", &payload, &control())
            .is_err()
    );
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["rows"][0]["text"], "阿罗娜的委托【单条】");
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits, original_edits);
    call(&mut p, "history", json!({"undo":false}));
    assert!(
        p.edits
            .iter()
            .all(|e| Some(&e.version) == p.version.as_ref())
    );
    let mut reopened = f.project();
    assert_eq!(
        call(&mut reopened, "check", json!({}))["migration_count"],
        0
    );
    f.unchanged();
}

#[test]
fn migration_start_saves_unchanged_content_even_when_its_position_moved() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    for (keyword, replacement) in [("任务", "自动迁移"), ("老师", "人工迁移")] {
        let search = call(&mut p, "search", json!({"keyword":keyword}));
        let opened = call(
            &mut p,
            "text_open",
            json!({"index":0,"search_nonce":search["nonce"]}),
        );
        call(
            &mut p,
            "text_save",
            json!({"nonce":opened["nonce"],"text":replacement}),
        );
    }
    assert_eq!(p.edits.len(), 2);
    p.edits[1].row_id = bajptr_core::database::RowId::Integer(1_000_000);
    p.store
        .save(
            "text-edits.json",
            &bajptr_project::model::edits_bytes(&p.edits).unwrap(),
        )
        .unwrap();
    let old = p.edits.clone();
    fs::write(
        f.game
            .join(region::CATALOG)
            .with_file_name("TableCatalog.hash"),
        b"mixed migration update",
    )
    .unwrap();
    let check = call(&mut p, "check", json!({}));
    call(
        &mut p,
        "baseline_commit",
        json!({"token":check["baseline_status"]["token"]}),
    );
    let started = call(&mut p, "migration_start", json!({}));
    assert_eq!(started["automatic_count"], 2);
    assert!(started["plan"]["items"].as_array().unwrap().is_empty());
    assert_eq!(started["plan"]["revision"], p.revision);
    let after_automatic = p.edits.clone();
    assert_eq!(
        after_automatic
            .iter()
            .filter(|e| Some(&e.version) == p.version.as_ref())
            .count(),
        2
    );
    assert_eq!(
        f.project().edits,
        after_automatic,
        "closing review must keep automatic migrations"
    );
    assert!(
        p.edits
            .iter()
            .all(|e| Some(&e.version) == p.version.as_ref())
    );
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits, old);
    call(&mut p, "history", json!({"undo":false}));
    assert_eq!(p.edits, after_automatic);
    f.unchanged();
}

#[test]
fn stale_confirmation_and_invalid_new_resources_preserve_previous_baseline() {
    let f = Fixture::new();
    let mut p = f.project();
    let first = call(&mut p, "check", json!({}));
    let saved = call(
        &mut p,
        "baseline_commit",
        json!({"token":first["baseline_status"]["token"]}),
    );
    let previous = PathBuf::from(saved["folder"].as_str().unwrap());
    let marker = f
        .game
        .join(region::CATALOG)
        .with_file_name("TableCatalog.hash");
    fs::write(&marker, b"update A").unwrap();
    let pending = call(&mut p, "check", json!({}));
    fs::write(&marker, b"update B").unwrap();
    assert!(
        p.dispatch(
            "baseline_commit",
            &json!({"token":pending["baseline_status"]["token"]}),
            &control()
        )
        .is_err()
    );
    let live = region::locate(f.game.to_str().unwrap()).unwrap();
    let mut bytes = fs::read(&live.bundle).unwrap();
    bytes[200] ^= 1;
    fs::write(&live.bundle, bytes).unwrap();
    let invalid = call(&mut p, "check", json!({}));
    assert!(
        p.dispatch(
            "baseline_commit",
            &json!({"token":invalid["baseline_status"]["token"]}),
            &control()
        )
        .is_err()
    );
    assert!(previous.join("baseline.json").is_file());
    f.unchanged();
}

#[test]
fn own_applied_resource_never_becomes_an_update_or_a_replacement_baseline() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let saved = call(&mut p, "baseline", json!({}));
    let folder = PathBuf::from(saved["folder"].as_str().unwrap());
    let original = fs::read(folder.join(saved["resource"].as_str().unwrap())).unwrap();
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    let mut reopened = f.project();
    assert_eq!(
        call(&mut reopened, "check", json!({}))["baseline_status"]["state"],
        "ready"
    );
    assert_eq!(
        fs::read(folder.join(saved["resource"].as_str().unwrap())).unwrap(),
        original
    );
    storage::retire(&folder).unwrap();
    let blocked = call(&mut reopened, "check", json!({}));
    assert_eq!(blocked["baseline_status"]["state"], "blocked");
    assert!(
        reopened
            .dispatch(
                "baseline_commit",
                &json!({"token":blocked["baseline_status"]["token"]}),
                &control()
            )
            .is_err()
    );
    assert!(!folder.exists());
}

#[test]
fn external_same_version_content_can_only_replace_baseline_after_confirmation() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let before = call(&mut p, "baseline", json!({}));
    let folder = PathBuf::from(before["folder"].as_str().unwrap());
    let record = fs::read(folder.join("baseline.json")).unwrap();
    // A separately installed text resource with the same game's version marker.
    let generated = generate_fixture_patch(&mut p).unwrap();
    let external = PathBuf::from(generated["output"].as_str().unwrap());
    let bytes = fs::read(&external).unwrap();
    let crc = bajptr_core::codec::inspect(
        &external,
        &bajptr_core::codec::key(region::KEY).unwrap(),
        &control(),
    )
    .unwrap()
    .crc;
    let live = region::locate(f.game.to_str().unwrap()).unwrap();
    fs::copy(
        &external,
        f.game
            .join(region::BUNDLES)
            .join(external.file_name().unwrap()),
    )
    .unwrap();
    region::publish_catalog(&live, bytes.len() as u64, crc).unwrap();
    let mut reopened = f.project();
    let changed = call(&mut reopened, "check", json!({}));
    assert_eq!(changed["baseline_status"]["state"], "changed");
    assert_eq!(fs::read(folder.join("baseline.json")).unwrap(), record);
    assert!(
        reopened
            .dispatch("preview", &json!({}), &control())
            .is_err()
    );
    let newest = call(
        &mut reopened,
        "baseline_commit",
        json!({"token":changed["baseline_status"]["token"]}),
    );
    assert_eq!(
        fs::read(folder.join(newest["resource"].as_str().unwrap())).unwrap(),
        bytes
    );
    assert_ne!(fs::read(folder.join("baseline.json")).unwrap(), record);
    assert_eq!(
        call(&mut reopened, "check", json!({}))["baseline_status"]["state"],
        "ready"
    );
    let search = call(&mut reopened, "search", json!({"keyword":"委托"}));
    assert!(search["total"].as_u64().unwrap() > 0);
}

#[test]
fn search_trims_keyword_before_matching_and_recording_context() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let expected = call(&mut p, "search", json!({"keyword":"任务"}));
    assert!(expected["total"].as_u64().unwrap() > 0);
    let actual = call(&mut p, "search", json!({"keyword":" 任务 \t"}));
    assert_eq!(actual["keyword"], "任务");
    assert_eq!(actual["rows"], expected["rows"]);
    assert_eq!(actual["total"], expected["total"]);
}

#[test]
fn later_search_pages_remain_editable_without_rescanning_the_database() {
    let mut f = Fixture::new();
    let game = region::locate(f.game.to_str().unwrap()).unwrap();
    let plain = f.root.join("pagination.db");
    let encrypted = f.root.join("pagination.encrypted");
    let key = bajptr_core::codec::key(region::KEY).unwrap();
    bajptr_core::codec::decrypt(&game.bundle, &plain, &key, &control()).unwrap();
    let con = rusqlite::Connection::open(&plain).unwrap();
    con.execute_batch(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1100)
         INSERT INTO LocalizeDBSchema(Bytes)
         SELECT Bytes FROM LocalizeDBSchema, n WHERE rowid=1",
    )
    .unwrap();
    drop(con);
    let (_, inspection) =
        bajptr_core::codec::encrypt_checked(&plain, &encrypted, &key, &[0; 16], &[], &control())
            .unwrap();
    fs::copy(
        &encrypted,
        game.bundle
            .with_file_name(format!("123456_{}", inspection.crc)),
    )
    .unwrap();
    region::publish_catalog(
        &game,
        fs::metadata(&encrypted).unwrap().len(),
        inspection.crc,
    )
    .unwrap();
    f.before = fs::read(&game.catalog).unwrap();
    let mut p = f.ready_project();
    let first = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(first["rows"].as_array().unwrap().len(), 500);
    assert!(first["total"].as_u64().unwrap() > 1000);
    let no_scan = Control::new(&|message| !message.is_some_and(|m| m.starts_with("扫描文本表：")));
    let second = p
        .dispatch(
            "search_more",
            &json!({"keyword":"任务","offset":500}),
            &no_scan,
        )
        .unwrap();
    assert_eq!(second["rows"].as_array().unwrap().len(), 1000);
    assert_eq!(
        &second["rows"].as_array().unwrap()[..500],
        first["rows"].as_array().unwrap()
    );
    let third = p
        .dispatch(
            "search_more",
            &json!({"keyword":"任务","offset":1000}),
            &no_scan,
        )
        .unwrap();
    let opened = call(
        &mut p,
        "text_open",
        json!({"search_nonce":third["nonce"],"index":500}),
    );
    assert_eq!(opened["original"], third["rows"][500]["original"]);
    let saved = call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"分页编辑"}),
    );
    assert_eq!(saved["search"]["rows"][500]["text"], "分页编辑");
    let old = saved["search"]["rows"][999]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        saved["search"]["rows"][1000]["text"]
            .as_str()
            .unwrap()
            .contains(&old)
    );
    let preview = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":third["nonce"],"indices":[999,1000],"old":old,"new":"分页批量"}),
    );
    assert_eq!(preview["changes"].as_array().unwrap().len(), 2);
    let saved = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    for i in [999, 1000] {
        assert!(
            saved["search"]["rows"][i]["text"]
                .as_str()
                .unwrap()
                .contains("分页批量")
        );
    }
    assert_eq!(saved["search"]["rows"][500]["text"], "分页编辑");
    f.unchanged();
}

#[test]
fn data_directory_rejects_gui_contents_without_creating_nested_storage() {
    let f = Fixture::new();
    for data in [f.data.join(".gui"), f.root.join("copied-gui-contents")] {
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("settings.json"), b"{}").unwrap();
        let mut p = Project::new(data.clone());
        let error = p
            .dispatch("load", &json!({}), &control())
            .unwrap_err()
            .to_string();
        assert!(error.contains("--data-dir"));
        assert!(error.contains("父目录"));
        assert!(!data.join(".gui").exists());
        assert!(Worker::start(data, Arc::new(|| {})).is_err());
    }
    let empty = f.root.join("new-data");
    let mut p = Project::new(empty);
    assert!(p.dispatch("load", &json!({}), &control()).is_ok());
}

#[test]
fn search_source_choices_follow_completed_keyword_and_survive_paging() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let loaded = call(&mut p, "load", json!({}));
    assert!(loaded["search"].is_null());
    let tables = call(&mut p, "tables", json!({}));
    assert_eq!(tables.as_array().unwrap().len(), 1);
    let search = call(
        &mut p,
        "search",
        json!({"keyword":"任务","source":"LocalizeDBSchema"}),
    );
    assert_eq!(search["source_tables"][0]["name"], "LocalizeDBSchema");
    assert_eq!(search["source_tables"][0]["label"], "界面与系统提示");
    let page = call(
        &mut p,
        "search_more",
        json!({"keyword":"任务","source":"LocalizeDBSchema","offset":search["rows"].as_array().unwrap().len()}),
    );
    assert_eq!(page["source_tables"], search["source_tables"]);
    let empty = call(&mut p, "search", json!({"keyword":"没有命中的新词"}));
    assert_eq!(empty["source_tables"], json!([]));
    assert_eq!(empty["total"], 0);
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["source_tables"].as_array().unwrap().len(), 1);
    let revision = p.revision;
    let settings = call(
        &mut p,
        "settings",
        json!({"game":f.game,"skip_kana":false,"revision":revision}),
    );
    assert!(settings["search"].is_null());
    let unskipped = call(&mut p, "search", json!({"keyword":"アロナ"}));
    assert_eq!(unskipped["source_tables"].as_array().unwrap().len(), 1);
    let revision = p.revision;
    let settings = call(
        &mut p,
        "settings",
        json!({"game":f.game,"skip_kana":true,"revision":revision}),
    );
    assert!(settings["search"].is_null());
    let skipped = call(&mut p, "search", json!({"keyword":"アロナ"}));
    assert_eq!(skipped["source_tables"], json!([]));
    f.unchanged();
}

#[test]
fn source_filter_keeps_other_matching_tables_available() {
    // Extend only this disposable game, before capturing its initial baseline.
    let mut f = Fixture::new();
    let game = region::locate(f.game.to_str().unwrap()).unwrap();
    let plain = f.root.join("sources.db");
    let encrypted = f.root.join("sources.encrypted");
    let key = bajptr_core::codec::key(region::KEY).unwrap();
    bajptr_core::codec::decrypt(&game.bundle, &plain, &key, &control()).unwrap();
    let con = rusqlite::Connection::open(&plain).unwrap();
    con.execute_batch(
        "CREATE TABLE LocalizeSkillDBSchema(Bytes BLOB);
        INSERT INTO LocalizeSkillDBSchema SELECT Bytes FROM LocalizeDBSchema WHERE rowid=1;
        CREATE TABLE LocalizeEmptyDBSchema(Bytes BLOB)",
    )
    .unwrap();
    drop(con);
    let (_, inspection) =
        bajptr_core::codec::encrypt_checked(&plain, &encrypted, &key, &[0; 16], &[], &control())
            .unwrap();
    fs::copy(
        &encrypted,
        game.bundle
            .with_file_name(format!("123456_{}", inspection.crc)),
    )
    .unwrap();
    region::publish_catalog(
        &game,
        fs::metadata(&encrypted).unwrap().len(),
        inspection.crc,
    )
    .unwrap();
    f.before = fs::read(&game.catalog).unwrap();
    let mut p = f.ready_project();
    assert_eq!(
        call(&mut p, "tables", json!({})).as_array().unwrap().len(),
        3
    );
    for source in ["LocalizeSkillDBSchema", "LocalizeDBSchema", ""] {
        let search = call(&mut p, "search", json!({"keyword":"任务","source":source}));
        let names: Vec<_> = search["source_tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["LocalizeDBSchema", "LocalizeSkillDBSchema"]);
        if !source.is_empty() {
            assert!(
                search["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["table"] == source)
            );
        }
    }
    let search = call(
        &mut p,
        "search",
        json!({"keyword":"老师","source":"LocalizeSkillDBSchema"}),
    );
    assert_eq!(search["total"], 0);
    assert_eq!(search["source_tables"].as_array().unwrap().len(), 1);
    assert_eq!(search["source_tables"][0]["name"], "LocalizeDBSchema");
    let before = p.snapshot();
    for (old, mode, expected) in [
        ("", "sub", 3),
        ("任务", "sub", 2),
        ("任务", "exact", 0),
        ("阿罗娜的任务", "exact", 2),
        ("^阿罗娜.*任务$", "re", 2),
        ("老师", "sub", 1),
        ("不存在", "sub", 0),
        ("アロナ", "sub", 0),
    ] {
        let result = call(&mut p, "scope_sources", json!({"old":old,"mode":mode}));
        assert_eq!(result.as_array().unwrap().len(), expected, "{old}/{mode}");
        assert_eq!(
            p.snapshot(),
            before,
            "candidate lookup changed project state"
        );
    }
    assert!(
        p.dispatch("scope_sources", &json!({"old":"(","mode":"re"}), &control())
            .is_err()
    );
    let revision = p.revision;
    call(
        &mut p,
        "settings",
        json!({"revision":revision,"game":f.game,"skip_kana":false}),
    );
    assert_eq!(
        call(
            &mut p,
            "scope_sources",
            json!({"old":"アロナ","mode":"sub"})
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    let revision = p.revision;
    call(
        &mut p,
        "settings",
        json!({"revision":revision,"game":f.game,"skip_kana":true}),
    );
    assert_eq!(
        call(
            &mut p,
            "scope_sources",
            json!({"old":"アロナ","mode":"sub"})
        ),
        json!([])
    );
    f.unchanged();
}

#[test]
fn current_documents_preserve_verified_replacement_behavior() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let expected = oracle();
    assert_eq!(
        region::version(&f.game.join(region::CATALOG)).unwrap(),
        expected["version"]
    );
    let preview = call(&mut p, "preview", json!({}));
    for (key, value) in expected["preview"].as_object().unwrap() {
        assert_eq!(&preview["stats"][key], value, "{key}");
    }
    for example in expected["templates"].as_array().unwrap() {
        let value = call(&mut p, "template", example.clone());
        assert_eq!(value["old"], example["old"]);
        assert_eq!(value["new"], example["new"]);
    }
    f.unchanged();
}
#[test]
fn whole_wordlist_regenerates_from_one_immutable_baseline_and_restore_preserves_documents() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    let applied = call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    assert_eq!(applied["installed"], true);
    assert!(!f.data.join("output").exists());
    assert_ne!(fs::read(f.game.join(region::CATALOG)).unwrap(), f.before);
    let unchanged = call(&mut p, "preview", json!({}));
    assert_eq!(unchanged["total_changes"], 0);
    assert_eq!(unchanged["application"], "当前游戏文字与词表一致");
    assert_eq!(p.application, "当前游戏文字与词表一致");
    let revision = p.revision;
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"阿罗娜","new":"第二次名字","mode":"sub","source":"测试","table":""}}),
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(preview["stats"]["updated"], 1);
    assert_eq!(preview["application"], "词表已保存，尚未应用");
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    let state = call(&mut p, "check", json!({}));
    assert_eq!(state["application"], "已应用，与当前词表一致");
    let rules = fs::read(f.data.join(".gui/rules.tsv")).unwrap();
    call(&mut p, "restore", json!({}));
    f.unchanged();
    assert_eq!(fs::read(f.data.join(".gui/rules.tsv")).unwrap(), rules);
    let baselines = walkdir::WalkDir::new(f.data.join("backup"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|p| p.file_name() == "baseline.json")
        .count();
    assert_eq!(baselines, 1);
}
#[test]
fn single_and_batch_edits_bind_original_search_and_share_undo_history() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    call(&mut p, "search", json!({"keyword":"老师"}));
    let opened = call(
        &mut p,
        "text_open",
        json!({"index":0,"search_nonce":search["nonce"]}),
    );
    assert_eq!(opened["text"], "阿洛娜同学的委托");
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"只改这段\n下一行"}),
    );
    assert_eq!(p.edits[0].original, "阿罗娜的任务");
    call(&mut p, "history", json!({"undo":true}));
    assert!(p.edits.is_empty());
    call(&mut p, "history", json!({"undo":false}));
    assert_eq!(p.edits.len(), 1);
    call(&mut p, "history", json!({"undo":true}));
    let batch = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"委托","new":"爆炸"}),
    );
    assert_eq!(batch["changes"].as_array().unwrap().len(), 1);
    call(&mut p, "batch_save", json!({"nonce":batch["nonce"]}));
    assert!(
        p.dispatch("batch_save", &json!({"nonce":batch["nonce"]}), &control())
            .is_err()
    );
    assert_eq!(p.edits.len(), 1);
    let reopened = f.project();
    assert_eq!(reopened.edits, p.edits);
    f.unchanged();
}
#[test]
fn batch_preserves_the_single_edit_and_removing_batch_restores_it() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let opened = call(
        &mut p,
        "text_open",
        json!({"index":0,"search_nonce":search["nonce"]}),
    );
    let single = "阿洛娜同学的委托【单条】";
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":single}),
    );
    let draft = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"委托","new":"神秘"}),
    );
    let state = call(&mut p, "batch_save", json!({"nonce":draft["nonce"]}));
    assert_eq!(p.edits.iter().filter(|e| !e.batch).count(), 1);
    assert_eq!(p.edits.iter().filter(|e| e.batch).count(), 1);
    let id = state["batches"][0]["id"].clone();
    let single_index = p.edits.iter().position(|e| !e.batch).unwrap();
    let opened = call(&mut p, "text_open", json!({"edit_index":single_index}));
    assert_eq!(opened["text"], single);
    assert_eq!(opened["has_batch"], true);
    assert_eq!(
        opened["batch_operation"],
        json!({"old":"委托","new":"神秘"})
    );
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"阿洛娜同学的委托【调整单条】"}),
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(
        preview["stats"]["changes"][0]["after"],
        "阿洛娜同学的神秘【调整单条】"
    );
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    let mut p = f.ready_project();
    assert_eq!(p.edits.len(), 2);
    let revision = p.revision;
    call(&mut p, "remove_batch", json!({"id":id,"revision":revision}));
    assert_eq!(p.edits.len(), 1);
    assert!(!p.edits[0].batch);
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["rows"][0]["text"], "阿洛娜同学的委托【调整单条】");
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits.len(), 2);
    assert_eq!(call(&mut p, "preview", json!({}))["total_changes"], 0);
}

#[test]
fn removing_single_keeps_batch_and_search_editor_does_not_apply_it_twice() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let draft = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"委托","new":"委托委托"}),
    );
    call(&mut p, "batch_save", json!({"nonce":draft["nonce"]}));
    let opened = call(
        &mut p,
        "text_open",
        json!({"index":0,"search_nonce":search["nonce"]}),
    );
    assert_eq!(opened["text"], "阿洛娜同学的委托");
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"阿洛娜同学的委托【单条】"}),
    );
    assert_eq!(p.edits.len(), 2);
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["rows"][0]["text"], "阿洛娜同学的委托委托【单条】");
    let i = p.edits.iter().position(|e| !e.batch).unwrap();
    let revision = p.revision;
    call(
        &mut p,
        "remove_edit",
        json!({"index":i,"revision":revision}),
    );
    assert_eq!(p.edits.len(), 1);
    assert!(p.edits[0].batch);
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["rows"][0]["text"], "阿洛娜同学的委托委托");
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits.len(), 2);
    assert_eq!(call(&mut p, "preview", json!({}))["total_changes"], 1);
    f.unchanged();
}

#[test]
fn removing_one_batch_member_preserves_other_members_and_single_edits() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let opened = call(
        &mut p,
        "text_open",
        json!({"search_nonce":search["nonce"],"index":0}),
    );
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"阿洛娜同学的委托【保留单条】"}),
    );
    let draft = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0,1],"old":"委托","new":"神奇"}),
    );
    let saved = call(&mut p, "batch_save", json!({"nonce":draft["nonce"]}));
    let id = saved["batches"][0]["id"].clone();
    let other = call(&mut p, "search", json!({"keyword":"老师"}));
    let draft = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":other["nonce"],"indices":[0],"old":"老师","new":"教员"}),
    );
    call(&mut p, "batch_save", json!({"nonce":draft["nonce"]}));
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    let target = p
        .edits
        .iter()
        .position(|e| e.batch && e.row_id == bajptr_core::database::RowId::Integer(1))
        .unwrap();
    let single = p.edits.iter().position(|e| !e.batch).unwrap();
    let snapshot = p.edits.clone();
    for payload in [
        json!({"id":id,"index":target}),
        json!({"id":"wrong","index":target,"revision":p.revision}),
        json!({"id":id,"index":single,"revision":p.revision}),
        json!({"id":id,"index":999,"revision":p.revision}),
    ] {
        assert!(
            p.dispatch("remove_batch_member", &payload, &control())
                .is_err()
        );
        assert_eq!(p.edits, snapshot);
    }
    let revision = p.revision;
    let saved = call(
        &mut p,
        "remove_batch_member",
        json!({"id":id,"index":target,"revision":revision}),
    );
    assert_eq!(p.edits.len(), 3);
    assert_eq!(saved["batches"].as_array().unwrap().len(), 2);
    assert_eq!(saved["batches"][0]["count"], 1);
    assert!(
        p.edits
            .iter()
            .any(|e| !e.batch && e.replacement.contains("保留单条"))
    );
    assert!(
        p.dispatch(
            "remove_batch_member",
            &json!({"id":id,"index":target,"revision":revision}),
            &control()
        )
        .is_err()
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(preview["total_changes"], 1);
    assert_eq!(
        preview["stats"]["changes"][0]["after"],
        "阿洛娜同学的委托【保留单条】"
    );
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits, snapshot);
    assert_eq!(call(&mut p, "preview", json!({}))["total_changes"], 0);
    call(&mut p, "history", json!({"undo":false}));
    let mut reopened = f.ready_project();
    assert_eq!(reopened.edits, p.edits);
    let last = reopened
        .edits
        .iter()
        .position(|e| {
            e.batch_operation
                .as_ref()
                .is_some_and(|b| json!(b.id) == id)
        })
        .unwrap();
    let revision = reopened.revision;
    let state = call(
        &mut reopened,
        "remove_batch_member",
        json!({"id":id,"index":last,"revision":revision}),
    );
    assert_eq!(state["batches"].as_array().unwrap().len(), 1);
    assert_eq!(reopened.edits.iter().filter(|e| !e.batch).count(), 1);
}

#[test]
fn one_batch_operation_is_one_rule_row_with_its_original_selected_scope() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    call(
        &mut p,
        "save_rule",
        json!({"revision":0,"index":1,"rule":{"old":"任务","new":"神秘","mode":"sub","source":"测试","table":""}}),
    );
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    assert_eq!(search["rows"].as_array().unwrap().len(), 2);
    let preview = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0,1],"old":"神秘","new":"神奇"}),
    );
    let saved = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    assert_eq!(p.edits.len(), 2);
    let batches = saved["batches"].as_array().unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0]["old"], "神秘");
    assert_eq!(batches[0]["new"], "神奇");
    assert_eq!(batches[0]["count"], 2);
    let id = batches[0]["id"].clone();
    let targets = p
        .edits
        .iter()
        .map(|e| (e.table.clone(), e.row_id.clone(), e.slot))
        .collect::<Vec<_>>();
    let opened = call(&mut p, "batch_open", json!({"id":id}));
    assert_eq!(opened["old"], "神秘");
    assert_eq!(opened["new"], "神奇");
    let revised = call(
        &mut p,
        "batch_preview",
        json!({"group_id":id,"indices":[0,1],"old":"神秘","new":"奇妙"}),
    );
    let saved = call(&mut p, "batch_save", json!({"nonce":revised["nonce"]}));
    assert_eq!(saved["batches"][0]["id"], id);
    assert_eq!(saved["batches"][0]["new"], "奇妙");
    assert_eq!(
        p.edits
            .iter()
            .map(|e| (e.table.clone(), e.row_id.clone(), e.slot))
            .collect::<Vec<_>>(),
        targets
    );
    assert!(
        p.edits
            .iter()
            .all(|e| e.replacement.contains("奇妙") && !e.replacement.contains("神奇"))
    );
    let reopened = f.project();
    assert_eq!(reopened.state().unwrap()["batches"][0]["new"], "奇妙");
    let revision = p.revision;
    call(&mut p, "remove_batch", json!({"id":id,"revision":revision}));
    assert!(p.edits.is_empty());
    call(&mut p, "history", json!({"undo":true}));
    assert_eq!(p.edits.len(), 2);
    assert_eq!(p.state().unwrap()["batches"].as_array().unwrap().len(), 1);
    f.unchanged();
}

#[test]
fn repeated_batches_merge_matching_replacements_and_share_history() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    call(
        &mut p,
        "save_rule",
        json!({"revision":0,"index":1,"rule":{"old":"任务","new":"神秘","mode":"sub","source":"测试","table":""}}),
    );
    let mut id = Value::Null;
    for index in 0..2 {
        let search = call(&mut p, "search", json!({"keyword":"任务"}));
        let preview = call(
            &mut p,
            "batch_preview",
            json!({"search_nonce":search["nonce"],"indices":[index],"old":"神秘","new":"神奇"}),
        );
        assert_eq!(preview["changes"].as_array().unwrap().len(), 1);
        // Previewing the next selection must not add it before Save.
        assert_eq!(p.edits.len(), index);
        let saved = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
        assert_eq!(saved["batches"].as_array().unwrap().len(), 1);
        assert_eq!(saved["batches"][0]["count"], index + 1);
        if index == 0 {
            id = saved["batches"][0]["id"].clone();
        } else {
            assert_eq!(saved["batches"][0]["id"], id);
        }
    }
    let saved = p.edits.clone();
    assert_eq!(f.project().edits, saved);
    let undone = call(&mut p, "history", json!({"undo":true}));
    assert_eq!(undone["batches"][0]["count"], 1);
    assert_eq!(p.edits.len(), 1);
    let redone = call(&mut p, "history", json!({"undo":false}));
    assert_eq!(redone["batches"][0]["count"], 2);
    assert_eq!(p.edits, saved);
    let opened = call(&mut p, "batch_open", json!({"id":id}));
    assert_eq!(opened["count"], 2);
    let edited = call(
        &mut p,
        "batch_preview",
        json!({"group_id":id,"indices":[0,1],"old":"神秘","new":"奇妙"}),
    );
    call(&mut p, "batch_save", json!({"nonce":edited["nonce"]}));
    assert!(p.edits.iter().all(|e| e.replacement.contains("奇妙")));
    let revision = p.revision;
    call(&mut p, "remove_batch", json!({"id":id,"revision":revision}));
    assert!(p.edits.is_empty());
    f.unchanged();
}

#[test]
fn overlapping_batch_selections_are_a_union_without_replacing_twice() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    for indices in [json!([0]), json!([0, 1])] {
        let search = call(&mut p, "search", json!({"keyword":"任务"}));
        let preview = call(
            &mut p,
            "batch_preview",
            json!({"search_nonce":search["nonce"],"indices":indices,"old":"委托","new":"委托委托"}),
        );
        assert_eq!(preview["changes"].as_array().unwrap().len(), 1);
        let saved = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
        assert_eq!(saved["batches"].as_array().unwrap().len(), 1);
    }
    assert_eq!(p.edits.len(), 2);
    assert_eq!(p.state().unwrap()["batches"][0]["count"], 2);
    for edit in &p.edits {
        let operation = edit.batch_operation.as_ref().unwrap();
        assert_eq!(
            edit.replacement,
            operation.before.replace("委托", "委托委托")
        );
        assert_eq!(edit.replacement.matches("委托").count(), 2);
    }
    let before = p.edits.clone();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let duplicate = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0,1],"old":"委托","new":"委托委托"}),
    );
    assert!(duplicate["changes"].as_array().unwrap().is_empty());
    assert_eq!(p.edits, before);
    f.unchanged();
}

#[test]
fn batch_merge_preserves_other_games_versions_and_unclassified_legacy_entries() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let preview = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"委托","new":"神奇"}),
    );
    call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    let mut inactive = p.edits[0].clone();
    inactive.version = "0".repeat(64);
    inactive.batch_operation.as_mut().unwrap().id = "a".repeat(32);
    let mut foreign = p.edits[0].clone();
    foreign.game = "another-game".into();
    foreign.batch_operation.as_mut().unwrap().id = "b".repeat(32);
    p.edits[0].batch_operation = None;
    p.edits.extend([inactive, foreign]);
    let preserved = p.edits.clone();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let preview = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[1],"old":"委托","new":"神奇"}),
    );
    let saved = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    assert_eq!(p.edits.len(), 4);
    assert_eq!(&p.edits[..3], preserved);
    assert_eq!(saved["batches"].as_array().unwrap().len(), 3);
    assert!(
        saved["batches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["count"] == 1)
    );
    assert_eq!(f.project().edits, p.edits);
    f.unchanged();
}

#[test]
fn different_batch_replacements_remain_separate_until_edited_to_match() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    p.skip = false;
    for (index, new) in ["甲", "乙"].iter().enumerate() {
        let search = call(&mut p, "search", json!({"keyword":"任务"}));
        let preview = call(
            &mut p,
            "batch_preview",
            json!({"search_nonce":search["nonce"],"indices":[index],"old":"委托","new":new}),
        );
        call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    }
    let state = p.state().unwrap();
    assert_eq!(state["batches"].as_array().unwrap().len(), 2);
    let id = state["batches"][0]["id"].clone();
    let other_id = state["batches"][1]["id"].clone();
    let preview = call(
        &mut p,
        "batch_preview",
        json!({"group_id":other_id,"indices":[0],"old":"委托","new":"甲"}),
    );
    let merged = call(&mut p, "batch_save", json!({"nonce":preview["nonce"]}));
    assert_eq!(merged["batches"].as_array().unwrap().len(), 1);
    assert_eq!(merged["batches"][0]["id"], id);
    assert_eq!(merged["batches"][0]["count"], 2);
    assert!(p.edits.iter().all(|e| e.replacement.contains('甲')));
    f.unchanged();
}

#[test]
fn editing_a_saved_batch_entry_preserves_its_kind_and_target() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let search = call(&mut p, "search", json!({"keyword":"任务"}));
    let batch = call(
        &mut p,
        "batch_preview",
        json!({"search_nonce":search["nonce"],"indices":[0],"old":"委托","new":"批量结果"}),
    );
    call(&mut p, "batch_save", json!({"nonce":batch["nonce"]}));
    let target = p.edits[0].clone();
    assert!(target.batch);
    let opened = call(&mut p, "text_open", json!({"edit_index":0}));
    call(
        &mut p,
        "text_save",
        json!({"nonce":opened["nonce"],"text":"重新编辑的批量结果"}),
    );
    assert_eq!(p.edits.len(), 1);
    assert!(p.edits[0].batch);
    assert!(p.edits[0].same_address(&target));
    assert_eq!(p.edits[0].original, target.original);
    assert_eq!(f.project().edits, p.edits);
    let revision = p.revision;
    call(
        &mut p,
        "remove_edit",
        json!({"index":0,"revision":revision}),
    );
    assert!(p.edits.is_empty());
    call(&mut p, "history", json!({"undo":true}));
    assert!(p.edits[0].batch);
    f.unchanged();
}

#[test]
fn broken_or_externally_changed_wordlist_is_never_overwritten() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let path = f.data.join(".gui/rules.tsv");
    fs::write(&path, "外部内容\t新内容\n").unwrap();
    assert!(p.dispatch("save_rule",&json!({"index":0,"revision":0,"rule":{"old":"阿罗娜","new":"新名字","mode":"sub","table":""}}),&control()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "外部内容\t新内容\n");
    fs::write(&path, "坏文件").unwrap();
    let mut broken = f.project();
    assert!(broken.dispatch("preview", &json!({}), &control()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "坏文件");
    f.unchanged();
}
#[test]
fn corrupt_existing_restore_target_is_rejected_and_catalog_is_preserved() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    let before = fs::read(f.game.join(region::CATALOG)).unwrap();
    let original = region::parse(&f.before).unwrap();
    let target = f
        .game
        .join(region::BUNDLES)
        .join(format!("123456_{}", original.crc));
    let mut b = fs::read(&target).unwrap();
    b[4000] ^= 1;
    fs::write(&target, b).unwrap();
    assert!(p.dispatch("restore", &json!({}), &control()).is_err());
    assert_eq!(fs::read(f.game.join(region::CATALOG)).unwrap(), before);
}
#[test]
fn stale_preview_and_incomplete_baseline_cannot_be_applied_or_recreated() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let preview = call(&mut p, "preview", json!({}));
    call(
        &mut p,
        "save_rule",
        json!({"revision":0,"index":0,"rule":{"old":"阿罗娜","new":"新名字","mode":"sub","source":"测试","table":""}}),
    );
    assert!(
        p.dispatch("apply", &json!({"nonce":preview["nonce"]}), &control())
            .is_err()
    );
    let baseline = call(&mut p, "baseline", json!({}));
    let record = PathBuf::from(baseline["folder"].as_str().unwrap()).join("baseline.json");
    storage::retire(&record).unwrap();
    assert!(p.dispatch("preview", &json!({}), &control()).is_err());
    f.unchanged();
}
fn result(worker: &Worker, id: u64) -> Value {
    loop {
        let message = worker.events.recv_timeout(Duration::from_secs(20)).unwrap();
        if message["id"] == id && message["event"] != "progress" {
            assert_eq!(message["event"], "result", "{message}");
            return message["data"].clone();
        }
    }
}
#[test]
fn late_cancel_rebases_later_background_snapshots_and_accept_releases_results() {
    let f = Fixture::new();
    drop(f.ready_project());
    let worker = Worker::start(f.data.clone(), Arc::new(|| {})).unwrap();
    worker.send(1, "load", json!({})).unwrap();
    result(&worker, 1);
    worker.send(2, "search", json!({"keyword":"任务"})).unwrap();
    let first = result(&worker, 2);
    worker.send(3, "search", json!({"keyword":"老师"})).unwrap();
    let second = result(&worker, 3);
    assert_ne!(first["nonce"], second["nonce"]);
    worker.send(4, "tables", json!({})).unwrap();
    result(&worker, 4);
    worker.send(5, "cancel", json!({"target":3})).unwrap();
    worker
        .send(
            6,
            "settings",
            json!({"game":f.game,"skip_kana":true,"revision":0}),
        )
        .unwrap();
    assert_eq!(result(&worker, 6)["search"]["nonce"], first["nonce"]);
    worker.send(7, "cancel", json!({"target":4})).unwrap();
    worker
        .send(
            8,
            "settings",
            json!({"game":f.game,"skip_kana":true,"revision":0}),
        )
        .unwrap();
    assert_eq!(result(&worker, 8)["search"]["nonce"], first["nonce"]);
    worker.send(9, "search", json!({"keyword":"老师"})).unwrap();
    let accepted = result(&worker, 9);
    worker.send(10, "accept", json!({"target":9})).unwrap();
    worker.send(11, "cancel", json!({"target":9})).unwrap();
    worker
        .send(
            12,
            "settings",
            json!({"game":f.game,"skip_kana":true,"revision":0}),
        )
        .unwrap();
    assert_eq!(result(&worker, 12)["search"]["nonce"], accepted["nonce"]);
}
#[test]
fn cancelling_apply_during_encryption_leaves_game_unchanged_and_preview_reusable() {
    let f = Fixture::new();
    let mut project = f.ready_project();
    let preview = call(&mut project, "preview", json!({}));
    let cancelled = project.dispatch(
        "apply",
        &json!({"nonce":preview["nonce"]}),
        &Control::new(&|message| !message.is_some_and(|m| m.starts_with("正在加密数据库："))),
    );
    assert!(matches!(cancelled, Err(bajptr_project::Error::Cancelled)));
    assert!(!f.data.join("output").exists());
    f.unchanged();
    let applied = call(&mut project, "apply", json!({"nonce":preview["nonce"]}));
    assert_eq!(applied["application"], "已应用，与当前词表一致");
}

#[test]
fn late_cancel_after_apply_never_rewinds_committed_state() {
    let f = Fixture::new();
    drop(f.ready_project());
    let worker = Worker::start(f.data.clone(), Arc::new(|| {})).unwrap();
    worker.send(1, "load", json!({})).unwrap();
    result(&worker, 1);
    worker.send(2, "preview", json!({})).unwrap();
    let preview = result(&worker, 2);
    worker.send(3, "accept", json!({"target":2})).unwrap();
    worker
        .send(4, "apply", json!({"nonce":preview["nonce"]}))
        .unwrap();
    let applied = result(&worker, 4);
    assert_eq!(applied["application"], "已应用，与当前词表一致");
    worker.send(5, "cancel", json!({"target":4})).unwrap();
    worker
        .send(
            6,
            "settings",
            json!({"game":f.game,"skip_kana":true,"revision":0}),
        )
        .unwrap();
    assert_eq!(result(&worker, 6)["application"], applied["application"]);
    assert_ne!(fs::read(f.game.join(region::CATALOG)).unwrap(), f.before);
}

#[test]
fn file_locks_refuse_a_second_writer() {
    let f = Fixture::new();
    let path = f.data.join(".gui/rules.tsv");
    let lock = storage::Lock::acquire(&path).unwrap();
    assert!(storage::Lock::acquire(&path).is_err());
    drop(lock);
    assert!(storage::Lock::acquire(&path).is_ok());
}

#[test]
fn zero_hit_feedback_keeps_the_existing_desktop_row_contract() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    call(
        &mut p,
        "save_rule",
        json!({"revision":0,"index":null,"rule":{"old":"任务","new":"委托","mode":"exact","table":"","source":"测试"}}),
    );
    let preview = call(&mut p, "preview", json!({}));
    let reasons = preview["stats"]["zero_reasons"].as_array().unwrap();
    assert_eq!(reasons[0][0], 2);
    assert!(reasons[0][1].as_str().unwrap().contains("整段相同"));
    f.unchanged();
}

#[test]
fn missing_original_resource_is_republished_from_the_verified_baseline() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let original = region::locate(f.game.to_str().unwrap()).unwrap().bundle;
    let preview = call(&mut p, "preview", json!({}));
    call(&mut p, "apply", json!({"nonce":preview["nonce"]}));
    storage::retire(&original).unwrap();
    assert!(!original.exists());
    call(&mut p, "restore", json!({}));
    assert!(original.is_file());
    f.unchanged();
}

#[cfg(windows)]
#[test]
fn unavailable_drive_in_settings_still_allows_reselecting_the_game() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let value = call(
        &mut p,
        "settings",
        json!({"revision":0,"game":r"Z:\bajptr-unavailable-game","skip_kana":true}),
    );
    assert_eq!(value["game"], r"Z:\bajptr-unavailable-game");
    assert!(p.dispatch("check", &json!({}), &control()).is_err());
    let reopened = f.project();
    assert_eq!(reopened.game, r"Z:\bajptr-unavailable-game");
    let revision = p.revision;
    call(
        &mut p,
        "settings",
        json!({"revision":revision,"game":f.game,"skip_kana":true}),
    );
    assert_eq!(call(&mut p, "check", json!({}))["connected"], true);
    f.unchanged();
}

#[test]
fn prior_baseline_format_is_rejected_without_rebuilding_from_live_resources() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    let baseline = call(&mut p, "baseline", json!({}));
    let record = PathBuf::from(baseline["folder"].as_str().unwrap()).join("baseline.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    value["format"] = json!(1);
    fs::write(&record, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(p.dispatch("preview", &json!({}), &control()).is_err());
    let unchanged: Value = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(unchanged["format"], 1);
    f.unchanged();
}

#[test]
fn invalid_regex_output_reports_its_location_and_never_publishes_a_patch() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    call(
        &mut p,
        "save_rule",
        json!({"revision":0,"index":0,"rule":{"old":"阿罗娜","new":"\\t","mode":"re","source":"回归","table":""}}),
    );
    let errors = [
        p.dispatch("preview", &json!({}), &control()).unwrap_err(),
        generate_fixture_patch(&mut p).unwrap_err(),
    ];
    for error in errors {
        let error = error.to_string();
        for detail in [
            "LocalizeDBSchema",
            "记录",
            "字段",
            "第 1 条词条",
            "控制字符",
        ] {
            assert!(error.contains(detail), "{error}");
        }
        assert!(
            !f.data.join("output").exists(),
            "a failed rule must not publish output"
        );
        f.unchanged();
    }
}
