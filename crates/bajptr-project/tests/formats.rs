use bajptr_project::{model, storage, templates::Templates};
use serde_json::json;
#[test]
fn edit_documents_have_named_fields_and_reject_positional_rows() {
    let edit = model::Edit {
        game: "game".into(),
        version: "a".repeat(64),
        table: "LocalizeDBSchema".into(),
        row_id: bajptr_core::database::RowId::Integer(1),
        slot: 0,
        original: "任务".into(),
        replacement: "委托".into(),
        batch: false,
        batch_operation: None,
    };
    let bytes = model::edits_bytes(std::slice::from_ref(&edit)).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["format"], 2);
    assert_eq!(value["edits"][0]["original"], "任务");
    assert_eq!(
        model::parse_edits(&bytes).unwrap().as_slice(),
        std::slice::from_ref(&edit)
    );
    assert!(
        model::parse_edits(
            &serde_json::to_vec(&json!({"format":1,"edits":[edit.array()]})).unwrap()
        )
        .is_err()
    );
    assert!(
        model::parse_edits(
            &serde_json::to_vec(&json!({"format":2,"edits":[edit.array()]})).unwrap()
        )
        .is_err()
    );
}
#[test]
fn single_and_batch_pair_round_trips_but_same_kind_duplicates_are_rejected() {
    let single = model::Edit {
        game: "game".into(),
        version: "a".repeat(64),
        table: "LocalizeDBSchema".into(),
        row_id: bajptr_core::database::RowId::Integer(1),
        slot: 0,
        original: "任务".into(),
        replacement: "私人任务".into(),
        batch: false,
        batch_operation: None,
    };
    let mut batch = single.clone();
    batch.batch = true;
    batch.replacement = "私人委托".into();
    batch.batch_operation = Some(model::BatchOperation {
        id: "b".repeat(32),
        old: "任务".into(),
        new: "委托".into(),
        before: single.replacement.clone(),
    });
    let pair = vec![single.clone(), batch.clone()];
    assert_eq!(
        model::parse_edits(&model::edits_bytes(&pair).unwrap()).unwrap(),
        pair
    );
    let resolved = model::final_edits(&pair, "game", &"a".repeat(64)).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].replacement, "私人委托");
    assert!(
        model::final_edits(&[single.clone(), single.clone()], "game", &"a".repeat(64)).is_err()
    );
    assert!(model::validate_edits(&[batch.clone(), batch.clone()]).is_err());
    batch.original = "其他原文".into();
    assert!(model::validate_edits(&[single.clone(), batch.clone()]).is_err());
    batch.original = single.original.clone();
    batch.batch_operation = None;
    assert!(model::validate_edits(&[single, batch]).is_err());
}

#[test]
fn wordlists_require_the_single_five_column_format() {
    assert!(model::parse_rules("任务\t委托\n".as_bytes()).is_err());
    assert!(model::parse_rules("任务\t委托\tsub\t备注\t\n".as_bytes()).is_ok());
}
#[test]
fn template_documents_round_trip_with_the_current_format() {
    let root = std::env::temp_dir().join(format!(
        "bajptr-format-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("templates.json");
    let mut store = Templates::new(path.clone());
    let draft = json!({"key":"prefix","values":["任务","委托"],"sample":"任务开始"});
    store.remember(r"\A任务", "委托", &draft).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["format"], 2);
    assert!(value["drafts"].is_object());
    assert_eq!(Templates::new(path).lookup(r"\A任务", "委托"), Some(draft));
    storage::retire(&root).unwrap();
}
