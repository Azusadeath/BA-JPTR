use crate::{app::Desktop, state::*};
use eframe::egui;
use serde_json::json;

struct Snapshot {
    texts: Vec<(String, egui::Rect, egui::Rect)>,
    window: egui::Rect,
}
fn draw(ctx: &egui::Context, app: &mut Desktop, frame: u32) -> Snapshot {
    draw_events(ctx, app, frame, vec![])
}
fn draw_events(
    ctx: &egui::Context,
    app: &mut Desktop,
    frame: u32,
    events: Vec<egui::Event>,
) -> Snapshot {
    let time = ctx.input(|input| input.time).max(f64::from(frame) / 60.) + 1. / 60.;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900., 620.),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| app.dialogs(ui.ctx()),
    );
    output.textures_delta.clear();
    let window = ctx
        .memory(|memory| {
            memory
                .areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| layer.order == egui::Order::Middle)
                .filter_map(|layer| memory.area_rect(layer.id))
                .max_by(|a, b| a.area().total_cmp(&b.area()))
        })
        .unwrap_or(egui::Rect::ZERO);
    let texts = output
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_string(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
                shape.clip_rect,
            )),
            _ => None,
        })
        .collect();
    Snapshot { texts, window }
}

#[test]
fn source_template_and_about_use_normal_width_and_keep_main_actions_visible() {
    for case in ["source", "template", "about"] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        match case {
            "source" => {
                app.scope_form = Some(ScopeForm::new(
                    "A",
                    vec![json!({"name":"A","label":"界面与系统提示"})],
                ))
            }
            "template" => {
                app.state = json!({"templates":bajptr_project::templates::metadata()}).into();
                app.template_form = Some(TemplateForm {
                    key: "aliases".into(),
                    values: vec!["任务".into(), "委托".into()],
                    sample: "完成任务".into(),
                    result: "完成委托".into(),
                    built: Some(json!({})),
                    error: String::new(),
                });
            }
            _ => app.show_about = true,
        }
        let result = settle(&ctx, &mut app);
        assert!(
            result.window.width() > 300. && result.window.width() <= 600.,
            "{case}: {:?}",
            result.window
        );
        assert!(!result.texts.iter().any(|(text, _, _)| text == "取消"));
        for label in match case {
            "source" => vec!["全选", "反选", "确定"],
            "template" => vec!["填入词条"],
            _ => vec!["查看最新版本", "使用说明"],
        } {
            let (_, rect, clip) = result
                .texts
                .iter()
                .find(|(text, _, _)| text == label)
                .unwrap();
            assert!(
                clip.contains_rect(*rect) && rect.bottom() < 620.,
                "{case}: {label} is clipped"
            );
        }
    }
}

#[test]
fn record_managers_shrink_for_few_records_and_empty_search_results() {
    for batch in [false, true] {
        for amount in [0, 1, 30] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            let edits=(0..amount).map(|index|json!({"index":index,"active":true,"label":"界面文本","edit":{"batch":batch,"original":format!("任务{index}"),"replacement":format!("委托{index}"),"batch_operation":{"id":"test","before":format!("任务{index}")}}})).collect::<Vec<_>>();
            app.state = json!({"edits":edits}).into();
            if batch {
                app.batch_scope = Some(BatchScopeForm {
                    id: "test".into(),
                    ..Default::default()
                });
            } else {
                app.show_edits = true;
            }
            let result = settle(&ctx, &mut app);
            assert!(
                result.window.width() <= if amount <= 1 { 680. } else { 820. },
                "batch={batch},amount={amount}: {:?}",
                result.window
            );
            if amount == 0 {
                assert!(result.window.height() < 300., "empty: {:?}", result.window);
            }
            if amount > 0 {
                let label = if batch {
                    "从本批移除此段"
                } else {
                    "编辑文本"
                };
                let (_, rect, clip) = result
                    .texts
                    .iter()
                    .find(|(text, _, _)| text == label)
                    .unwrap();
                assert!(
                    clip.contains_rect(*rect) && rect.bottom() < 620.,
                    "{label} is clipped"
                );
                if amount == 1 && !batch {
                    assert!(result.window.height() < 360.);
                    for label in ["任务0", "委托0"] {
                        let (_, rect, clip) = result
                            .texts
                            .iter()
                            .find(|(text, _, _)| text == label)
                            .unwrap();
                        assert!(
                            clip.expand(1.).contains_rect(*rect),
                            "{label}: text={rect:?}, clip={clip:?}, window={:?}",
                            result.window
                        );
                    }
                }
                if amount > 1 && !batch {
                    app.edit_view.filter = "任务0".into();
                    let one = settle(&ctx, &mut app);
                    assert!(one.window.width() <= 680.);
                    for label in ["任务0", "委托0"] {
                        let (_, rect, clip) = one
                            .texts
                            .iter()
                            .rev()
                            .find(|(text, _, _)| text == label)
                            .unwrap();
                        assert!(
                            clip.expand(1.).contains_rect(*rect),
                            "filtered single text is clipped: {label}, {rect:?}, {clip:?}"
                        );
                    }
                }
                if batch {
                    app.batch_scope.as_mut().unwrap().filter = "没有匹配".into();
                } else {
                    app.edit_view.filter = "没有匹配".into();
                }
                if amount > 1 || batch {
                    let empty = settle(&ctx, &mut app);
                    assert!(
                        empty.window.width() <= 560. && empty.window.height() < 320.,
                        "filtered: {:?}",
                        empty.window
                    );
                    assert!(
                        empty
                            .texts
                            .iter()
                            .any(|(text, _, _)| text == "没有匹配文本。")
                    );
                }
            }
        }
    }
}

