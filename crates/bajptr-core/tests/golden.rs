use bajptr_core::{
    Control,
    protocol::{Session, execute},
};
use serde_json::Value;
#[test]
fn captured_python_blob_results_match_bytes_hits_and_explanations() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-contract-v1.json")).unwrap();
    let control = Control {
        callback: &|_| true,
    };
    let mut session = Session::default();
    for case in fixture["cases"].as_array().unwrap() {
        let result = execute(&mut session, case["request"].clone(), &control);
        if case["expected"].get("error").is_some() {
            assert!(result.is_err(), "{}", case["name"]);
        } else {
            assert_eq!(result.unwrap(), case["expected"], "{}", case["name"]);
        }
    }
}
