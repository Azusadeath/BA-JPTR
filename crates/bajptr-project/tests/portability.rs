use bajptr_project::{Control, desktop::Project, region, storage};
use serde_json::json;
use std::fs;
fn temporary() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bajptr-portable-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&path).unwrap();
    path
}
#[cfg(windows)]
#[test]
fn unavailable_custom_trash_falls_back_to_recycle_bin() {
    let root = temporary();
    let source = root.join("scratch");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("text.txt"), "测试").unwrap();
    let unavailable = root.join("not-a-directory");
    fs::write(&unavailable, "file").unwrap();
    storage::retire_to(&source, Some(&unavailable.join("trash"))).unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&unavailable).unwrap(), "file");
    storage::retire(&root).unwrap();
}
#[test]
fn clean_project_can_save_first_rule_before_selecting_a_game() {
    let root = temporary();
    let mut project = Project::new(root.join("全新工具 目录"));
    let ctl = Control::new(&|_| true);
    let loaded = project.dispatch("load", &json!({}), &ctl).unwrap();
    assert!(loaded["errors"].as_object().unwrap().is_empty());
    assert!(loaded["rules"].as_array().unwrap().is_empty());
    project.dispatch("save_rule",&json!({"revision":0,"index":null,"rule":{"old":"暴击","new":"爆炸","mode":"sub","source":"","table":""}}),&ctl).unwrap();
    let mut reopened = Project::new(root.join("全新工具 目录"));
    assert_eq!(
        reopened.dispatch("load", &json!({}), &ctl).unwrap()["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    storage::retire(&root).unwrap();
}
#[test]
fn international_resource_layout_is_rejected_without_modifying_it() {
    let root = temporary();
    let source =
        root.join("BlueArchive_Data/StreamingAssets/PUB/Resource/Preload/TableBundles/ExcelDB.db");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"synthetic international resource").unwrap();
    assert!(region::locate(root.to_str().unwrap()).is_err());
    assert_eq!(
        fs::read(&source).unwrap(),
        b"synthetic international resource"
    );
    storage::retire(&root).unwrap();
}

#[test]
fn raw_international_key_works_but_upstream_eight_byte_trailer_is_not_supported() {
    use bajptr_core::codec;
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use std::io::Write;
    let root = temporary();
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1/encrypted.db");
    let ctl = Control::new(&|_| true);
    let plain = root.join("plain.db");
    let info = codec::decrypt(&source, &plain, &codec::key(region::KEY).unwrap(), &ctl).unwrap();
    let salt = B64.decode(info.salt).unwrap();
    let ivs: Vec<Vec<u8>> = info.ivs.iter().map(|v| B64.decode(v).unwrap()).collect();
    let international =
        codec::key("ef0aaca06f34b4a4be3172a75a3ea565e815f9ece35b1fb12b7a166ba0807bc4").unwrap();
    let input = root.join("global-synthetic.db");
    codec::encrypt(&plain, &input, &international, &salt, &ivs, &ctl).unwrap();
    let output = root.join("roundtrip.db");
    codec::decrypt(&input, &output, &international, &ctl).unwrap();
    assert_eq!(fs::read(&plain).unwrap(), fs::read(&output).unwrap());
    // Upstream's default extra_bytes (4) plus CRC-fix trailer (4).
    fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap()
        .write_all(&[8, 8, 8, 8, 0, 0, 0, 0])
        .unwrap();
    assert!(codec::decrypt(&input, &root.join("trailer.db"), &international, &ctl).is_err());
    storage::retire(&root).unwrap();
}