#[test]
fn migration_review_uses_compact_empty_and_single_group_layouts() {
    for amount in [0, 1, 20] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        let items=(0..amount).map(|index|json!({"group_key":format!("group-{index}"),"original":format!("原文{index}"),"replacement":format!("结果{index}"),"table":"LocalizeDBSchema","candidates":[{"row":["LocalizeDBSchema",index,0,format!("原文{index}")],"context":"可核对的上下文"}]})).collect::<Vec<_>>();
        app.migration_form = Some(crate::migration_review::MigrationForm::new(
            json!({"nonce":"test","revision":0,"items":items}),
        ));
        let result = settle(&ctx, &mut app);
        assert!(
            result.window.width() <= if amount <= 1 { 700. } else { 820. },
            "{amount}: {:?}",
            result.window
        );
        if amount == 0 {
            assert!(result.window.width() <= 560. && result.window.height() < 360.);
        } else {
            let (_, rect, clip) = result
                .texts
                .iter()
                .find(|(text, _, _)| text == "确认本组（1 处）")
                .unwrap();
            assert!(clip.contains_rect(*rect) && rect.bottom() < 620.);
            app.migration_form.as_mut().unwrap().filter = "没有匹配".into();
            let empty = settle(&ctx, &mut app);
            assert!(
                empty.window.width() <= 560. && empty.window.height() < 360.,
                "filtered: {:?}",
                empty.window
            );
            assert!(
                empty
                    .texts
                    .iter()
                    .any(|(text, _, _)| text == "没有匹配的文本。")
            );
            app.migration_form.as_mut().unwrap().filter.clear();
            for index in 0..amount {
                assert!(app.migration_form.as_mut().unwrap().confirm(index));
            }
            let complete = settle(&ctx, &mut app);
            assert!(
                complete.window.width() <= 560. && complete.window.height() < 360.,
                "complete: {:?}",
                complete.window
            );
            assert!(complete.texts.iter().any(|(text, _, _)| text == "核对完成"));
        }
    }
}

#[test]
fn editors_omit_duplicate_cancel_buttons_and_escape_discards_drafts() {
    for case in ["rule", "source", "template", "text", "batch", "combined"] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        app.state=json!({"revision":0,"rules":[],"edits":[],"templates":bajptr_project::templates::metadata()}).into();
        match case {
            "rule" => app.rule_form = Some(RuleForm::new(None, 0, None)),
            "source" => {
                app.scope_form = Some(ScopeForm::new(
                    "A",
                    vec![json!({"name":"A","label":"界面文本"})],
                ))
            }
            "template" => {
                app.template_form = Some(TemplateForm {
                    key: "aliases".into(),
                    values: vec!["任务".into(), "委托".into()],
                    sample: "任务".into(),
                    result: "委托".into(),
                    built: None,
                    error: String::new(),
                })
            }
            "text" => {
                app.text_form = Some(TextForm::new(
                    json!({"nonce":"test","label":"界面文本","original":"任务","text":"未保存的修改"}),
                ))
            }
            "batch" => {
                app.batch_form = Some(BatchForm {
                    group_id: String::new(),
                    indices: vec![0],
                    old: "任务".into(),
                    new: "未保存的修改".into(),
                    nonce: String::new(),
                    changes: vec![],
                    error: String::new(),
                })
            }
            _ => {
                app.combined_form = Some(CombinedForm::new(
                    json!({"nonce":"test","old":"任务","new":"未保存的修改","tables":["A"],"members":[]}),
                ))
            }
        }
        let before = app.state.clone();
        let result = settle(&ctx, &mut app);
        assert!(
            !result.texts.iter().any(|(text, _, _)| text == "取消"),
            "{case}"
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| app.dialogs(ui.ctx()),
        );
        output.textures_delta.clear();
        assert!(!app.modal(), "{case} is still open");
        assert!(app.pending.is_empty(), "{case} submitted an edit");
        assert_eq!(app.state, before);
    }
}

