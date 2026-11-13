use crate::{
    app::*,
    dialogs::{record_browser, window},
    state::*,
};
use eframe::egui;

impl Desktop {
    pub fn combined_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.combined_form.take() else {
            return;
        };
        let mut open = true;
        let nested = self.scope_form.is_some();
        window("编辑替换内容与范围", ctx, 760.)
            .id(egui::Id::unique("combined-editor-compact"))
            .min_width(560.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.;
                ui.set_height((ctx.content_rect().height() - 90.).clamp(380., 620.));
                ui.add_enabled_ui(!nested && !self.write_busy(), |ui| {
                    egui::Panel::bottom("combined-actions").show(ui, |ui| {
                        if !form.error.is_empty() {
                            ui.colored_label(RED, &form.error);
                        }
                        ui.horizontal(|ui| {
                            if primary(ui, !form.old.is_empty(), "保存修改").clicked() {
                                self.submit(
                                    "combined_save",
                                    form.payload(),
                                    "正在一起保存整表和指定文本替换…",
                                );
                            }
                        });
                    });
                    ui.columns(2, |columns| {
                        columns[0].horizontal(|ui| {
                            ui.label("原文字");
                            ui.add(
                                egui::TextEdit::singleline(&mut form.old)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                        columns[1].horizontal(|ui| {
                            ui.label("替换为");
                            ui.add(
                                egui::TextEdit::singleline(&mut form.new)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    });
                    ui.separator();
                    let tables = bajptr_project::model::scopes(&form.scope)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("整表替换 · {} 个来源表", tables.len()));
                        if secondary(ui, true, "选择来源表…").clicked() {
                            let mut scope = ScopeForm::new(&form.scope, vec![]);
                            scope.old = form.old.clone();
                            scope.mode = "sub".into();
                            self.query_scope_sources(&mut scope);
                            self.scope_form = Some(scope);
                        }
                    });
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "指定文本 · {} 条",
                            form.members.len() - form.removed.len()
                        ));
                        let selected_count = form.selection.rows.len();
                        let action = if selected_count > 1 {
                            format!("移出所选（{selected_count}）")
                        } else {
                            "移出所选".into()
                        };
                        if secondary(ui, selected_count > 0, &action).clicked() {
                            form.removed.extend(form.selection.rows.iter().copied());
                            form.selection.clear();
                            form.selected = None;
                        }
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut form.filter)
                                    .hint_text("搜索文本或来源表")
                                    .desired_width(160.),
                            )
                            .changed()
                        {
                            form.selected = None;
                            form.selection.clear();
                        }
                    });
                    let preview_members = form
                        .members
                        .iter()
                        .filter(|member| !form.removed.contains(&(count(member, "index") as usize)))
                        .map(|member| {
                            let mut member = member.clone();
                            let before = text(&member["edit"]["batch_operation"], "before");
                            member["edit"]["replacement"] =
                                serde_json::json!(before.replace(&form.old, &form.new));
                            member
                        })
                        .collect::<Vec<_>>();
                    let members = preview_members.iter().collect::<Vec<_>>();
                    if members.is_empty() {
                        ui.colored_label(MUTED, "没有指定文本");
                        ui.take_available_space();
                    } else {
                        record_browser(
                            ui,
                            "combined-members",
                            &members,
                            &form.filter,
                            &mut form.selected,
                            true,
                            Some(&mut form.selection),
                        );
                    }
                });
            });
        if open {
            self.combined_form = Some(form);
        }
    }
}
