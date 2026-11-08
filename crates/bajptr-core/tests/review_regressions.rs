use bajptr_core::{
    Control,
    database::{self, Options, RowId, TextEdit},
    fields,
    rules::{self, Evidence, Rule, Rules},
};
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;

fn ctl() -> Control<'static> {
    Control {
        callback: &|_| true,
    }
}
fn rule(old: &str, new: &str, mode: &str) -> Rule {
    Rule {
        old: old.into(),
        new: new.into(),
        mode: mode.into(),
        source: String::new(),
        table: String::new(),
    }
}
fn options(skip: bool) -> Options {
    serde_json::from_value(json!({"skip_kana":skip,"explain":true,"tables":"Localize"})).unwrap()
}
fn blob(strings: &[&str]) -> Vec<u8> {
    let root = 16usize;
    let object = 4 + strings.len() * 4;
    let mut bytes = vec![0; root + object];
    bytes[0..4].copy_from_slice(&(root as u32).to_le_bytes());
    bytes[4..6].copy_from_slice(&((4 + strings.len() * 2) as u16).to_le_bytes());
    bytes[6..8].copy_from_slice(&(object as u16).to_le_bytes());
    bytes[root..root + 4].copy_from_slice(&((root - 4) as i32).to_le_bytes());
    for (slot, text) in strings.iter().enumerate() {
        bytes[8 + slot * 2..10 + slot * 2].copy_from_slice(&((4 + slot * 4) as u16).to_le_bytes());
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let fpos = root + 4 + slot * 4;
        let start = bytes.len();
        bytes[fpos..fpos + 4].copy_from_slice(&((start - fpos) as u32).to_le_bytes());
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    bytes
}
fn simple_db() -> Connection {
    let con = Connection::open_in_memory().unwrap();
    con.execute_batch("CREATE TABLE LocalizeDBSchema(Id INTEGER PRIMARY KEY, Bytes BLOB)")
        .unwrap();
    con
}

#[test]
fn empty_then_nonempty_matches_agree_in_sample_preview_and_written_blob() {
    let (sample, hits) = rules::regex_sample("a*|b", "X", "b").unwrap();
    assert_eq!((sample.as_str(), hits), ("XXX", 3));
    let rules = Rules::new(vec![rule("a*|b", "X", "re")]).unwrap();
    let mut evidence = Evidence::new(1, true);
    assert_eq!(
        rules
            .view("LocalizeDBSchema")
            .apply(&rules, "b", &mut evidence, false)
            .unwrap()
            .unwrap(),
        sample
    );
    let con = simple_db();
    let original = blob(&["b中"]);
    con.execute("INSERT INTO LocalizeDBSchema VALUES (1,?)", [&original])
        .unwrap();
    let expected = rules::regex_sample("a*|b", "X", "b中").unwrap();
    let preview =
        database::patch_database(&con, None, None, &rules, &options(false), false, &ctl()).unwrap();
    let written =
        database::patch_database(&con, None, None, &rules, &options(false), true, &ctl()).unwrap();
    assert_eq!(preview.hits, written.hits);
    assert_eq!(written.hits[0], expected.1 as u64);
    let bytes: Vec<u8> = con
        .query_row("SELECT Bytes FROM LocalizeDBSchema", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fields::parse(&bytes, true).unwrap()[0].text, expected.0);
}

#[test]
fn empty_match_does_not_allow_a_lower_priority_rule_to_consume_its_position() {
    let rules = Rules::new(vec![rule("a*", "X", "re"), rule("b", "Y", "re")]).unwrap();
    let mut evidence = Evidence::new(2, true);
    assert_eq!(
        rules
            .view("LocalizeDBSchema")
            .apply(&rules, "b中", &mut evidence, false)
            .unwrap()
            .unwrap(),
        "XbX中X"
    );
    assert_eq!(evidence.hits, vec![3, 0]);
}

#[test]
fn replacement_errors_identify_rule_record_and_field_in_preview_and_write() {
    for write in [false, true] {
        let con = simple_db();
        let first = blob(&["任务"]);
        let bad = blob(&["攻击"]);
        con.execute("INSERT INTO LocalizeDBSchema VALUES (1,?)", [&first])
            .unwrap();
        con.execute("INSERT INTO LocalizeDBSchema VALUES (2,?)", [&bad])
            .unwrap();
        let rules =
            Rules::new(vec![rule("任务", "委托", "sub"), rule("攻击", r"\t", "re")]).unwrap();
        let error =
            database::patch_database(&con, None, None, &rules, &options(false), write, &ctl())
                .unwrap_err()
                .to_string();
        for detail in [
            "LocalizeDBSchema",
            "记录 2",
            "字段 0",
            "第 2 条词条",
            "控制字符",
        ] {
            assert!(error.contains(detail), "missing {detail}: {error}");
        }
        let unchanged: Vec<u8> = con
            .query_row("SELECT Bytes FROM LocalizeDBSchema WHERE Id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            unchanged, first,
            "failed writes must roll back earlier valid rows"
        );
    }
}

#[test]
fn dynamic_output_limit_is_not_reported_as_a_bad_resource() {
    let con = simple_db();
    let text = "攻".repeat(fields::MAX_STRING / 6 + 5);
    let original = blob(&[&text]);
    con.execute("INSERT INTO LocalizeDBSchema VALUES (7,?)", [&original])
        .unwrap();
    let rules = Rules::new(vec![rule("(.+)", r"\1\1", "re")]).unwrap();
    let error = database::patch_database(&con, None, None, &rules, &options(false), false, &ctl())
        .unwrap_err()
        .to_string();
    for detail in [
        "LocalizeDBSchema",
        "记录 7",
        "字段 0",
        "第 1 条词条",
        "过长",
    ] {
        assert!(error.contains(detail), "{error}");
    }
}

#[test]
fn malformed_records_have_precise_preview_diagnostics_and_stop_writes() {
    let con = simple_db();
    let mut bad = blob(&["攻击"]);
    bad[4..6].copy_from_slice(&5u16.to_le_bytes());
    con.execute("INSERT INTO LocalizeDBSchema VALUES (9,?)", [&bad])
        .unwrap();
    let rules = Rules::new(vec![rule("攻击", "爆炸", "sub")]).unwrap();
    let preview =
        database::patch_database(&con, None, None, &rules, &options(false), false, &ctl()).unwrap();
    assert_eq!(preview.parse_failed, 1);
    let details = serde_json::to_value(preview).unwrap()["diagnostics"].to_string();
    for detail in ["LocalizeDBSchema", "9", "bad vtable length"] {
        assert!(details.contains(detail), "{details}");
    }
    let error = database::patch_database(&con, None, None, &rules, &options(false), true, &ctl())
        .unwrap_err()
        .to_string();
    for detail in ["LocalizeDBSchema", "记录 9", "bad vtable length"] {
        assert!(error.contains(detail), "{error}");
    }
    let unchanged: Vec<u8> = con
        .query_row("SELECT Bytes FROM LocalizeDBSchema", [], |r| r.get(0))
        .unwrap();
    assert_eq!(unchanged, bad);
}

#[test]
fn without_rowid_tables_use_their_real_single_primary_key() {
    let con = Connection::open_in_memory().unwrap();
    con.execute_batch(
        "CREATE TABLE LocalizeDBSchema(Id TEXT PRIMARY KEY, Bytes BLOB) WITHOUT ROWID",
    )
    .unwrap();
    con.execute(
        "INSERT INTO LocalizeDBSchema VALUES ('text-id',?)",
        [blob(&["攻击"])],
    )
    .unwrap();
    let rules = Rules::new(vec![rule("攻击", "爆炸", "sub")]).unwrap();
    let stats =
        database::patch_database(&con, None, None, &rules, &options(false), true, &ctl()).unwrap();
    assert_eq!(stats.parse_failed, 0);
    assert_eq!(stats.rows_changed, 1);
    let (found, total) = database::scan(&con, "Localize", "爆炸", 10, &ctl()).unwrap();
    assert_eq!(total, 1);
    assert_eq!(found[0].1, RowId::Text("text-id".into()));
}

#[test]
fn search_pages_cover_all_matches_and_keep_source_table_filtering_exact() {
    let con = Connection::open_in_memory().unwrap();
    con.execute_batch(
        "CREATE TABLE LocalizeDBSchema(Id INTEGER PRIMARY KEY, Bytes BLOB); CREATE TABLE SkillDBSchema(Id INTEGER PRIMARY KEY, Bytes BLOB)",
    )
    .unwrap();
    for id in 0..3 {
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (?,?)",
            rusqlite::params![id, blob(&["攻击"])],
        )
        .unwrap();
    }
    for id in 0..2 {
        con.execute(
            "INSERT INTO SkillDBSchema VALUES (?,?)",
            rusqlite::params![id, blob(&["攻击"])],
        )
        .unwrap();
    }
    let (first, total) =
        database::scan_page(&con, "Localize|Skill", "攻击", 2, 0, false, &ctl()).unwrap();
    let (second, total_again) =
        database::scan_page(&con, "Localize|Skill", "攻击", 2, 2, false, &ctl()).unwrap();
    assert_eq!(
        (first.len(), second.len(), total, total_again),
        (2, 2, 5, 5)
    );
    let (localize, local_total) =
        database::scan_page(&con, "^(?:LocalizeDBSchema)$", "攻击", 10, 0, false, &ctl()).unwrap();
    assert_eq!(local_total, 3);
    assert!(localize.iter().all(|row| row.0 == "LocalizeDBSchema"));
}

