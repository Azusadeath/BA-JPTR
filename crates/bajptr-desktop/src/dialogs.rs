use crate::{app::*, state::*};
use eframe::egui::{self, RichText};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn window<'a>(title: &'a str, ctx: &egui::Context, width: f32) -> egui::Window<'a> {
    egui::Window::new(title)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Light))
                .fill(egui::Color32::WHITE)
                .corner_radius(12)
                .inner_margin(18),
        )
        .title_frame(
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .inner_margin(egui::Margin::symmetric(12, 10)),
        )
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(width)
        .max_width((ctx.content_rect().width() - 32.).max(400.))
        .max_height((ctx.content_rect().height() - 40.).max(400.))
}

// Share text filtering and full-text presentation without forcing both managers
// to use the same list/detail layout.
fn record_filter(ui: &mut egui::Ui, id: &str, filter: &mut String, selected: &mut Option<usize>) {
    ui.push_id((id, "filter"), |ui| {
        if ui
            .add(
                egui::TextEdit::singleline(filter)
                    .hint_text("搜索原文、替换结果或来源表")
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            *selected = None;
        }
    });
}

fn record_browser(
    ui: &mut egui::Ui,
    id: &str,
    entries: &[&Value],
    filter: &str,
    selected: &mut Option<usize>,
    batch: bool,
) {
    ui.push_id(id, |ui| {
        let visible: Vec<_> = entries
            .iter()
            .copied()
            .filter(|e| edit_matches(e, filter))
            .collect();
        if !visible
            .iter()
            .any(|e| Some(count(e, "index") as usize) == *selected)
        {
            *selected = None;
        }
        ui.colored_label(
            MUTED,
            format!("显示 {} / {} 段", visible.len(), entries.len()),
        );
        let body = ui.available_height();
        egui::Panel::top("records")
            .resizable(true)
            .default_size(body * 0.48)
            .size_range(100. ..=(body - if batch { 160. } else { 120. }).max(100.))
            .show(ui, |ui| {
                let text_width = (ui.available_width() - 145.) * 0.5;
                let widths = [145., text_width, text_width];
                table_header(
                    ui,
                    &[
                        "来源表",
                        "底稿原文",
                        if batch {
                            "本批替换后"
                        } else {
                            "单条修改后"
                        },
                    ],
                    &widths,
                );
                egui::ScrollArea::vertical()
                    .id_salt("rows")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, visible.len(), |ui, range| {
                        for i in range {
                            let entry = visible[i];
                            let index = count(entry, "index") as usize;
                            let label = if entry["active"] == false {
                                format!("{}（版本失效）", text(entry, "label"))
                            } else {
                                text(entry, "label")
                            };
                            if data_row(
                                ui,
                                index,
                                *selected == Some(index),
                                &[
                                    label,
                                    text(&entry["edit"], "original").replace(['\r', '\n'], " "),
                                    text(&entry["edit"], "replacement").replace(['\r', '\n'], " "),
                                ],
                                &widths,
                            )
                            .clicked()
                            {
                                *selected = Some(index);
                            }
                        }
                    });
            });
        if let Some(entry) = visible
            .iter()
            .find(|e| Some(count(e, "index") as usize) == *selected)
        {
            if entry["active"] == false {
                ui.colored_label(RED, "版本失效：可查看或移除，不能继续编辑。");
            }
            let edit = &entry["edit"];
            let before = if batch {
                text(&edit["batch_operation"], "before")
            } else {
                text(edit, "original")
            };
            let original = text(edit, "original");
            let after = text(edit, "replacement");
            if batch && original == before {
                egui::CollapsingHeader::new("底稿原文（与本批前相同）").show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("original")
                        .min_scrolled_height(0.)
                        .max_height((ui.available_height() - 100.).clamp(22., 70.))
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(&original).wrap().selectable(true));
                        });
                });
            }
            let height = ui.available_height().max(0.);
            let mut comparisons = Vec::new();
            if batch && original != before {
                comparisons.push(("底稿原文", original.as_str()));
            }
            comparisons.push((
                if batch {
                    "本批替换前"
                } else {
                    "底稿原文"
                },
                before.as_str(),
            ));
            comparisons.push((
                if batch {
                    "本批替换后"
                } else {
                    "单条修改后"
                },
                after.as_str(),
            ));
            if ui.available_width() >= 660. {
                ui.columns(comparisons.len(), |cols| {
                    for (column, (label, value)) in cols.iter_mut().zip(&comparisons) {
                        full_text(column, label, label, value, height);
                    }
                });
            } else {
                let text_height = (height
                    - ui.spacing().item_spacing.y * (comparisons.len() - 1) as f32)
                    / comparisons.len() as f32;
                for (label, value) in comparisons {
                    full_text(ui, label, label, value, text_height);
                }
            }
        } else {
            ui.colored_label(
                MUTED,
                if visible.is_empty() {
                    "没有匹配文本。"
                } else {
                    "选择一段查看完整内容。"
                },
            );
            ui.take_available_space();
        }
    });
}

