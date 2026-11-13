use crate::{
    app::*,
    dialogs::{ComparisonRow, comparison_browser, window},
    state::*,
};
use eframe::egui;
use serde_json::{Value, json};

impl Desktop {
    pub(crate) fn preview_dialog(&mut self, ctx: &egui::Context) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        let stats = &preview["stats"];
        let changes = slice(stats, "changes");
        let total = count(&preview, "total_changes");
        let mut open = true;
        let mut close = false;
        let mut locate = None;
        window("预览替换", ctx, if total == 0 { 420. } else { 780. })
            .id(egui::Id::unique((
                "replacement-preview-compact",
                total == 0,
            )))
            .min_width(if total == 0 { 300. } else { 560. })
            .open(&mut open)
            .show(ctx, |ui| {
                if total > 0 {
                    ui.set_height((ctx.content_rect().height() - 130.).clamp(360., 620.));
                }
                let mut show_actions = |ui: &mut egui::Ui| {
                    let misses = list(stats, "zero_reasons");
                    if !misses.is_empty() {
                        egui::CollapsingHeader::new(format!("未命中词条（{}）", misses.len()))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("preview-missed-rules")
                                    .max_height(100.)
                                    .show(ui, |ui| {
                                        for miss in misses {
                                            let array =
                                                miss.as_array().cloned().unwrap_or_default();
                                            let index = array
                                                .first()
                                                .and_then(Value::as_u64)
                                                .map(|index| index as usize);
                                            let entry = index.and_then(|index| {
                                                slice(&self.state, "rules").get(index)
                                            });
                                            if let Some(entry) = entry {
                                                let old = entry["display_old"]
                                                    .as_str()
                                                    .unwrap_or_else(|| {
                                                        entry["rule"]["old"].as_str().unwrap_or("")
                                                    });
                                                let new = entry["display_new"]
                                                    .as_str()
                                                    .unwrap_or_else(|| {
                                                        entry["rule"]["new"].as_str().unwrap_or("")
                                                    });
                                                let caption = format!(
                                                    "{old} → {}",
                                                    if new.is_empty() {
                                                        "（删除匹配文字）"
                                                    } else {
                                                        new
                                                    }
                                                );
                                                ui.push_id(index, |ui| {
                                                    ui.horizontal(|ui| {
                                                        let width = (ui.available_width()
                                                            - action_width(ui, "定位词条")
                                                            - ui.spacing().item_spacing.x)
                                                            .max(40.);
                                                        ui.add_sized(
                                                            [width, 24.],
                                                            egui::Label::new(
                                                                egui::RichText::new(&caption)
                                                                    .strong(),
                                                            )
                                                            .truncate(),
                                                        )
                                                        .on_hover_text(&caption);
                                                        if secondary(ui, true, "定位词条").clicked()
                                                        {
                                                            locate = index;
                                                        }
                                                    });
                                                    let table = text(&entry["rule"], "table");
                                                    let scope = match bajptr_project::model::scopes(
                                                        &table,
                                                    )
                                                    .ok()
                                                    .flatten()
                                                    {
                                                        Some(tables) if !tables.is_empty() => {
                                                            tables
                                                                .iter()
                                                                .map(|table| {
                                                                    bajptr_project::model::title(
                                                                        table,
                                                                    )
                                                                })
                                                                .collect::<Vec<_>>()
                                                                .join("、")
                                                        }
                                                        _ => bajptr_project::model::scope_title(
                                                            &table,
                                                        ),
                                                    };
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&scope)
                                                                .color(MUTED),
                                                        )
                                                        .truncate(),
                                                    )
                                                    .on_hover_text(&scope);
                                                });
                                            }
                                            ui.add(
                                                egui::Label::new(
                                                    array
                                                        .get(1)
                                                        .and_then(Value::as_str)
                                                        .unwrap_or_else(|| {
                                                            miss["reason"].as_str().unwrap_or("")
                                                        }),
                                                )
                                                .wrap(),
                                            );
                                            ui.separator();
                                        }
                                    });
                            });
                    }
                    let diagnostics = list(stats, "diagnostics");
                    if !diagnostics.is_empty() {
                        if count(stats, "parse_failed") == 0 {
                            let mut skipped = Vec::new();
                            if count(stats, "tables_skipped") > 0 {
                                skipped
                                    .push(format!("{} 张来源表", count(stats, "tables_skipped")));
                            }
                            if count(stats, "fields_skipped") > 0 {
                                skipped
                                    .push(format!("{} 个异常字段", count(stats, "fields_skipped")));
                            }
                            if !skipped.is_empty() {
                                ui.colored_label(
                                    egui::Color32::from_rgb(139, 91, 21),
                                    format!("已跳过{}，原始数据保留。", skipped.join("、")),
                                );
                            }
                        }
                        egui::CollapsingHeader::new(format!(
                            "处理详情（{}项，最多显示20项）",
                            diagnostics.len()
                        ))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("preview-diagnostics")
                                .max_height(110.)
                                .show(ui, |ui| {
                                    for diagnostic in &diagnostics {
                                        let location = bajptr_project::model::title(&text(
                                            diagnostic, "table",
                                        ));
                                        ui.add(
                                            egui::Label::new(format!(
                                                "{location}：{}",
                                                text(diagnostic, "message")
                                            ))
                                            .wrap(),
                                        );
                                    }
                                });
                        });
                    }
                    if count(stats, "parse_failed") > 0 {
                        ui.colored_label(
                            RED,
                            "存在无法解析的记录，已禁止应用。展开“处理详情”查看来源与原因。",
                        );
                    }
                    let preparing = self.busy.as_ref().is_some_and(|busy| {
                        self.pending
                            .get(&busy.id)
                            .is_some_and(|action| action == "prepare_patch")
                    });
                    let ready = preview["prepared"] == true;
                    let preparation_error = text(&preview, "preparation_error");
                    if count(&preview, "total_changes") > 0 && count(stats, "parse_failed") == 0 {
                        if preparing {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.colored_label(MUTED, "正在准备…");
                            });
                        } else if !ready {
                            if !preparation_error.is_empty() {
                                ui.colored_label(RED, "准备失败");
                                ui.colored_label(RED, &preparation_error);
                            }
                            let action = if preparation_error.is_empty() {
                                "继续准备"
                            } else {
                                "重试"
                            };
                            if secondary(ui, self.busy.is_none(), action).clicked() {
                                self.submit(
                                    "prepare_patch",
                                    json!({"nonce":text(&preview,"nonce")}),
                                    "正在准备应用…",
                                );
                            }
                        }
                    }
                    if total > 0 {
                        ui.horizontal(|ui| {
                            if primary(
                                ui,
                                count(stats, "parse_failed") == 0
                                    && preview["prepared"] == true
                                    && self.busy.is_none(),
                                "应用到游戏",
                            )
                            .clicked()
                            {
                                self.submit(
                                    "apply",
                                    json!({"nonce":text(&preview,"nonce"),"prepared":true}),
                                    "正在应用到游戏…",
                                );
                                close = true;
                            }
                            ui.colored_label(MUTED, "请先退出游戏。");
                        });
                    }
                };
                if total > 0 {
                    egui::Panel::bottom("preview-actions").show(ui, &mut show_actions);
                } else {
                    show_actions(ui);
                }
                if total == 0 {
                    ui.label("本次 0 处变化");
                    ui.colored_label(
                        MUTED,
                        if count(stats, "parse_failed") > 0 {
                            "本次未生成可应用的变化，请查看处理详情。"
                        } else {
                            "当前游戏文字已与替换结果一致。"
                        },
                    );
                } else {
                    ui.label(format!(
                        "本次 {total} 处变化：新增 {}，调整 {}，撤销 {}",
                        count(stats, "added"),
                        count(stats, "updated"),
                        count(stats, "reverted")
                    ));
                    if total > changes.len() as u64 {
                        ui.colored_label(
                            MUTED,
                            format!("显示前 {} 处，上方统计包含全部变化。", changes.len()),
                        );
                    }
                    let rows = changes
                        .iter()
                        .enumerate()
                        .map(|(index, change)| {
                            let table = text(change, "table");
                            let label = self
                                .tables
                                .iter()
                                .find(|item| text(item, "name") == table)
                                .map(|item| text(item, "label"))
                                .unwrap_or(table);
                            ComparisonRow {
                                index,
                                label: format!("{} · {}", text(change, "kind"), label),
                                original: change["original"].as_str().unwrap_or(""),
                                before: change["before"].as_str().unwrap_or(""),
                                after: change["after"].as_str().unwrap_or(""),
                                active: true,
                            }
                        })
                        .collect::<Vec<_>>();
                    comparison_browser(
                        ui,
                        "preview-comparison",
                        &rows,
                        &mut self.preview_row,
                        ["当前游戏文字", "替换后的文字"],
                        None,
                    );
                }
            });
        if !open || close {
            self.preview = None;
        }
        if let Some(index) = locate {
            let row = replacement_rows(&self.state)
                .iter()
                .position(|row| match row {
                    ReplacementRow::Rule(rule) => *rule == index,
                    ReplacementRow::Combined { rules, .. } => rules.contains(&index),
                    ReplacementRow::Batch(_) => false,
                });
            if let Some(row) = row {
                self.rule_selection = Some(row);
                self.reveal_rule = Some(row);
                self.tab = 0;
                self.preview = None;
                self.restore_context_status();
            }
        }
    }
}