#[test]
fn search_skip_kana_matches_the_replacement_option() {
    let con = Connection::open_in_memory().unwrap();
    con.execute_batch("CREATE TABLE LocalizeDBSchema(Id INTEGER PRIMARY KEY, Bytes BLOB)")
        .unwrap();
    con.execute(
        "INSERT INTO LocalizeDBSchema VALUES (1,?)",
        [blob(&["攻撃", "ｱｲﾃﾑ攻撃"])],
    )
    .unwrap();
    let (rows, total) = database::scan_page(&con, "Localize", "攻撃", 10, 0, true, &ctl()).unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].3, "攻撃");
}

#[test]
fn composite_primary_keys_bind_only_the_selected_record() {
    for columns in ["Id", "Key"] {
        let con = Connection::open_in_memory().unwrap();
        con.execute_batch(&format!("CREATE TABLE LocalizeDBSchema({columns} INTEGER, Lang TEXT, Bytes BLOB, PRIMARY KEY({columns},Lang)) WITHOUT ROWID")).unwrap();
        let original = blob(&["攻击"]);
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (1,'cn',?)",
            [&original],
        )
        .unwrap();
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (1,'jp',?)",
            [&original],
        )
        .unwrap();
        let (found, total) = database::scan(&con, "Localize", "攻击", 10, &ctl()).unwrap();
        assert_eq!(total, 2, "{columns}");
        assert_ne!(
            found[0].1, found[1].1,
            "a partial Key column must not identify both rows"
        );
        let mut options = options(true);
        options.edits.push(TextEdit {
            wordlist_applied: false,
            table: "LocalizeDBSchema".into(),
            row_id: found[0].1.clone(),
            slot: 0,
            original: "攻击".into(),
            replacement: "只改这条".into(),
        });
        let rules = Rules::new(vec![]).unwrap();
        let stats =
            database::patch_database(&con, None, None, &rules, &options, true, &ctl()).unwrap();
        assert_eq!(stats.rows_changed, 1);
        assert_eq!(stats.parse_failed, 0);
        let modified: Vec<u8> = con
            .query_row(
                "SELECT Bytes FROM LocalizeDBSchema WHERE Lang='cn'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let untouched: Vec<u8> = con
            .query_row(
                "SELECT Bytes FROM LocalizeDBSchema WHERE Lang='jp'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fields::parse(&modified, true).unwrap()[0].text, "只改这条");
        assert_eq!(untouched, original);
    }
}