fn full_text(ui: &mut egui::Ui, id: &str, label: &str, value: &str, height: f32) {
    let heading = ui.label(RichText::new(label).strong());
    let remaining = (height - heading.rect.height() - ui.spacing().item_spacing.y).max(0.);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .min_scrolled_height(0.)
        .max_height(remaining)
        .show(ui, |ui| {
            ui.add(egui::Label::new(value).wrap().selectable(true));
        });
}

fn single_edit_browser(
    ui: &mut egui::Ui,
    entries: &[&Value],
    selected: &mut Option<usize>,
    show_list: bool,
) {
    ui.push_id("single-edit-browser", |ui| {
        if show_list {
            egui::Panel::left("records")
                .resizable(true)
                .default_size(220.)
                .size_range(160. ..=300.)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("single-records")
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_HEIGHT, entries.len(), |ui, range| {
                            for i in range {
                                let entry = entries[i];
                                let index = count(entry, "index") as usize;
                                let summary = format!(
                                    "{} · {}",
                                    text(entry, "label"),
                                    text(&entry["edit"], "original")
                                        .replace(['\r', '\n'], " ")
                                        .chars()
                                        .take(80)
                                        .collect::<String>()
                                );
                                let width = ui.available_width();
                                if data_row(
                                    ui,
                                    index,
                                    *selected == Some(index),
                                    &[summary],
                                    &[width],
                                )
                                .clicked()
                                {
                                    *selected = Some(index);
                                }
                            }
                        });
                });
        }
        if let Some(entry) = entries
            .iter()
            .find(|entry| Some(count(entry, "index") as usize) == *selected)
        {
            ui.add(egui::Label::new(text(entry, "label")).truncate())
                .on_hover_text(text(&entry["edit"], "table"));
            if entry["active"] == false {
                ui.colored_label(RED, "版本失效：可查看或移除，不能继续编辑。");
            }
            let height = ui.available_height().max(0.);
            let original = text(&entry["edit"], "original");
            let replacement = text(&entry["edit"], "replacement");
            if ui.available_width() >= 560. {
                ui.columns(2, |columns| {
                    full_text(&mut columns[0], "original", "底稿原文", &original, height);
                    full_text(
                        &mut columns[1],
                        "replacement",
                        "单条修改后",
                        &replacement,
                        height,
                    );
                });
            } else {
                let half = (height - ui.spacing().item_spacing.y) * 0.5;
                full_text(ui, "original", "底稿原文", &original, half);
                full_text(ui, "replacement", "单条修改后", &replacement, half);
            }
        } else {
            ui.colored_label(MUTED, "没有匹配文本。");
        }
    });
}

