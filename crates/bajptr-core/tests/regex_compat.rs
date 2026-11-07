use bajptr_core::rules::{Evidence, Rule, Rules};

fn replace(pattern: &str, replacement: &str, text: &str) -> String {
    let rules = Rules::new(vec![Rule {
        old: pattern.into(),
        new: replacement.into(),
        mode: "re".into(),
        source: String::new(),
        table: String::new(),
    }])
    .unwrap();
    let mut evidence = Evidence::new(1, false);
    rules
        .view("LocalizeDBSchema")
        .apply(&rules, text, &mut evidence, false)
        .unwrap()
        .unwrap_or_else(|| text.to_string())
}

#[test]
fn python_regex_features_execute_without_a_compatibility_process() {
    assert_eq!(replace(r"(?<=暴)击", "炸", "暴击"), "暴炸");
    assert_eq!(replace(r"(攻击)\1", r"\g<1>", "攻击攻击"), "攻击");
    assert_eq!(replace(r"\w+", "字", "暴击 e\u{301} ²"), "字 字 ²");
    assert_eq!(replace(r"\A|\Z", "边", "暴击"), "边暴击边");
    assert_eq!(replace(r"击$", "炸", "暴击\n"), "暴炸\n");
}

#[test]
fn regex_samples_match_python_unicode_and_empty_match_contracts() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("regex-oracle.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result = bajptr_core::rules::regex_sample(
            case["old"].as_str().unwrap(),
            case["new"].as_str().unwrap(),
            case["text"].as_str().unwrap(),
        );
        let (text, count) = result.unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_eq!(
            serde_json::json!([text, count]),
            serde_json::json!([case["result"], case["count"]]),
            "{case}"
        );
    }
}

#[test]
fn pathological_backtracking_fails_within_a_bounded_budget() {
    let text = "ab".repeat(28);
    let started = std::time::Instant::now();
    let result = bajptr_core::rules::regex_sample(r"(?i)(a|b|ab)*(?>c)", "改", &text);
    assert!(
        matches!(result, Err(bajptr_core::Error::Regex(_))),
        "{result:?}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