#[test]
fn non_text_tables_are_reported_separately_instead_of_blocking_text_updates() {
    let con = simple_db();
    con.execute_batch("CREATE TABLE LocalizeMetadata(Id INTEGER PRIMARY KEY, Value TEXT)")
        .unwrap();
    con.execute(
        "INSERT INTO LocalizeDBSchema VALUES (1,?)",
        [blob(&["攻击"])],
    )
    .unwrap();
    let rules = Rules::new(vec![rule("攻击", "爆炸", "sub")]).unwrap();
    let stats =
        database::patch_database(&con, None, None, &rules, &options(false), true, &ctl()).unwrap();
    assert_eq!(stats.parse_failed, 0);
    assert_eq!(stats.rows_changed, 1);
    let stats = serde_json::to_value(stats).unwrap();
    assert_eq!(stats["tables_skipped"], 1);
    assert!(
        stats["diagnostics"]
            .to_string()
            .contains("LocalizeMetadata")
    );
}

#[test]
fn one_invalid_slot_does_not_discard_valid_fields_or_other_records() {
    let con = simple_db();
    let mut original = blob(&["攻击", "其他"]);
    let object = original[6..8].to_vec();
    original[10..12].copy_from_slice(&object);
    con.execute("INSERT INTO LocalizeDBSchema VALUES (1,?)", [&original])
        .unwrap();
    let rules = Rules::new(vec![rule("攻击", "爆炸", "sub")]).unwrap();
    let stats =
        database::patch_database(&con, None, None, &rules, &options(false), true, &ctl()).unwrap();
    assert_eq!(stats.rows_changed, 1);
    assert_eq!(stats.parse_failed, 0);
    let details = serde_json::to_value(stats).unwrap();
    assert_eq!(details["fields_skipped"], 1);
    let bytes: Vec<u8> = con
        .query_row("SELECT Bytes FROM LocalizeDBSchema", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        &bytes[..28],
        &original[..28],
        "unrecognized slot and all offsets stay unchanged"
    );
    assert_eq!(fields::parse(&bytes, true).unwrap()[0].text, "爆炸");
    assert!(
        details["diagnostics"]
            .to_string()
            .contains("LocalizeDBSchema")
    );
}