impl Desktop {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let before = [
            self.rule_form.is_some(),
            self.template_form.is_some(),
            self.text_form.is_some(),
            self.batch_form.is_some(),
        ];
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !self.write_busy() {
            if self.table_detail.is_some() {
                self.table_detail = None;
            } else if self.error.is_some() {
                self.error = None;
            } else if self.template_form.is_some() {
                self.template_form = None;
            } else if self.scope_form.is_some() {
                self.scope_form = None;
            } else if self.text_form.is_some() {
                self.text_form = None;
            } else if self.batch_scope.is_some() {
                self.batch_scope = None;
            } else if self.batch_form.is_some() {
                self.batch_form = None;
            } else if self.preview.is_some() {
                self.preview = None;
            } else if self.rule_form.is_some() {
                self.rule_form = None;
            } else if self.confirm_import.is_some() {
                self.confirm_import = None;
            } else if self.confirm_restore {
                self.confirm_restore = false;
            } else if self.confirm_baseline.is_some() {
                self.confirm_baseline = None;
                self.status = "尚未确认底稿，点击“检查底稿”继续。".into();
            } else if self.show_edits {
                self.show_edits = false;
            } else {
                self.show_logs = false;
            }
        }
        self.rule_dialog(ctx);
        self.scope_dialog(ctx);
        self.template_dialog(ctx);
        self.text_dialog(ctx);
        self.batch_dialog(ctx);
        self.batch_scope_dialog(ctx);
        self.preview_dialog(ctx);
        self.source_notices_dialog(ctx);
        self.edit_manager(ctx);
        self.confirmations(ctx);
        if let Some(detail) = self.table_detail.clone() {
            let mut open = true;
            window("来源表详情", ctx, 580.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.heading(text(&detail, "label"));
                    ui.label(text(&detail, "purpose"));
                    ui.colored_label(MUTED, text(&detail, "basis"));
                    ui.add_space(8.);
                    ui.label("原始表名");
                    ui.add(
                        egui::Label::new(text(&detail, "name"))
                            .wrap()
                            .selectable(true),
                    );
                    if ui.button("复制原表名").clicked() {
                        ctx.copy_text(text(&detail, "name"));
                    }
                });
            if !open {
                self.table_detail = None;
            }
        }
        if self.show_logs {
            let mut open = true;
            let longest = self
                .logs
                .iter()
                .max_by_key(|line| line.chars().count())
                .map(String::as_str)
                .unwrap_or("");
            let font = egui::TextStyle::Body.resolve(&ctx.style_of(egui::Theme::Light));
            let width = ctx
                .fonts_mut(|fonts| fonts.layout_no_wrap(longest.into(), font, TEXT).size().x + 12.)
                .clamp(320., (ctx.content_rect().width() - 80.).clamp(320., 720.));
            window("运行详情", ctx, width)
                .id(egui::Id::unique("run-details-content-width"))
                .min_width(280.)
                .max_width(width)
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(ctx.content_rect().height() - 160.)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.add(egui::Label::new(line).wrap().selectable(true));
                            }
                        });
                });
            self.show_logs = open;
        }
        if let Some(message) = self.error.clone() {
            let mut open = true;
            let mut close = false;
            window("未完成", ctx, 520.).open(&mut open).show(ctx, |ui| {
                ui.add(egui::Label::new(message).wrap());
                ui.add_space(12.);
                if primary(ui, true, "知道了").clicked() {
                    close = true;
                }
                if ui.button("运行详情").clicked() {
                    self.show_logs = true;
                }
            });
            if !open || close {
                self.error = None;
            }
        }
        let after = [
            self.rule_form.is_some(),
            self.template_form.is_some(),
            self.text_form.is_some(),
            self.batch_form.is_some(),
        ];
        if before
            .into_iter()
            .zip(after)
            .any(|(before, after)| before && !after)
        {
            self.restore_context_status();
        }
    }
    fn rule_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.rule_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let nested = self.scope_form.is_some()
            || self.template_form.is_some()
            || self.table_detail.is_some();
        window(
            if form.index.is_some() {
                "编辑词条"
            } else {
                "添加词条"
            },
            ctx,
            640.,
        )
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_enabled_ui(!nested && !self.write_busy(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("匹配方式");
                    egui::ComboBox::from_id_salt("rule-mode")
                        .selected_text(mode_name(&form.mode))
                        .show_ui(ui, |ui| {
                            for (mode, label) in [
                                ("sub", "包含文字"),
                                ("exact", "整段相同"),
                                ("re", "正则模板"),
                            ] {
                                ui.selectable_value(&mut form.mode, mode.into(), label);
                            }
                        });
                });
                if form.mode != "re" || form.raw {
                    ui.label(if form.mode == "re" {
                        "原表达式"
                    } else {
                        "原文字"
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut form.old)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY),
                    );
                    ui.label("替换为");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.new)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY),
                    );
                } else {
                    if !form.draft.is_null() {
                        for value in list(&form.draft, "values") {
                            ui.label(value.as_str().unwrap_or(""));
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(if form.draft.is_null() {
                            "正则模板…"
                        } else {
                            "编辑模板…"
                        })
                        .clicked()
                    {
                        let key = if form.draft.is_null() {
                            "aliases".into()
                        } else {
                            text(&form.draft, "key")
                        };
                        let template = &self.state["templates"][&key];
                        let values = if form.draft.is_null() {
                            if form.mode != "re" {
                                vec![form.old.clone(), form.new.clone()]
                            } else {
                                list(template, "defaults")
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect()
                            }
                        } else {
                            list(&form.draft, "values")
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        };
                        self.template_form = Some(TemplateForm {
                            key,
                            values,
                            sample: if form.draft.is_null() {
                                text(template, "example")
                            } else {
                                text(&form.draft, "sample")
                            },
                            result: String::new(),
                            built: None,
                            error: String::new(),
                        });
                        self.template_changed = Some(Instant::now());
                    }
                    if form.mode == "re" {
                        ui.checkbox(&mut form.raw, "直接编辑表达式");
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let label = if form.scope.is_empty() || form.scope == "[]" {
                        "未选来源表".into()
                    } else if form.scope.starts_with('[') {
                        format!(
                            "已选 {} 张表",
                            serde_json::from_str::<Vec<String>>(&form.scope)
                                .unwrap_or_default()
                                .len()
                        )
                    } else {
                        self.tables
                            .iter()
                            .find(|t| text(t, "name") == form.scope)
                            .map(|t| text(t, "label"))
                            .unwrap_or(form.scope.clone())
                    };
                    ui.label(label);
                    if ui.button("选择来源表…").clicked() {
                        let mut scope = ScopeForm::new(&form.scope, vec![]);
                        scope.old = form.old.clone();
                        scope.mode = form.mode.clone();
                        self.query_scope_sources(&mut scope);
                        self.scope_form = Some(scope);
                    }
                });
                if form.new.is_empty() && !form.old.is_empty() {
                    ui.colored_label(MUTED, "替换内容留空，将删除匹配文字。");
                }
                if !form.error.is_empty() {
                    ui.colored_label(RED, &form.error);
                }
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    if primary(
                        ui,
                        !form.old.is_empty()
                            && !form.scope.is_empty()
                            && form.scope != "[]"
                            && (form.mode != "re" || form.raw || !form.draft.is_null()),
                        "保存词条",
                    )
                    .clicked()
                    {
                        self.submit("save_rule", form.payload(), "正在保存词条…", false);
                    }
                    if ui.button("取消").clicked() {
                        close = true;
                    }
                });
            });
        });
        if open && !close {
            self.rule_form = Some(form);
        }
    }
    fn scope_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.scope_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        window("选择来源表", ctx, 650.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.filter)
                            .font(egui::TextStyle::Body)
                            .hint_text("筛选中文名称或表名")
                            .desired_width(330.),
                    );
                    if ui
                        .add_enabled(form.request.is_none(), egui::Button::new("刷新"))
                        .clicked()
                    {
                        self.query_scope_sources(&mut form);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (label, operation) in
                        [("全选", "select"), ("清空", "clear"), ("反选", "invert")]
                    {
                        if ui
                            .add_enabled(form.request.is_none(), egui::Button::new(label))
                            .clicked()
                        {
                            form.update_visible(operation);
                            form.error.clear();
                        }
                    }
                    ui.colored_label(MUTED, format!("已选 {} 张表", form.checked.len()));
                });
                let mut available = form.available.clone();
                if form.old.is_empty() && form.request.is_none() {
                    for name in form.checked.clone() {
                        if !available.iter().any(|v| text(v, "name") == name) {
                            available.push(json!({"name":name,"label":name}));
                        }
                    }
                }
                let needle = form.filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height((ctx.content_rect().height() - 350.).clamp(150., 320.))
                    .show(ui, |ui| {
                        if form.request.is_some() {
                            ui.spinner();
                            ui.colored_label(MUTED, "正在读取来源表…");
                        } else if available.is_empty() && form.error.is_empty() {
                            ui.colored_label(
                                MUTED,
                                if form.old.is_empty() {
                                    "未读取到来源表。"
                                } else {
                                    "没有匹配的来源表。"
                                },
                            );
                        }
                        for row in available {
                            let name = text(&row, "name");
                            let label = text(&row, "label");
                            if !format!("{name} {label}").to_lowercase().contains(&needle) {
                                continue;
                            }
                            let mut checked = form.checked.contains(&name);
                            let response = ui
                                .allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), 34.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.checkbox(&mut checked, label)
                                            .on_hover_text(text(&row, "purpose"))
                                    },
                                )
                                .inner;
                            if response.changed() {
                                form.error.clear();
                                if checked {
                                    form.checked.insert(name);
                                } else {
                                    form.checked.remove(&name);
                                }
                            }
                        }
                    });
                if !form.error.is_empty() {
                    ui.colored_label(RED, &form.error);
                }
                ui.horizontal(|ui| {
                    if primary(ui, true, "确定").clicked() {
                        match form.encode() {
                            Ok(scope) => {
                                if let Some(rule) = &mut self.rule_form {
                                    rule.scope = scope;
                                }
                                close = true;
                            }
                            Err(error) => form.error = error,
                        }
                    }
                    if ui.button("取消").clicked() {
                        close = true;
                    }
                });
            });
        if open && !close {
            self.scope_form = Some(form);
        }
    }
    fn template_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.template_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let mut changed = false;
        window("正则模板", ctx, 680.)
            .open(&mut open)
            .show(ctx, |ui| {
                let templates = self.state["templates"].clone();
                let old_key = form.key.clone();
                egui::ComboBox::from_id_salt("template-kind")
                    .selected_text(text(&templates[&form.key], "title"))
                    .show_ui(ui, |ui| {
                        for key in ["aliases", "prefix", "suffix", "between", "number"] {
                            ui.selectable_value(
                                &mut form.key,
                                key.into(),
                                text(&templates[key], "title"),
                            );
                        }
                    });
                if old_key != form.key {
                    form.values = list(&templates[&form.key], "defaults")
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                    form.sample = text(&templates[&form.key], "example");
                    changed = true;
                }
                let template = &templates[&form.key];
                ui.add(egui::Label::new(text(template, "description")).wrap());
                let fields = list(template, "fields");
                for (index, value) in form.values.iter_mut().enumerate() {
                    ui.label(fields.get(index).and_then(Value::as_str).unwrap_or("文字"));
                    changed |= ui
                        .add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY))
                        .changed();
                }
                ui.label("试写文本");
                changed |= ui
                    .add(
                        egui::TextEdit::multiline(&mut form.sample)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3),
                    )
                    .changed();
                if changed {
                    form.built = None;
                    form.result.clear();
                    form.error.clear();
                    self.template_changed = Some(Instant::now());
                }
                ui.label("替换效果");
                ui.add(egui::Label::new(&form.result).wrap());
                if !form.error.is_empty() {
                    ui.colored_label(RED, &form.error);
                }
                let due = self
                    .template_changed
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(250));
                if due && self.busy.is_none() {
                    self.template_changed = None;
                    self.submit(
                        "template",
                        json!({"key":form.key,"values":form.values,"sample":form.sample}),
                        "正在试写…",
                        true,
                    );
                }
                ui.horizontal(|ui| {
                    if primary(ui, form.built.is_some() && !self.write_busy(), "填入词条").clicked()
                    {
                        let built = form.built.as_ref().unwrap();
                        if let Some(rule) = &mut self.rule_form {
                            rule.old = text(built, "old");
                            rule.new = text(built, "new");
                            rule.mode = "re".into();
                            rule.draft = built["draft"].clone();
                            rule.raw = false;
                        }
                        close = true;
                    }
                    if ui.button("取消").clicked() {
                        close = true;
                    }
                });
            });
        if self.template_changed.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if open && !close {
            self.template_form = Some(form);
        }
    }
    fn text_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.text_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        window("编辑文本", ctx, 780.)
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(&form.label);
                if form.has_batch {
                    ui.colored_label(
                        MUTED,
                        "此处编辑批量处理前的文本；保存后会再执行该条批量替换。",
                    );
                }
                ui.add_enabled_ui(!self.write_busy(), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(
                            (ctx.content_rect().height()
                                - 320.
                                - if form.show_original { 110. } else { 0. })
                            .clamp(120., 320.),
                        )
                        .show(ui, |ui| {
                            if ui
                                .add(
                                    egui::TextEdit::multiline(&mut form.value)
                                        .font(egui::TextStyle::Body)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(12),
                                )
                                .changed()
                            {
                                form.notice.clear();
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .button(if form.show_original {
                                "收起底稿原文"
                            } else {
                                "查看底稿原文"
                            })
                            .clicked()
                        {
                            form.show_original = !form.show_original;
                        }
                        if ui.button("恢复此条原文").clicked() {
                            form.value = form.original.clone();
                            form.error.clear();
                            form.notice = "已填回底稿原文，点击保存后生效。".into();
                        }
                    });
                    if form.show_original {
                        egui::ScrollArea::vertical()
                            .max_height(100.)
                            .show(ui, |ui| {
                                ui.label(&form.original);
                            });
                    }
                    if !form.notice.is_empty() {
                        ui.colored_label(MUTED, &form.notice);
                    }
                    if !form.error.is_empty() {
                        ui.colored_label(RED, &form.error);
                    }
                    ui.horizontal(|ui| {
                        if primary(ui, true, "保存修改").clicked() {
                            self.submit(
                                "text_save",
                                json!({"nonce":form.nonce,"text":form.value}),
                                "正在保存单条修改…",
                                false,
                            );
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                        ui.colored_label(MUTED, "保存后需预览、应用才会写入游戏。");
                    });
                });
            });
        if open && !close {
            self.text_form = Some(form);
        }
    }
    fn batch_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.batch_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        window(if form.group_id.is_empty(){"替换选中文本"}else{"编辑批量修改"},ctx,820.).open(&mut open).show(ctx,|ui|{ui.label(format!("仅修改选中的 {} 条文本",form.indices.len()));if !form.group_id.is_empty() && ui.button("查看文本范围，移除误选段落…").clicked(){self.open_batch_scope(&form.group_id);close=true;}ui.add_enabled_ui(!self.write_busy(),|ui|{
            ui.label("查找文字");let old=ui.add(egui::TextEdit::singleline(&mut form.old).font(egui::TextStyle::Body).desired_width(f32::INFINITY));ui.label("替换为");let new=ui.add(egui::TextEdit::singleline(&mut form.new).font(egui::TextStyle::Body).desired_width(f32::INFINITY));
            if old.changed()||new.changed(){form.nonce.clear();form.changes.clear();form.error.clear();}
            if ui.button("预览选中").clicked(){self.submit("batch_preview",json!({"group_id":form.group_id,"indices":form.indices,"search_nonce":text(&self.search,"nonce"),"old":form.old,"new":form.new}),"正在预览选中文本…",true);}
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height()*0.35).show(ui,|ui|{for change in &form.changes{ui.label(RichText::new(text(change,"label")).color(MUTED));ui.label(format!("当前：{}\n替换后：{}",text(change,"before"),text(change,"after")));ui.separator();}});
            if form.new.is_empty() && !form.old.is_empty(){ui.colored_label(MUTED,"留空将删除选中文本中的匹配文字。");}
            ui.colored_label(RED,&form.error);ui.horizontal(|ui|{if primary(ui,!form.nonce.is_empty()&&!form.changes.is_empty(),"保存修改").clicked(){self.submit("batch_save",json!({"nonce":form.nonce}),"正在保存选中文本修改…",false);}
if ui.button("取消").clicked(){close=true;}});
        });});
        if open && !close {
            self.batch_form = Some(form);
        }
    }
    fn batch_scope_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.batch_scope.take() else {
            return;
        };
        let state = self.state.clone();
        let members: Vec<_> = slice(&state, "edits")
            .iter()
            .filter(|e| form.contains(e))
            .collect();
        let mut open = true;
        let mut close = false;
        window("批量修改 · 文本范围",ctx,980.).open(&mut open).show(ctx,|ui| {
            ui.set_height((ctx.content_rect().height()-140.).clamp(360.,620.));
            if let Some(first)=members.first() {
                ui.add(egui::Label::new(format!("{} → {} · 共 {} 段",text(&first["edit"]["batch_operation"],"old"),text(&first["edit"]["batch_operation"],"new"),members.len())).truncate());
            } else {ui.label("此批已没有文本段。");}
            record_filter(ui, "batch-scope", &mut form.filter, &mut form.selected);
            egui::Panel::bottom("batch-scope-actions").show(ui,|ui| {
                if !form.error.is_empty(){ui.colored_label(RED,&form.error);}
                let selected=members.iter().find(|e|Some(count(e,"index") as usize)==form.selected);
                ui.horizontal(|ui| {
                    if secondary(ui, selected.is_some()&&!self.write_busy(),"从本批移除此段").clicked()
                        && let Some(entry)=selected {
                        self.submit("remove_batch_member",json!({"id":form.id,"index":entry["index"],"revision":self.revision()}),"正在从本批移除这段文本…",false);
                    }
                    if ui.button("关闭").clicked(){close=true;}
                });
                ui.colored_label(MUTED,"只取消该段的本批修改，保留已有单条修改。预览并应用后写入游戏。");
                ui.colored_label(MUTED,"关闭后可用主界面的“撤销”恢复移除。");
            });
            record_browser(ui,"batch-scope",&members,&form.filter,&mut form.selected,true);
        });
        if open && !close {
            self.batch_scope = Some(form);
        }
    }
    fn source_notices_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_source_notices {
            return;
        }
        let notices = list(&self.state, "source_notices");
        if notices.is_empty() {
            self.show_source_notices = false;
            return;
        }
        let mut open = true;
        let mut edit = None;
        window("新增来源表 · 词条命中", ctx, 860.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("以下词条在新增来源表中命中，尚未加入替换范围。");
                egui::ScrollArea::vertical()
                    .id_salt("new-source-notices")
                    .max_height((ctx.content_rect().height() - 240.).max(180.))
                    .show(ui, |ui| {
                        for notice in &notices {
                            ui.push_id((text(notice, "table"), count(notice, "index")), |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!(
                                        "{} · {} 段",
                                        text(notice, "label"),
                                        count(notice, "fields")
                                    ))
                                    .on_hover_text(text(notice, "table"));
                                    if secondary(ui, !self.write_busy(), "编辑词条").clicked() {
                                        edit = Some(count(notice, "index") as usize);
                                    }
                                });
                                ui.add(
                                    egui::Label::new(format!(
                                        "{} → {}（{}）",
                                        text(notice, "old"),
                                        if text(notice, "new").is_empty() {
                                            "删除匹配文字".into()
                                        } else {
                                            text(notice, "new")
                                        },
                                        mode_name(&text(notice, "mode"))
                                    ))
                                    .wrap(),
                                );
                                ui.colored_label(
                                    MUTED,
                                    text(notice, "example").replace(['\r', '\n'], " "),
                                );
                                ui.separator();
                            });
                        }
                    });
                if secondary(ui, !self.write_busy(), "已读").clicked() {
                    self.submit(
                        "sources_ack",
                        json!({"revision":self.revision()}),
                        "正在保存提醒状态…",
                        false,
                    );
                }
            });
        if let Some(index) = edit {
            let rules = list(&self.state, "rules");
            if let Some(rule) = rules.get(index) {
                self.rule_form = Some(RuleForm::new(Some(index), self.revision(), Some(rule)));
            }
            self.show_source_notices = false;
        } else if !open {
            self.show_source_notices = false;
        }
    }
    fn preview_dialog(&mut self, ctx: &egui::Context) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let stats = &preview["stats"];
        let changes = slice(stats, "changes");
        window("预览替换", ctx, 940.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!(
                    "本次 {} 处变化：新增 {}，调整 {}，撤销 {}",
                    count(&preview, "total_changes"),
                    count(stats, "added"),
                    count(stats, "updated"),
                    count(stats, "reverted")
                ));
                let widths = [
                    55.,
                    150.,
                    (ui.available_width() - 260.) * 0.5,
                    (ui.available_width() - 260.) * 0.5,
                ];
                table_header(
                    ui,
                    &["变化", "来源表", "当前游戏文字", "本次最终文字"],
                    &widths,
                );
                egui::ScrollArea::vertical()
                    .id_salt("preview-changes")
                    .max_height(ctx.content_rect().height() * 0.33)
                    .show_rows(ui, ROW_HEIGHT, changes.len(), |ui, range| {
                        for i in range {
                            let change = &changes[i];
                            let table = text(change, "table");
                            let label = self
                                .tables
                                .iter()
                                .find(|v| text(v, "name") == table)
                                .map(|v| text(v, "label"))
                                .unwrap_or(table);
                            if data_row(
                                ui,
                                i,
                                self.preview_row == Some(i),
                                &[
                                    text(change, "kind"),
                                    label,
                                    text(change, "before").replace('\n', " [换行] "),
                                    text(change, "after").replace('\n', " [换行] "),
                                ],
                                &widths,
                            )
                            .clicked()
                            {
                                self.preview_row = Some(i);
                            }
                        }
                    });
                if changes.is_empty() {
                    ui.colored_label(MUTED, "当前游戏文字无需更改。");
                }
                if count(&preview, "total_changes") > changes.len() as u64 {
                    ui.colored_label(
                        MUTED,
                        format!("显示前 {} 处，上方统计包含全部变化。", changes.len()),
                    );
                }
                if let Some(row) = self.preview_row.and_then(|i| changes.get(i)) {
                    egui::ScrollArea::vertical()
                        .id_salt("preview-text-detail")
                        .max_height(110.)
                        .show(ui, |ui| {
                            ui.label(format!(
                                "底稿原文：{}\n当前游戏：{}\n本次结果：{}",
                                text(row, "original"),
                                text(row, "before"),
                                text(row, "after")
                            ));
                        });
                }
                let misses = list(stats, "zero_reasons");
                if !misses.is_empty() {
                    egui::CollapsingHeader::new(format!("未命中词条（{}）", misses.len())).show(
                        ui,
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("preview-missed-rules")
                                .max_height(100.)
                                .show(ui, |ui| {
                                    for miss in misses {
                                        let array = miss.as_array().cloned().unwrap_or_default();
                                        ui.label(
                                            if let Some(reason) =
                                                array.get(1).and_then(Value::as_str)
                                            {
                                                format!(
                                                    "第 {} 条：{}",
                                                    array
                                                        .first()
                                                        .and_then(Value::as_u64)
                                                        .unwrap_or(0)
                                                        + 1,
                                                    reason
                                                )
                                            } else {
                                                text(&miss, "reason")
                                            },
                                        );
                                    }
                                });
                        },
                    );
                }
                let diagnostics = list(stats, "diagnostics");
                if !diagnostics.is_empty() {
                    if count(stats, "parse_failed") == 0 {
                        let mut skipped = Vec::new();
                        if count(stats, "tables_skipped") > 0 {
                            skipped.push(format!("{} 张来源表", count(stats, "tables_skipped")));
                        }
                        if count(stats, "fields_skipped") > 0 {
                            skipped.push(format!("{} 个异常字段", count(stats, "fields_skipped")));
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
                                    let location =
                                        bajptr_project::model::title(&text(diagnostic, "table"));
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
                            ui.colored_label(
                                MUTED,
                                "正在后台准备补丁，可先核对预览；关闭窗口会保留本次准备。",
                            );
                        });
                    } else if ready {
                        ui.colored_label(
                            MUTED,
                            if preview["prepared_reused"] == true {
                                "已复用准备好的补丁，确认后应用。"
                            } else {
                                "补丁已准备，确认后应用。"
                            },
                        );
                    } else {
                        if !preparation_error.is_empty() {
                            ui.colored_label(RED, &preparation_error);
                        }
                        if secondary(ui, self.busy.is_none(), "重新准备补丁").clicked() {
                            self.submit(
                                "prepare_patch",
                                json!({"nonce":text(&preview,"nonce")}),
                                "正在后台准备补丁…",
                                true,
                            );
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if primary(
                        ui,
                        count(&preview, "total_changes") > 0
                            && count(stats, "parse_failed") == 0
                            && ready
                            && self.busy.is_none(),
                        "应用到游戏",
                    )
                    .clicked()
                    {
                        self.submit(
                            "apply",
                            json!({"nonce":text(&preview,"nonce"),"prepared":true}),
                            "正在校验并应用准备好的补丁…",
                            true,
                        );
                        close = true;
                    }
                    if ui
                        .button(if count(&preview, "total_changes") > 0 {
                            "取消"
                        } else {
                            "关闭"
                        })
                        .clicked()
                    {
                        close = true;
                    }
                    ui.colored_label(MUTED, "请先退出游戏。未保留的旧规则会撤销。");
                });
            });
        if !open || close {
            self.preview = None;
        }
    }
    fn edit_manager(&mut self, ctx: &egui::Context) {
        if !self.show_edits {
            return;
        }
        let state = self.state.clone();
        let edits: Vec<_> = slice(&state, "edits")
            .iter()
            .filter(|e| e["edit"]["batch"] != true)
            .collect();
        let mut open = true;
        let mut close = false;
        if edits.len() <= 1 {
            self.edit_view.filter.clear();
        }
        let visible: Vec<_> = edits
            .iter()
            .copied()
            .filter(|entry| edit_matches(entry, &self.edit_view.filter))
            .collect();
        if !visible
            .iter()
            .any(|entry| Some(count(entry, "index") as usize) == self.edit_view.selected)
        {
            self.edit_view.selected = visible.first().map(|entry| count(entry, "index") as usize);
        }
        window(
            "单条修改",
            ctx,
            if edits.is_empty() {
                500.
            } else if edits.len() > 1 {
                980.
            } else {
                780.
            },
        )
        .id(egui::Id::unique("single-edit-manager-compact"))
        .min_width(500.)
        .open(&mut open)
        .show(ctx, |ui| {
            let text_width = ((ui.available_width() - 20.) * 0.5).max(200.);
            let text_height = edits
                .first()
                .map(|entry| {
                    ["original", "replacement"]
                        .iter()
                        .map(|key| {
                            ui.painter()
                                .layout(
                                    text(&entry["edit"], key),
                                    egui::FontId::proportional(16.),
                                    TEXT,
                                    text_width,
                                )
                                .size()
                                .y
                        })
                        .fold(0., f32::max)
                })
                .unwrap_or(0.);
            let content_height = if edits.is_empty() {
                90.
            } else if edits.len() == 1 {
                (text_height + 120.).max(180.)
            } else {
                430.
            };
            let content_height = content_height.min((ctx.content_rect().height() - 160.).max(180.));
            ui.set_height(content_height);
            ui.add_enabled_ui(self.text_form.is_none() && !self.write_busy(), |ui| {
                if edits.len() > 1 {
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 95.).max(160.);
                        if ui
                            .add_sized(
                                [width, 34.],
                                egui::TextEdit::singleline(&mut self.edit_view.filter)
                                    .hint_text("搜索原文、替换结果或来源表"),
                            )
                            .changed()
                        {
                            self.edit_view.selected = None;
                        }
                        ui.colored_label(MUTED, format!("{} / {} 条", visible.len(), edits.len()));
                    });
                }
                egui::Panel::bottom("single-edit-actions").show(ui, |ui| {
                    let selected = edits
                        .iter()
                        .find(|e| Some(count(e, "index") as usize) == self.edit_view.selected);
                    ui.horizontal(|ui| {
                        if selected.is_some() {
                            if primary(
                                ui,
                                selected.is_some_and(|e| e["active"] == true),
                                "编辑文本",
                            )
                            .clicked()
                                && let Some(entry) = selected
                            {
                                self.submit(
                                    "text_open",
                                    json!({"edit_index":entry["index"]}),
                                    "正在读取单条修改…",
                                    true,
                                );
                            }
                            ui.add_space(12.);
                            if secondary(ui, selected.is_some(), "移除单条修改").clicked()
                                && let Some(entry) = selected
                            {
                                self.submit(
                                    "remove_edit",
                                    json!({"index":entry["index"],"revision":self.revision()}),
                                    "正在移除单条修改…",
                                    false,
                                );
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if secondary(ui, true, "关闭").clicked() {
                                close = true;
                            }
                        });
                    });
                });
                if edits.is_empty() {
                    ui.label("尚未保存单条修改。可在「查找」双击文本开始编辑。");
                }
                if !edits.is_empty() {
                    single_edit_browser(
                        ui,
                        &visible,
                        &mut self.edit_view.selected,
                        edits.len() > 1,
                    );
                }
            });
        });
        self.show_edits = open && !close;
    }
    fn confirmations(&mut self, ctx: &egui::Context) {
        if let Some(candidate) = self.confirm_baseline.clone() {
            let state = text(&candidate, "state");
            let first = state == "missing";
            let mut open = true;
            let mut close = false;
            window(if first {"设置初始底稿"} else {"更新初始底稿"},ctx,540.)
                .open(&mut open)
                .show(ctx,|ui| {
                    ui.label(match state.as_str() {
                        "missing" => "还没有初始底稿。将当前游戏文本保存为初始底稿？",
                        "changed" => "当前游戏资源与初始底稿不同。将当前文本设为新的初始底稿？",
                        _ => "检测到游戏资源版本更新。将当前游戏文本保存为新版初始底稿？",
                    });
                    ui.add_space(8.);
                    ui.colored_label(MUTED,format!("当前资源：{}",text(&candidate,"game_version")));
                    ui.add_space(8.);
                    ui.label("会包含当前已安装的汉化或其他修改。请先退出游戏，并确认这是要作为替换起点的文本。");
                    if !first {
                        ui.colored_label(MUTED,"新底稿校验成功后才移出旧底稿；词表和修改记录会保留。");
                    }
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if primary(ui,!self.write_busy(),if first {"设为初始底稿"} else {"更新底稿"}).clicked() {
                            self.submit("baseline_commit",json!({"token":candidate["token"]}),"正在保存并校验初始底稿…",false);
                            close = true;
                        }
                        if ui.button(if first {"暂不设置"} else {"暂不更新"}).clicked() { close = true; }
                    });
                });
            if !open || close {
                self.confirm_baseline = None;
                if !self.write_busy() {
                    self.status = "尚未确认底稿，点击“检查底稿”继续。".into();
                }
            }
        }
        if let Some(import) = self.confirm_import.clone() {
            let mut open = true;
            let mut close = false;
            window("导入词表", ctx, 500.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "用 {} 条导入词条替换当前词表？",
                        count(&import, "count")
                    ));
                    ui.horizontal(|ui| {
                        if primary(ui, !self.write_busy(), "导入替换").clicked() {
                            self.submit("import_commit", import.clone(), "正在导入词表…", false);
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
            if !open || close {
                self.confirm_import = None;
            }
        }
        if self.confirm_restore {
            let mut open = true;
            let mut close = false;
            window("恢复初始底稿", ctx, 520.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        "恢复当前版本的底稿文字；保留词表、单条及批量修改记录。请先退出游戏。",
                    );
                    ui.horizontal(|ui| {
                        if primary(ui, !self.write_busy(), "恢复底稿").clicked() {
                            self.submit("restore", json!({}), "正在恢复初始底稿…", false);
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
            if !open || close {
                self.confirm_restore = false;
            }
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn selected_batch_details_stay_above_footer_at_minimum_size() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let entry = json!({"index":0,"active":true,"label":"技能说明","edit":{"table":"Skill","row_id":1,"slot":0,"original":"原文".repeat(300),"replacement":"结果\n".repeat(100),"batch_operation":{"before":"本批之前\n".repeat(100)}}});
        let mut selected = Some(0);
        for frame in 0..3 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 620.),
                    )),
                    time: Some(f64::from(frame)),
                    ..Default::default()
                },
                |ui| {
                    let area =
                        egui::Rect::from_min_size(egui::pos2(18., 90.), egui::vec2(828., 400.));
                    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(area));
                    record_browser(
                        &mut body,
                        "minimum-layout",
                        &[&entry],
                        "",
                        &mut selected,
                        true,
                    );
                    assert!(
                        body.min_rect().bottom() <= area.bottom() + 1.,
                        "detail overlaps footer: {} > {}",
                        body.min_rect().bottom(),
                        area.bottom()
                    );
                },
            );
        }
    }
}
