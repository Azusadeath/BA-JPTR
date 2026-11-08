use bajptr_project::{Control, desktop::Project, region, storage, worker::Worker};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn control() -> Control<'static> {
    Control {
        callback: &|m| {
            if let Some(m) = m {
                eprintln!("{m}");
            }
            true
        },
    }
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
fn desktop_has_no_standalone_patch_generation_action() {
    let f = Fixture::new();
    let mut p = f.ready_project();
    assert!(p.dispatch("generate", &json!({}), &control()).is_err());
    assert!(!f.data.join("output").exists());
    f.unchanged();
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
fn failed_game_write_saves_a_verified_patch_and_allows_retry() {
    let f = Fixture::new();
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
    assert_eq!(call(&mut p, "preview", json!({}))["total_changes"], 0);
    let revision = p.revision;
    call(
        &mut p,
        "save_rule",
        json!({"revision":revision,"index":0,"rule":{"old":"阿罗娜","new":"第二次名字","mode":"sub","source":"测试","table":""}}),
    );
    let preview = call(&mut p, "preview", json!({}));
    assert_eq!(preview["stats"]["updated"], 1);
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
        &Control {
            callback: &|message| !message.is_some_and(|m| m.starts_with("正在加密数据库：")),
        },
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