#[test]
fn every_regex_template_keeps_its_commit_action_visible_at_minimum_size() {
    for key in ["aliases", "prefix", "suffix", "between", "number"] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        let templates = bajptr_project::templates::metadata();
        let values = list(&templates[key], "defaults")
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
        app.template_form = Some(TemplateForm {
            key: key.into(),
            values,
            sample: text(&templates[key], "example"),
            result: "试写结果".into(),
            built: Some(json!({})),
            error: String::new(),
        });
        app.state = json!({"templates":templates}).into();
        let result = settle(&ctx, &mut app);
        let (_, rect, clip) = result
            .texts
            .iter()
            .find(|(text, _, _)| text == "填入词条")
            .unwrap();
        assert!(
            clip.expand(1.).contains_rect(*rect) && rect.bottom() < 620.,
            "{key}: {rect:?}, {clip:?}"
        );
    }
}
fn settle(ctx: &egui::Context, app: &mut Desktop) -> Snapshot {
    let mut result = draw(ctx, app, 0);
    for frame in 1..4 {
        result = draw(ctx, app, frame);
    }
    result
}
#[test]
fn zero_change_and_short_editors_use_compact_windows() {
    for case in ["zero", "text", "batch"] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        match case {
            "zero" => app.preview = Some(json!({"total_changes":0,"stats":{"changes":[]}}).into()),
            "text" => {
                app.text_form = Some(TextForm::new(
                    json!({"nonce":"test","label":"界面文本","original":"恢复体力","text":"补充行动力"}),
                ))
            }
            _ => {
                app.batch_form = Some(BatchForm {
                    group_id: String::new(),
                    indices: vec![0],
                    old: "体力".into(),
                    new: "行动力".into(),
                    nonce: String::new(),
                    changes: vec![],
                    error: String::new(),
                })
            }
        }
        let result = settle(&ctx, &mut app);
        assert!(
            result.window.width() > 300.
                && result.window.width() <= if case == "zero" { 500. } else { 680. },
            "{case}: {:?}",
            result.window
        );
        assert!(
            result.window.height() <= if case == "zero" { 200. } else { 350. },
            "{case}: {:?}",
            result.window
        );
        assert!(!result.texts.iter().any(|(label, _, _)| label == "关闭"));
    }
}
#[test]
fn minimum_preview_shows_multiple_entries_and_readable_full_text_without_clipping_actions() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    let before = (0..60)
        .map(|i| format!("原文第{i}行\n"))
        .collect::<String>();
    let after = (0..60)
        .map(|i| format!("结果第{i}行\n"))
        .collect::<String>();
    let changes=(0..80).map(|i|json!({"kind":"新增","table":"text","original":before,"before":format!("条目{i}\n{before}"),"after":after})).collect::<Vec<_>>();
    app.preview = Some(
        json!({"total_changes":80,"prepared":true,"stats":{"changes":changes,"added":80}}).into(),
    );
    let result = settle(&ctx, &mut app);
    let visible = result
        .texts
        .iter()
        .filter(|(label, rect, clip)| {
            label.starts_with("条目") && !label.contains('\n') && clip.contains_rect(*rect)
        })
        .count();
    assert!(visible >= 4, "only {visible} entries are visible");
    let (_, _, before_clip) = result
        .texts
        .iter()
        .find(|(label, _, _)| label.starts_with("条目0\n"))
        .unwrap();
    let (_, _, after_clip) = result
        .texts
        .iter()
        .find(|(label, _, _)| label.starts_with("结果第0行\n"))
        .unwrap();
    for clip in [before_clip, after_clip] {
        assert!(
            clip.height() >= 100. && clip.width() >= 300.,
            "full text pane too small: {clip:?}"
        );
    }
    let (_, rect, clip) = result
        .texts
        .iter()
        .find(|(text, _, _)| text == "应用到游戏")
        .unwrap();
    assert!(
        clip.contains_rect(*rect) && rect.bottom() < 620.,
        "apply action is clipped"
    );
    assert!(result.window.width() <= 850.);
}

