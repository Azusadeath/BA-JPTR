use crate::{app::*, state::*};
use eframe::egui::{self, RichText};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

fn record_name(name: &str) -> &str {
    match name {
        "rules.tsv" => "替换词表",
        "text-edits.json" => "单条与批量修改",
        "source-history.json" => "来源表记录",
        "application-state.json" => "应用状态记录",
        _ => name,
    }
}

pub(crate) fn window<'a>(title: &'a str, ctx: &egui::Context, width: f32) -> egui::Window<'a> {
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

fn about_link(ui: &mut egui::Ui, label: &str, url: &str) {
    let response = ui.add_sized(
        [ui.available_width(), 42.],
        egui::Button::new("")
            .left_text(RichText::new(label).size(16.))
            .right_text(RichText::new("›").size(22.).color(MUTED))
            .frame_when_inactive(false),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, ui.is_enabled(), label));
    if response.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
    response.on_hover_text(url);
}

// Share text filtering and full-text presentation without forcing both managers
// to use the same list/detail layout.
pub(crate) fn record_filter(
    ui: &mut egui::Ui,
    id: &str,
    filter: &mut String,
    selected: &mut Option<usize>,
) {
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

pub(crate) struct ComparisonRow<'a> {
    pub index: usize,
    pub label: String,
    pub original: &'a str,
    pub before: &'a str,
    pub after: &'a str,
    pub active: bool,
}

pub(crate) fn comparison_browser(
    ui: &mut egui::Ui,
    id: &str,
    rows: &[ComparisonRow<'_>],
    selected: &mut Option<usize>,
    labels: [&str; 2],
    mut multi: Option<&mut Selection>,
) {
    ui.push_id(id, |ui| {
        let visible = rows.iter().map(|row| row.index).collect::<Vec<_>>();
        if let Some(multi) = multi.as_mut() {
            let indices = visible.iter().copied().collect::<BTreeSet<_>>();
            multi.rows.retain(|index| indices.contains(index));
            if multi.anchor.is_some_and(|index| !indices.contains(&index)) {
                multi.anchor = None;
            }
        }
        if !rows.iter().any(|row| Some(row.index) == *selected) {
            *selected = rows.first().map(|row| row.index);
        }
        if rows.is_empty() {
            ui.colored_label(MUTED, "没有匹配文本。");
            ui.take_available_space();
            return;
        }
        egui::Panel::left("comparison-list")
            .resizable(true)
            .default_size(200.)
            .size_range(150. ..=280.)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("rows")
                    .auto_shrink([false, false])
                    .show_rows(ui, 56., rows.len(), |ui, range| {
                        for i in range {
                            let row = &rows[i];
                            let is_selected = multi.as_ref().map_or_else(
                                || *selected == Some(row.index),
                                |multi| multi.rows.contains(&row.index),
                            );
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 56.),
                                egui::Sense::click(),
                            );
                            if is_selected {
                                ui.painter().rect_filled(rect, 6, SOFT_BLUE);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 6, BG);
                            }
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    ui.is_enabled(),
                                    is_selected,
                                    format!("{}：{} → {}", row.label, row.before, row.after),
                                )
                            });
                            for (offset, value, color) in
                                [(6., row.before, TEXT), (30., row.after, BLUE)]
                            {
                                let mut line = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(egui::Rect::from_min_size(
                                            rect.min + egui::vec2(8., offset),
                                            egui::vec2(rect.width() - 16., 20.),
                                        ))
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                line.add(
                                    egui::Label::new(
                                        RichText::new(value.replace(['\r', '\n'], " "))
                                            .color(color),
                                    )
                                    .selectable(false)
                                    .truncate(),
                                );
                            }
                            if response.clicked() {
                                if let Some(multi) = multi.as_mut() {
                                    let modifiers = ui.input(|input| input.modifiers);
                                    multi.click_visible(
                                        row.index,
                                        &visible,
                                        modifiers.ctrl || modifiers.command,
                                        modifiers.shift,
                                    );
                                }
                                *selected = Some(row.index);
                            }
                        }
                    });
            });
        if let Some(row) = rows.iter().find(|row| Some(row.index) == *selected) {
            ui.colored_label(MUTED, &row.label);
            if !row.active {
                ui.colored_label(RED, "版本失效：可查看或移除，不能继续编辑。");
            }
            if row.original != row.before {
                egui::CollapsingHeader::new("底稿原文").show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("original", row.index))
                        .min_scrolled_height(0.)
                        .max_height(80.)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(row.original).wrap().selectable(true));
                        });
                });
            }
            let height = ui.available_height().max(0.);
            ui.push_id(row.index, |ui| {
                if ui.available_width() >= 680. {
                    ui.columns(2, |columns| {
                        full_text(&mut columns[0], "before", labels[0], row.before, height);
                        full_text(&mut columns[1], "after", labels[1], row.after, height);
                    });
                } else {
                    let half = (height - ui.spacing().item_spacing.y) * 0.5;
                    full_text(ui, "before", labels[0], row.before, half);
                    full_text(ui, "after", labels[1], row.after, half);
                }
            });
        }
    });
}