#[test]
fn skip_kana_covers_halfwidth_katakana_extensions_and_explicit_edit_override() {
    for text in ["ｱｲﾃﾑ说明", "ㇰ属性", "アイテム说明"] {
        assert!(fields::has_kana(text), "{text}");
        let rules = Rules::new(vec![rule(text, "中文名称", "exact")]).unwrap();
        let original = blob(&[text]);
        let mut evidence = Evidence::new(1, true);
        let (skipped, stats) = database::patch_blob(
            &original,
            &rules,
            &rules.view("LocalizeDBSchema"),
            &mut evidence,
            &options(true),
            &HashMap::new(),
        )
        .unwrap();
        assert!(skipped.is_none());
        assert_eq!(stats.kana_skipped, 1);
        let (changed, _) = database::patch_blob(
            &original,
            &rules,
            &rules.view("LocalizeDBSchema"),
            &mut evidence,
            &options(false),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            fields::parse(changed.as_ref().unwrap(), true).unwrap()[0].text,
            "中文名称"
        );
        let (explicit, _) = database::patch_blob(
            &original,
            &rules,
            &rules.view("LocalizeDBSchema"),
            &mut evidence,
            &options(true),
            &HashMap::from([(0, "指定修改".into())]),
        )
        .unwrap();
        assert_eq!(
            fields::parse(explicit.as_ref().unwrap(), true).unwrap()[0].text,
            "指定修改"
        );
    }
    assert!(!fields::has_kana("普通攻击·ABC123"));
}

#[test]
fn a_shadowed_rowid_column_is_not_treated_as_a_unique_record_id() {
    let con = Connection::open_in_memory().unwrap();
    con.execute_batch("CREATE TABLE LocalizeDBSchema(rowid INTEGER, Bytes BLOB)")
        .unwrap();
    for _ in 0..2 {
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (5,?)",
            [blob(&["攻击"])],
        )
        .unwrap();
    }
    let (rows, total) = database::scan(&con, "Localize", "攻击", 10, &ctl()).unwrap();
    assert_eq!(total, 2);
    assert_ne!(rows[0].1, rows[1].1);
}

#[test]
fn comparison_cache_handles_composite_keys_and_non_text_tables() {
    let original = Connection::open_in_memory().unwrap();
    let current = Connection::open_in_memory().unwrap();
    for con in [&original, &current] {
        con.execute_batch("CREATE TABLE LocalizeDBSchema(Id INTEGER, Lang TEXT, Bytes BLOB, PRIMARY KEY(Id,Lang)) WITHOUT ROWID; CREATE TABLE LocalizeMetadata(Value TEXT)").unwrap();
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (1,'cn',?)",
            [blob(&["攻击"])],
        )
        .unwrap();
        con.execute(
            "INSERT INTO LocalizeDBSchema VALUES (1,'jp',?)",
            [blob(&["攻击"])],
        )
        .unwrap();
    }
    current
        .execute(
            "UPDATE LocalizeDBSchema SET Bytes=? WHERE Lang='cn'",
            [blob(&["当前文字"])],
        )
        .unwrap();
    let cached = database::current_overrides(&original, &current, "Localize", 1024 * 1024, &ctl())
        .unwrap()
        .unwrap();
    let rules = Rules::new(vec![rule("攻击", "爆炸", "sub")]).unwrap();
    let streamed = database::patch_database(
        &original,
        Some(&current),
        None,
        &rules,
        &options(false),
        false,
        &ctl(),
    )
    .unwrap();
    let cached = database::patch_database(
        &original,
        None,
        Some(&cached),
        &rules,
        &options(false),
        false,
        &ctl(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(streamed).unwrap(),
        serde_json::to_value(cached).unwrap()
    );
}

#[test]
fn scanning_bad_records_emits_a_location_and_the_actual_parse_reason() {
    let con = simple_db();
    let mut bad = blob(&["攻击"]);
    bad[4..6].copy_from_slice(&5u16.to_le_bytes());
    con.execute("INSERT INTO LocalizeDBSchema VALUES (12,?)", [bad])
        .unwrap();
    let logs = std::sync::Mutex::new(Vec::new());
    let callback = |message: Option<&str>| {
        if let Some(message) = message {
            logs.lock().unwrap().push(message.to_owned());
        }
        true
    };
    let (_, total) = database::scan(
        &con,
        "Localize",
        "攻击",
        10,
        &Control {
            callback: &callback,
        },
    )
    .unwrap();
    assert_eq!(total, 0);
    let logs = logs.lock().unwrap().join("\n");
    for detail in ["LocalizeDBSchema", "记录 12", "bad vtable length"] {
        assert!(logs.contains(detail), "{logs}");
    }
}