fn click_label(
    ctx: &egui::Context,
    app: &mut Desktop,
    snapshot: &Snapshot,
    label: &str,
) -> Snapshot {
    let (_, rect, clip) = snapshot
        .texts
        .iter()
        .find(|(text, _, _)| text == label)
        .unwrap();
    assert!(
        clip.contains_rect(*rect),
        "{label} is clipped: {rect:?}, {clip:?}"
    );
    let pos = rect.center();
    for (frame, pressed) in [(10, true), (11, false)] {
        draw_events(
            ctx,
            app,
            frame,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    settle(ctx, app)
}

#[test]
fn filling_original_and_live_batch_result_remain_an_unsaved_draft() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    let saved_state = app.state.clone();
    app.text_form = Some(TextForm::new(json!({
        "nonce":"test", "label":"界面文本", "original":"恢复体力", "text":"补充体力",
        "batch_operation":{"old":"体力","new":"行动力"},
    })));
    let initial = settle(&ctx, &mut app);
    assert!(
        initial
            .texts
            .iter()
            .any(|(label, _, _)| label == "补充行动力")
    );
    assert!(!initial.texts.iter().any(|(label, _, _)| label == "未保存"));
    let filled = click_label(&ctx, &mut app, &initial, "填回原文");
    assert_eq!(app.text_form.as_ref().unwrap().value, "恢复体力");
    assert!(
        filled
            .texts
            .iter()
            .any(|(label, _, _)| label == "恢复行动力")
    );
    assert!(filled.texts.iter().any(|(label, _, _)| label == "未保存"));
    assert!(app.pending.is_empty());
    assert_eq!(app.state, saved_state);
    app.text_form.as_mut().unwrap().value = "使用体力\r\n体力".repeat(100);
    let edited = settle(&ctx, &mut app);
    assert_eq!(
        app.text_form.as_ref().unwrap().final_text(),
        "使用行动力\n行动力".repeat(100)
    );
    assert!(
        edited
            .texts
            .iter()
            .any(|(label, _, _)| label.starts_with("使用行动力\n行动力"))
    );
    let (_, rect, clip) = edited
        .texts
        .iter()
        .find(|(label, _, _)| label == "保存修改")
        .unwrap();
    assert!(
        clip.contains_rect(*rect) && rect.bottom() < 620.,
        "save action is clipped: {rect:?}, {clip:?}"
    );
    draw_events(
        &ctx,
        &mut app,
        20,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(app.text_form.is_none());
    assert!(app.pending.is_empty());
    assert_eq!(app.state, saved_state);
}

#[test]
fn source_picker_can_clear_combined_table_scope_without_saving_the_draft() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    let state = app.state.clone();
    app.combined_form = Some(CombinedForm::new(json!({
        "nonce":"test", "old":"任务", "new":"委托", "tables":["A","B"], "members":[],
    })));
    app.scope_form = Some(ScopeForm::new(
        r#"["A","B"]"#,
        vec![
            json!({"name":"A","label":"界面文本"}),
            json!({"name":"B","label":"任务文本"}),
        ],
    ));
    let initial = settle(&ctx, &mut app);
    let cleared = click_label(&ctx, &mut app, &initial, "清空");
    assert!(app.scope_form.as_ref().unwrap().checked.is_empty());
    click_label(&ctx, &mut app, &cleared, "确定");
    assert!(app.scope_form.is_none());
    assert_eq!(
        app.combined_form.as_ref().unwrap().payload()["tables"],
        json!([])
    );
    assert!(app.pending.is_empty());
    assert_eq!(app.state, state);
}

#[test]
fn missed_rule_locator_reveals_the_actual_main_row_after_rule_merging() {
    for (index, expected_row) in [(1, 0), (64, 63)] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        let mut rules = vec![
            json!({"rule":{"old":"任务","new":"委托","mode":"sub","table":"LocalizeDBSchema"}}),
            json!({"rule":{"old":"任务","new":"委托","mode":"sub","table":"LocalizeSkillDBSchema"}}),
        ];
        rules.extend((2..65).map(|index| json!({"display_old":format!("旧词{index}"),"display_new":format!("新词{index}"),"rule":{"old":format!("旧词{index}"),"new":format!("新词{index}"),"mode":"exact","table":"LocalizeDBSchema"}})));
        app.apply_state(json!({"game":"test-game","version":"current","revision":0,"rules":rules,"batches":[{"id":"batch","old":"任务","new":"委托","active":true,"version":"current","count":1}]}));
        app.loaded = true;
        app.connected = true;
        app.tab = 1;
        app.preview = Some(
            json!({"total_changes":0,"stats":{"zero_reasons":[[index,"没有匹配文本","",""]]}})
                .into(),
        );
        let initial = settle(&ctx, &mut app);
        let expanded = click_label(&ctx, &mut app, &initial, "未命中词条（1）");
        let old = if index == 1 {
            "任务".to_string()
        } else {
            format!("旧词{index}")
        };
        assert!(
            expanded
                .texts
                .iter()
                .any(|(label, _, _)| label.starts_with(&old))
        );
        click_label(&ctx, &mut app, &expanded, "定位词条");
        assert!(app.preview.is_none());
        assert_eq!(app.tab, 0);
        assert_eq!(app.rule_selection, Some(expected_row));
        let mut visible = false;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 620.),
                    )),
                    time: Some(ctx.input(|input| input.time) + 1. / 60.),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.replacement(ui));
                },
            );
            output.textures_delta.clear();
            visible = output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == old => shape
                    .clip_rect
                    .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
                _ => false,
            });
        }
        assert!(visible, "located rule {index} is outside the visible list");
        assert!(app.reveal_rule.is_none());
        assert!(app.pending.is_empty());
    }
}

