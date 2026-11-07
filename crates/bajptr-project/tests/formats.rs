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