pub(crate) fn record_browser(
    ui: &mut egui::Ui,
    id: &str,
    entries: &[&Value],
    filter: &str,
    selected: &mut Option<usize>,
    batch: bool,
    multi: Option<&mut Selection>,
) {
    let rows = entries
        .iter()
        .filter(|entry| edit_matches(entry, filter))
        .map(|entry| {
            let edit = &entry["edit"];
            let original = edit["original"].as_str().unwrap_or("");
            ComparisonRow {
                index: count(entry, "index") as usize,
                label: text(entry, "label"),
                original,
                before: if batch {
                    edit["batch_operation"]["before"]
                        .as_str()
                        .unwrap_or(original)
                } else {
                    original
                },
                after: edit["replacement"].as_str().unwrap_or(""),
                active: entry["active"] != false,
            }
        })
        .collect::<Vec<_>>();
    ui.colored_label(MUTED, format!("显示 {} / {} 段", rows.len(), entries.len()));
    comparison_browser(
        ui,
        id,
        &rows,
        selected,
        if batch {
            ["替换前", "替换后"]
        } else {
            ["底稿原文", "修改后"]
        },
        multi,
    );
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

fn single_edit_text(ui: &mut egui::Ui, id: &str, label: &str, value: &str, height: f32) {
    let frame = egui::Frame::new()
        .fill(egui::Color32::WHITE)
        .stroke(egui::Stroke::new(1., LINE))
        .corner_radius(8)
        .inner_margin(14);
    let margin = frame.total_margin().sum();
    let size = egui::vec2(ui.available_width() - margin.x, height - margin.y).max(egui::Vec2::ZERO);
    frame.show(ui, |ui| {
        ui.set_min_size(size);
        let heading = ui.label(RichText::new(label).size(13.).color(MUTED));
        let remaining = (size.y - heading.rect.height() - ui.spacing().item_spacing.y).max(0.);
        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .min_scrolled_height(0.)
            .max_height(remaining)
            .show(ui, |ui| {
                ui.add(egui::Label::new(value).wrap().selectable(true));
            });
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
                                let summary = text(&entry["edit"], "original")
                                    .replace(['\r', '\n'], " ")
                                    .chars()
                                    .take(80)
                                    .collect::<String>();
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
            if entry["active"] == false {
                ui.colored_label(RED, "版本失效：可查看或移除，不能继续编辑。");
            }
            let height = ui.available_height().max(0.);
            let original = text(&entry["edit"], "original");
            let replacement = text(&entry["edit"], "replacement");
            if ui.available_width() >= 560. {
                ui.columns(2, |columns| {
                    single_edit_text(&mut columns[0], "original", "底稿原文", &original, height);
                    single_edit_text(
                        &mut columns[1],
                        "replacement",
                        "修改后",
                        &replacement,
                        height,
                    );
                });
            } else {
                let half = (height - ui.spacing().item_spacing.y) * 0.5;
                single_edit_text(ui, "original", "底稿原文", &original, half);
                single_edit_text(ui, "replacement", "修改后", &replacement, half);
            }
        } else {
            ui.colored_label(MUTED, "没有匹配文本。");
        }
    });
}