#[test]
fn skipping_a_migration_group_keeps_it_pending_and_confirmation_does_not_save() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    let items = (0..2).map(|index| json!({"group_key":format!("group-{index}"),"original":format!("原文{index}"),"replacement":format!("结果{index}"),"table":"LocalizeDBSchema","candidates":[{"row":["LocalizeDBSchema",index,0,format!("原文{index}")],"context":"上下文"}]})).collect::<Vec<_>>();
    app.migration_form = Some(crate::migration_review::MigrationForm::new(
        json!({"nonce":"test","items":items}),
    ));
    let initial = settle(&ctx, &mut app);
    let skipped = click_label(&ctx, &mut app, &initial, "暂时跳过");
    let form = app.migration_form.as_ref().unwrap();
    assert_eq!(form.selected, Some(1));
    assert_eq!(form.confirmed_count(), 0);
    assert!(form.payload()["choices"].as_array().unwrap().is_empty());
    click_label(&ctx, &mut app, &skipped, "确认本组（1 处）");
    let form = app.migration_form.as_ref().unwrap();
    assert_eq!(form.selected, Some(0));
    assert_eq!(form.confirmed_count(), 1);
    assert!(!form.groups[0].confirmed);
    assert!(app.pending.is_empty());
}

#[test]
fn preparation_blocks_apply_and_closing_preview_keeps_the_background_task() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    let preview = json!({"nonce":"test","total_changes":1,"prepared":false,"stats":{"changes":[{"kind":"新增","original":"体力","before":"体力","after":"行动力"}]}});
    app.preview = Some(preview.clone().into());
    app.busy = Some(crate::app::Busy {
        id: 7,
        cancellable: true,
    });
    app.pending.insert(7, "prepare_patch".into());
    let preparing = settle(&ctx, &mut app);
    click_label(&ctx, &mut app, &preparing, "应用到游戏");
    assert!(app.preview.is_some());
    draw_events(
        &ctx,
        &mut app,
        20,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(app.preview.is_none());
    assert_eq!(app.busy.as_ref().unwrap().id, 7);
    assert_eq!(app.pending.get(&7).unwrap(), "prepare_patch");
    assert!(!app.obsolete.contains(&7));
    app.busy = None;
    app.pending.clear();
    let mut failed = preview;
    failed["preparation_error"] = json!("准备文件无法写入，请检查目录权限。");
    app.preview = Some(failed.into());
    let failed = settle(&ctx, &mut app);
    assert!(failed.texts.iter().any(|(label, _, _)| label == "重试"));
    assert!(
        failed
            .texts
            .iter()
            .any(|(label, _, _)| label == "准备文件无法写入，请检查目录权限。")
    );
    click_label(&ctx, &mut app, &failed, "应用到游戏");
    assert!(app.preview.is_some());
}