impl Desktop {
    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        window("关于", ctx, 520.)
            .id(egui::Id::unique("about-product-information-compact"))
            .min_width(400.)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("about-content")
                    .auto_shrink([false, true])
                    .max_height((ctx.content_rect().height() - 140.).max(220.))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.;
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(249, 251, 254))
                            .stroke(egui::Stroke::new(1., LINE))
                            .corner_radius(12)
                            .inner_margin(18)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(RichText::new("BA-JPTR").size(24.).strong().color(TEXT));
                                ui.colored_label(
                                    MUTED,
                                    format!(
                                        "版本 {}（构建于 {}）",
                                        env!("CARGO_PKG_VERSION"),
                                        env!("BAJPTR_BUILD_TIME")
                                    ),
                                );
                                ui.colored_label(
                                    MUTED,
                                    "在已有汉化基础上，自定义蔚蓝档案日服 PC 文本。",
                                );
                                ui.add_space(6.);
                                ui.horizontal_wrapped(|ui| {
                                    if primary(ui, true, "查看最新版本").clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(
                                            "https://github.com/Azusadeath/BA-JPTR/releases/latest",
                                        ));
                                    }
                                    if secondary(ui, true, "使用说明").clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(
                                            "https://github.com/Azusadeath/BA-JPTR#readme",
                                        ));
                                    }
                                });
                            });
                        ui.add_space(6.);
                        ui.label(RichText::new("链接").strong().color(TEXT));
                        about_link(ui, "GitHub 仓库", "https://github.com/Azusadeath/BA-JPTR");
                        ui.separator();
                        about_link(
                            ui,
                            "历史版本",
                            "https://github.com/Azusadeath/BA-JPTR/releases",
                        );
                        ui.separator();
                        about_link(
                            ui,
                            "开源许可",
                            "https://github.com/Azusadeath/BA-JPTR/blob/main/LICENSE",
                        );
                        ui.add_space(12.);
                        egui::CollapsingHeader::new("构建与运行信息").show(ui, |ui| {
                            egui::Grid::new("about-build-metadata")
                                .num_columns(2)
                                .spacing(egui::vec2(18., 10.))
                                .show(ui, |ui| {
                                    for (label, value) in [
                                        ("源码版本", env!("BAJPTR_BUILD_COMMIT")),
                                        ("构建目标", env!("BAJPTR_BUILD_TARGET")),
                                        ("构建模式", env!("BAJPTR_BUILD_PROFILE")),
                                        ("Rust", env!("BAJPTR_BUILD_RUSTC")),
                                        ("开源许可", env!("CARGO_PKG_LICENSE")),
                                        ("运行平台", std::env::consts::OS),
                                        ("运行架构", std::env::consts::ARCH),
                                    ] {
                                        if !value.is_empty() {
                                            ui.colored_label(MUTED, label);
                                            ui.add(egui::Label::new(value).wrap().selectable(true));
                                            ui.end_row();
                                        }
                                    }
                                });
                        });
                    });
            });
        self.show_about = open;
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let before = [
            self.rule_form.is_some(),
            self.template_form.is_some(),
            self.text_form.is_some(),
            self.batch_form.is_some(),
            self.combined_form.is_some(),
        ];
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !self.write_busy() {
            if self.table_detail.is_some() {
                self.table_detail = None;
            } else if self.error.is_some() {
                self.error = None;
            } else if self.confirm_record_reset.is_some() {
                self.confirm_record_reset = None;
            } else if self.show_about {
                self.show_about = false;
            } else if self.template_form.is_some() {
                self.template_form = None;
            } else if self.scope_form.is_some() {
                self.scope_form = None;
            } else if self.combined_form.is_some() {
                self.combined_form = None;
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
            } else if self.confirm_backup.is_some() {
                self.confirm_backup = None;
            } else if self.migration_form.as_ref().is_some_and(|form| form.open) {
                if let Some(form) = &mut self.migration_form {
                    form.open = false;
                }
            } else if self.migration_notice {
                self.migration_notice = false;
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
        self.combined_dialog(ctx);
        self.scope_dialog(ctx);
        self.template_dialog(ctx);
        self.text_dialog(ctx);
        self.batch_dialog(ctx);
        self.batch_scope_dialog(ctx);
        self.preview_dialog(ctx);
        self.source_notices_dialog(ctx);
        self.edit_manager(ctx);
        self.confirmations(ctx);
        self.migration_dialogs(ctx);
        self.about_dialog(ctx);
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
                .map(crate::logging::Entry::formatted)
                .max_by_key(|line| line.chars().count())
                .unwrap_or_default();
            let font = egui::TextStyle::Monospace.resolve(&ctx.style_of(egui::Theme::Light));
            let width = ctx
                .fonts_mut(|fonts| fonts.layout_no_wrap(longest, font, TEXT).size().x + 12.)
                .clamp(440., (ctx.content_rect().width() - 80.).clamp(440., 800.));
            window("查看日志", ctx, width)
                .id(egui::Id::unique("run-details-content-width"))
                .min_width(280.)
                .max_width(width)
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.add_space(4.);
                    let log_height = (ui.available_height() - 56.)
                        .min(ctx.content_rect().height() - 236.)
                        .max(80.);
                    egui::ScrollArea::vertical()
                        .id_salt("log-content")
                        .auto_shrink([false, true])
                        .max_height(log_height)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(8, 10))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 5.;
                                    if self.logs.is_empty() {
                                        ui.colored_label(MUTED, "暂无日志");
                                    }
                                    for entry in &self.logs {
                                        let color = match entry.level {
                                            crate::logging::Level::Info => TEXT,
                                            crate::logging::Level::Warning => ORANGE,
                                            crate::logging::Level::Error => RED,
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(entry.formatted())
                                                    .monospace()
                                                    .color(color),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    }
                                });
                        });
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        if secondary(ui, !self.logs.is_empty(), "复制全部").clicked() {
                            ctx.copy_text(self.log_text());
                        }
                        if secondary(ui, !self.logs.is_empty(), "导出日志").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("日志文本", &["txt"])
                                .set_file_name("bajptr-log.txt")
                                .save_file()
                            && let Err(error) = self.export_logs(&path)
                        {
                            self.report_file_error("export_logs", &path, error);
                        }
                    });
                });
            self.show_logs = open;
        }
        if let Some(message) = self.error.clone() {
            let recovery = self
                .error_recovery
                .as_ref()
                .filter(|(target, _)| target == &message)
                .map(|(_, recovery)| *recovery);
            let errors = self.state["errors"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, error)| (name.clone(), error.as_str().unwrap_or("").to_string()))
                .collect::<Vec<_>>();
            let mut open = true;
            let mut close = false;
            let mut reconnect = false;
            let title = match recovery {
                Some(ErrorRecovery::Baseline) => "底稿异常",
                Some(ErrorRecovery::Records) => "记录异常",
                None => "操作失败",
            };
            window(title, ctx, 520.)
                .id(egui::Id::unique("operation-error"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.add(egui::Label::new(message).wrap());
                    ui.add_space(12.);
                    match recovery {
                        Some(ErrorRecovery::Baseline) => {
                            ui.horizontal_wrapped(|ui| {
                                if secondary(ui, self.data_path.is_some(), "打开备份位置").clicked()
                                {
                                    self.open_data_folder("backup");
                                }
                                if primary(
                                    ui,
                                    self.loaded
                                        && self.busy.is_none()
                                        && !self.path_dirty
                                        && !self.game.is_empty(),
                                    "尝试重建",
                                )
                                .clicked()
                                {
                                    self.submit(
                                        "baseline_rebuild_preview",
                                        json!({}),
                                        "正在查找并校验可用于重建的原始资源…",
                                    );
                                    close = true;
                                }
                            });
                        }
                        Some(ErrorRecovery::Records) => {
                            if secondary(ui, self.data_path.is_some(), "打开数据目录").clicked()
                            {
                                self.open_data_folder(".gui");
                            }
                            for (name, error) in &errors {
                                ui.push_id(name, |ui| {
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(record_name(name))
                                                .on_hover_text(format!("{name}\n{error}"));
                                            if [
                                                "rules.tsv",
                                                "text-edits.json",
                                                "source-history.json",
                                            ]
                                            .contains(&name.as_str())
                                                && action(
                                                    ui,
                                                    self.loaded && self.busy.is_none(),
                                                    "清空记录…",
                                                    ButtonKind::Soft,
                                                )
                                                .clicked()
                                            {
                                                self.confirm_record_reset = Some(name.clone());
                                                close = true;
                                            }
                                        })
                                    })
                                });
                            }
                        }
                        None => {}
                    }
                    if self.backend.is_none() && primary(ui, true, "重新连接").clicked() {
                        reconnect = true;
                    }
                    if action(
                        ui,
                        true,
                        "知道了",
                        if recovery.is_some() || self.backend.is_none() {
                            ButtonKind::Standard
                        } else {
                            ButtonKind::Primary
                        },
                    )
                    .clicked()
                    {
                        close = true;
                    }
                    if ui.button("查看日志").clicked() {
                        self.show_logs = true;
                    }
                });
            if !open || close {
                self.error = None;
                self.error_recovery = None;
            }
            if reconnect {
                self.reconnect(ctx);
            }
        }
        let after = [
            self.rule_form.is_some(),
            self.template_form.is_some(),
            self.text_form.is_some(),
            self.batch_form.is_some(),
            self.combined_form.is_some(),
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
                            for mode in ["sub", "exact", "re"] {
                                ui.selectable_value(&mut form.mode, mode.into(), mode_name(mode));
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
                        self.submit("save_rule", form.payload(), "正在保存词条…");
                    }
                });
            });
        });
        if open {
            self.rule_form = Some(form);
        }
    }
    fn scope_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.scope_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        window("选择来源表", ctx, 480.)
            .id(egui::Id::unique("source-picker-compact"))
            .min_width(360.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.filter)
                            .font(egui::TextStyle::Body)
                            .hint_text("筛选中文名称或表名")
                            .desired_width((ui.available_width() - 85.).max(140.)),
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
                    .min_scrolled_height(0.)
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
                        let scope = if self.combined_form.is_some()
                            && self.rule_form.is_none()
                            && form.checked.is_empty()
                        {
                            Ok("[]".into())
                        } else {
                            form.encode()
                        };
                        match scope {
                            Ok(scope) => {
                                if let Some(rule) = &mut self.rule_form {
                                    rule.scope = scope;
                                } else if let Some(combined) = &mut self.combined_form {
                                    combined.scope = scope;
                                }
                                close = true;
                            }
                            Err(error) => form.error = error,
                        }
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
        window("正则模板", ctx, 560.)
            .id(egui::Id::unique("regex-template-compact"))
            .min_width(400.)
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
        window("编辑文本", ctx, 600.)
            .id(egui::Id::unique("text-editor-compact"))
            .min_width(400.)
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.write_busy(), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("text-editor-body")
                        .min_scrolled_height(0.)
                        .max_height((ctx.content_rect().height() - 180.).clamp(160., 520.))
                        .show(ui, |ui| {
                            ui.colored_label(MUTED, &form.label);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("编辑内容").strong());
                                if form.value != form.initial {
                                    ui.colored_label(ORANGE, "未保存");
                                }
                            });
                            egui::ScrollArea::vertical()
                                .id_salt("text-editor-input")
                                .min_scrolled_height(0.)
                                .max_height(if form.batch.is_some() { 140. } else { 240. })
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut form.value)
                                            .font(egui::TextStyle::Body)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(2),
                                    );
                                });
                            if let Some((old, new)) = &form.batch {
                                ui.horizontal_wrapped(|ui| {
                                    ui.colored_label(MUTED, "批量替换");
                                    ui.label(old);
                                    ui.colored_label(MUTED, "→");
                                    ui.label(if new.is_empty() {
                                        "（删除匹配文字）"
                                    } else {
                                        new
                                    });
                                });
                                ui.label(RichText::new("替换后的文字").strong());
                                egui::ScrollArea::vertical()
                                    .id_salt("text-editor-result")
                                    .min_scrolled_height(0.)
                                    .max_height(120.)
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(form.final_text())
                                                .wrap()
                                                .selectable(true),
                                        );
                                    });
                            }
                            egui::CollapsingHeader::new("底稿原文").show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("text-editor-original")
                                    .min_scrolled_height(0.)
                                    .max_height(100.)
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(&form.original)
                                                .wrap()
                                                .selectable(true),
                                        );
                                    });
                            });
                        });
                    if !form.error.is_empty() {
                        ui.colored_label(RED, &form.error);
                    }
                    ui.horizontal(|ui| {
                        if secondary(ui, form.value != form.original, "填回原文").clicked() {
                            form.value = form.original.clone();
                            form.error.clear();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if primary(ui, true, "保存修改").clicked() {
                                self.submit(
                                    "text_save",
                                    json!({"nonce":form.nonce,"text":form.value}),
                                    "正在保存单条修改…",
                                );
                            }
                        });
                    });
                });
            });
        if open {
            self.text_form = Some(form);
        }
    }
    fn batch_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.batch_form.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        window(if form.group_id.is_empty(){"替换选中文本"}else{"编辑批量修改"},ctx,600.)
            .id(egui::Id::unique("batch-editor-compact")).min_width(400.)
            .open(&mut open).show(ctx,|ui| {
                ui.label(format!("仅修改选中的 {} 条文本",form.indices.len()));
                if !form.group_id.is_empty() && ui.button("管理所选文本…").clicked() {
                    self.open_batch_scope(&form.group_id);
                    close=true;
                }
                ui.add_enabled_ui(!self.write_busy(),|ui| {
                    ui.label("查找文字");
                    let old=ui.add(egui::TextEdit::singleline(&mut form.old)
                        .font(egui::TextStyle::Body).desired_width(f32::INFINITY));
                    ui.label("替换为");
                    let new=ui.add(egui::TextEdit::singleline(&mut form.new)
                        .font(egui::TextStyle::Body).desired_width(f32::INFINITY));
                    if old.changed() || new.changed() {
                        form.nonce.clear();form.changes.clear();form.error.clear();
                    }
                    if ui.button("预览选中").clicked() {
                        self.submit("batch_preview",json!({"group_id":form.group_id,"indices":form.indices,"search_nonce":text(&self.search,"nonce"),"old":form.old,"new":form.new}),"正在预览选中文本…");
                    }
                    if !form.changes.is_empty() {
                        egui::ScrollArea::vertical().id_salt("batch-preview-changes")
                            .min_scrolled_height(0.).max_height(ctx.content_rect().height()*0.35)
                            .show(ui,|ui| {
                                for change in &form.changes {
                                    ui.label(RichText::new(text(change,"label")).color(MUTED));
                                    ui.label(format!("当前：{}\n替换后：{}",text(change,"before"),text(change,"after")));
                                    ui.separator();
                                }
                            });
                    }
                    if form.new.is_empty() && !form.old.is_empty() {
                        ui.colored_label(MUTED,"留空将删除选中文本中的匹配文字。");
                    }
                    if !form.error.is_empty() {ui.colored_label(RED,&form.error);}
                    ui.horizontal(|ui| {
                        if primary(ui,!form.nonce.is_empty() && !form.changes.is_empty(),"保存修改").clicked() {
                            self.submit("batch_save",json!({"nonce":form.nonce}),"正在保存选中文本修改…");
                        }
                    });
                });
            });
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
        let visible_count = members
            .iter()
            .filter(|entry| edit_matches(entry, &form.filter))
            .count();
        window(
            "批量修改 · 文本范围",
            ctx,
            if visible_count <= 1 { 640. } else { 760. },
        )
        .id(egui::Id::unique("batch-scope-responsive"))
        .min_width(if visible_count == 0 {
            380.
        } else if visible_count == 1 {
            500.
        } else {
            620.
        })
        .max_width(if visible_count == 0 {
            520.
        } else if visible_count == 1 {
            680.
        } else {
            (ctx.content_rect().width() - 32.).max(620.)
        })
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_height(if visible_count == 0 {
                180.
            } else {
                (420. + (visible_count.saturating_sub(1).min(4) as f32) * 18.)
                    .min(ctx.content_rect().height() - 130.)
            });
            if let Some(first) = members.first() {
                ui.add(
                    egui::Label::new(format!(
                        "{} → {} · 共 {} 段",
                        text(&first["edit"]["batch_operation"], "old"),
                        text(&first["edit"]["batch_operation"], "new"),
                        members.len()
                    ))
                    .truncate(),
                );
            } else {
                ui.label("此批已没有文本段。");
            }
            record_filter(ui, "batch-scope", &mut form.filter, &mut form.selected);
            egui::Panel::bottom("batch-scope-actions").show(ui, |ui| {
                if !form.error.is_empty() {
                    ui.colored_label(RED, &form.error);
                }
                let selected = members
                    .iter()
                    .find(|e| Some(count(e, "index") as usize) == form.selected);
                ui.horizontal(|ui| {
                    if secondary(
                        ui,
                        selected.is_some() && !self.write_busy(),
                        "从本批移除此段",
                    )
                    .on_hover_text("保留这段文本已有的单条修改。")
                    .clicked()
                        && let Some(entry) = selected
                    {
                        self.submit(
                            "remove_batch_member",
                            json!({"id":form.id,"index":entry["index"],"revision":self.revision()}),
                            "正在从本批移除这段文本…",
                        );
                    }
                });
            });
            record_browser(
                ui,
                "batch-scope",
                &members,
                &form.filter,
                &mut form.selected,
                true,
                None,
            );
        });
        if open {
            self.batch_scope = Some(form);
        }
    }
    fn source_notices_dialog(&mut self, ctx: &egui::Context) {
        if self.migration_notice || self.migration_form.as_ref().is_some_and(|form| form.open) {
            return;
        }
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
        let long_text = visible.iter().any(|entry| {
            ["original", "replacement"].iter().any(|key| {
                entry["edit"][key]
                    .as_str()
                    .is_some_and(|value| value.contains('\n') || value.chars().count() > 80)
            })
        });
        window(
            "单条修改",
            ctx,
            if edits.is_empty() {
                440.
            } else if edits.len() > 1 {
                760.
            } else if long_text {
                680.
            } else {
                600.
            },
        )
        .id(egui::Id::unique("single-edit-manager-responsive"))
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Light))
                .fill(BG)
                .corner_radius(12)
                .inner_margin(18),
        )
        .min_width(if visible.is_empty() {
            360.
        } else if edits.len() > 1 {
            620.
        } else {
            500.
        })
        .max_width(if visible.is_empty() {
            520.
        } else if visible.len() == 1 && !long_text {
            640.
        } else {
            (ctx.content_rect().width() - 32.).max(620.)
        })
        .open(&mut open)
        .show(ctx, |ui| {
            let side_by_side = ui.available_width() >= 560.;
            let text_width = if side_by_side {
                (ui.available_width() - 68.) * 0.5
            } else {
                ui.available_width() - 30.
            };
            let text_height = visible
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
                        .fold(0_f32, |height, next| {
                            if side_by_side {
                                height.max(next)
                            } else {
                                height + next
                            }
                        })
                })
                .unwrap_or(0.);
            let content_height = if visible.is_empty() {
                if edits.is_empty() { 50. } else { 110. }
            } else if visible.len() == 1 {
                (text_height + if side_by_side { 100. } else { 160. }).max(160.)
                    + if edits.len() > 1 { 44. } else { 0. }
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
                                    .hint_text("搜索原文或替换结果"),
                            )
                            .changed()
                        {
                            self.edit_view.selected = None;
                        }
                        ui.colored_label(MUTED, format!("{} / {} 条", visible.len(), edits.len()));
                    });
                }
                if let Some(entry) = visible
                    .iter()
                    .find(|e| Some(count(e, "index") as usize) == self.edit_view.selected)
                {
                    ui.horizontal(|ui| {
                        if primary(ui, entry["active"] == true, "编辑文本").clicked() {
                            self.submit(
                                "text_open",
                                json!({"edit_index":entry["index"]}),
                                "正在读取单条修改…",
                            );
                        }
                        if secondary(ui, true, "移除").clicked() {
                            self.submit(
                                "remove_edit",
                                json!({"index":entry["index"],"revision":self.revision()}),
                                "正在移除单条修改…",
                            );
                        }
                    });
                }
                if edits.is_empty() {
                    ui.label("尚未保存单条修改。可在「查找」双击文本开始编辑。");
                }
                if !edits.is_empty() {
                    single_edit_browser(
                        ui,
                        &visible,
                        &mut self.edit_view.selected,
                        visible.len() > 1,
                    );
                }
            });
        });
        self.show_edits = open;
    }
    fn confirmations(&mut self, ctx: &egui::Context) {
        if let Some(name) = self.confirm_record_reset.clone() {
            let mut open = true;
            let mut close = false;
            window(&format!("清空{}", record_name(&name)), ctx, 540.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "将清空{}。原文件会保留在数据目录的 recovery 文件夹，其他记录保持不变。",
                        record_name(&name)
                    ));
                    ui.colored_label(ORANGE, "如需抢救原有内容，请先打开数据目录检查文件。");
                    ui.horizontal(|ui| {
                        if action(ui, self.busy.is_none(), "确认清空", ButtonKind::Danger).clicked()
                        {
                            self.submit(
                                "reset_record",
                                json!({"name":name,"revision":self.revision()}),
                                "正在清空有问题的记录…",
                            );
                            close = true;
                        }
                        if secondary(ui, !self.write_busy(), "取消").clicked() {
                            close = true;
                        }
                    });
                });
            if !open || close {
                self.confirm_record_reset = None;
            }
        }
        if let Some(candidate) = self.confirm_baseline.clone() {
            let state = text(&candidate, "state");
            let first = state == "missing";
            let repair = state == "repair";
            let mut open = true;
            let mut close = false;
            window(if repair {"重建初始底稿"} else if first {"设置初始底稿"} else {"更新初始底稿"},ctx,540.)
                .open(&mut open)
                .show(ctx,|ui| {
                    ui.label(match state.as_str() {
                        "missing" => "还没有初始底稿。将当前游戏文本保存为初始底稿？",
                        "changed" => "当前游戏资源与初始底稿不同。将当前文本设为新的初始底稿？",
                        "repair" => "已找到与原底稿目录记录一致、并通过完整性校验的数据库。使用它重建初始底稿？",
                        _ => "检测到游戏资源版本更新。将当前游戏文本保存为新版初始底稿？",
                    });
                    ui.add_space(8.);
                    if repair {
                        ui.add(egui::Label::new(format!("重建来源：{}",text(&candidate,"source"))).wrap());
                        ui.label("新底稿校验成功后才替换目录；损坏底稿完整保留在旁边，游戏文件、词表和修改记录保持不变。");
                    } else {
                        ui.colored_label(MUTED,format!("当前资源：{}",text(&candidate,"game_version")));
                        ui.add_space(8.);
                        ui.label("会包含当前已安装的汉化或其他修改。请先退出游戏，并确认这是要作为替换起点的文本。");
                    }
                    if !first && !repair {
                        ui.colored_label(MUTED,"新底稿校验成功后才移出旧底稿；词表和修改记录会保留。");
                    }
                    ui.add_space(14.);
                    ui.horizontal(|ui| {
                        if primary(ui,!self.write_busy(),if repair {"重建底稿"} else if first {"设为初始底稿"} else {"更新底稿"}).clicked() {
                            self.submit(if repair {"baseline_rebuild_commit"} else {"baseline_commit"},json!({"token":candidate["token"]}),"正在保存并校验初始底稿…");
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
        if let Some(backup) = self.confirm_backup.clone() {
            let mut open = true;
            let mut close = false;
            window("恢复备份", ctx, 560.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        "用备份替换当前词表、单条和批量修改、目录及选项？恢复前会保留当前数据。",
                    );
                    ui.label(format!(
                        "词条 {} 条 · 指定文本修改 {} 条",
                        count(&backup, "rule_count"),
                        count(&backup, "edit_count")
                    ));
                    let game = text(&backup, "game");
                    ui.label(format!(
                        "游戏目录：{}",
                        if game.is_empty() { "未设置" } else { &game }
                    ));
                    ui.label(format!(
                        "跳过含日文假名的文本：{}",
                        if backup["skip_kana"] == true {
                            "勾选"
                        } else {
                            "未勾选"
                        }
                    ));
                    ui.horizontal(|ui| {
                        if primary(ui, !self.write_busy(), "恢复备份").clicked() {
                            self.submit(
                                "backup_restore",
                                json!({"nonce":backup["nonce"]}),
                                "正在恢复备份…",
                            );
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
            if !open || close {
                self.confirm_backup = None;
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
                            self.submit("restore", json!({}), "正在恢复初始底稿…");
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
    fn preview_hides_empty_comparison_but_keeps_errors_and_changed_texts_visible() {
        for (total, failed) in [(0, 0), (0, 1), (1, 0)] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            app.preview = Some(std::sync::Arc::new(
                json!({"total_changes":total,"prepared":true,"stats":{"added":total,"parse_failed":failed,"changes":if total>0 {json!([{"kind":"新增","table":"test","original":"恢复体力","before":"恢复体力","after":"补充行动力"}])}else{json!([])}}}),
            ));
            let mut labels = vec![];
            for frame in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 620.),
                        )),
                        time: Some(f64::from(frame)),
                        ..Default::default()
                    },
                    |ui| app.preview_dialog(ui.ctx()),
                );
                output.textures_delta.clear();
                labels = output
                    .shapes
                    .into_iter()
                    .filter_map(|shape| match shape.shape {
                        egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                        _ => None,
                    })
                    .collect();
            }
            for heading in ["当前游戏文字", "替换后的文字", "应用到游戏"] {
                assert_eq!(
                    labels.iter().any(|label| label == heading),
                    total > 0,
                    "{heading}"
                );
            }
            assert!(!labels.iter().any(|label| label == "本次最终文字"));
            if total == 0 {
                assert!(!labels.iter().any(|label| label == "关闭"));
                assert!(
                    !labels
                        .iter()
                        .any(|label| label == "请先退出游戏。未保留的旧规则会撤销。")
                );
            }
            if failed > 0 {
                assert!(labels.iter().any(|label| label.contains("已禁止应用")));
                assert!(
                    !labels
                        .iter()
                        .any(|label| label.contains("已与替换结果一致"))
                );
            }
        }
    }
    #[test]
    fn baseline_status_uses_a_green_or_gray_circle() {
        for verified in [false, true] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            app.loaded = true;
            if verified {
                app.baseline = json!({"version_id":"test","folder":"backup"});
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 620.),
                    )),
                    ..Default::default()
                },
                |ui| app.tools_page(ui),
            );
            output.textures_delta.clear();
            let expected = if verified {
                egui::Color32::from_rgb(37, 147, 91)
            } else {
                egui::Color32::from_rgb(160, 169, 181)
            };
            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Circle(circle) if circle.fill==expected && circle.radius==5.)));
            assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()=="已验证")));
        }
    }
    #[test]
    fn selected_batch_details_stay_above_footer_at_minimum_size() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let entry = json!({"index":0,"active":true,"label":"技能说明","edit":{"table":"Skill","row_id":1,"slot":0,"original":"原文".repeat(300),"replacement":"结果\n".repeat(100),"batch_operation":{"before":"本批之前\n".repeat(100)}}});
        let mut selected = Some(0);
        for frame in 0..3 {
            let mut output = ctx.run_ui(
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
                        None,
                    );
                    assert!(
                        body.min_rect().bottom() <= area.bottom() + 1.,
                        "detail overlaps footer: {} > {}",
                        body.min_rect().bottom(),
                        area.bottom()
                    );
                },
            );
            output.textures_delta.clear();
        }
    }
}
