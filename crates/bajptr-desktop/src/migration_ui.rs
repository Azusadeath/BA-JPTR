use crate::{app::*, dialogs::window, state::*};
use eframe::egui::{self, RichText};
use serde_json::json;

fn comparison(ui: &mut egui::Ui, original: &str, replacement: &str) {
    ui.columns(2, |cols| {
        cols[0].colored_label(MUTED, "底稿原文");
        cols[0].add(egui::Label::new(original).wrap().selectable(true));
        cols[1].colored_label(MUTED, "迁移后");
        cols[1].add(
            egui::Label::new(RichText::new(replacement).color(BLUE))
                .wrap()
                .selectable(true),
        );
    });
}

impl Desktop {
    pub fn migration_dialogs(&mut self, ctx: &egui::Context) {
        if self.migration_notice {
            let mut open = true;
            let mut close = false;
            window("迁移旧版修改", ctx, 520.)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("检测到旧版单条或批量修改，是否迁移到当前底稿？");
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        if primary(ui, self.busy.is_none(), "开始迁移").clicked() {
                            self.submit(
                                "migration_start",
                                json!({"revision":self.revision()}),
                                "正在核对旧版修改…",
                            );
                            close = true;
                        }
                        if ui.button("暂不迁移").clicked() {
                            close = true;
                        }
                    });
                });
            if !open || close {
                self.migration_notice = false;
            }
        }
        let Some(mut form) = self.migration_form.take() else {
            return;
        };
        if !form.open {
            self.migration_form = Some(form);
            return;
        }
        let busy = self.busy.is_some();
        let mut open = true;
        window("迁移旧版修改 · 核对文本", ctx, 1000.)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_height((ctx.content_rect().height() - 140.).clamp(360., 620.));
                let total = slice(&form.plan, "items").len();
                let confirmed = form.confirmed_count();
                let pending = form.groups.iter().filter(|g| !g.confirmed).count();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} 段 · {} 组修改", total, form.groups.len()));
                    ui.colored_label(MUTED, format!("已确认 {} 段", confirmed));
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut form.show_confirmed,
                        false,
                        format!("待核对 {pending}"),
                    );
                    ui.selectable_value(
                        &mut form.show_confirmed,
                        true,
                        format!("已确认 {}", form.groups.len() - pending),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut form.filter)
                            .hint_text("搜索文本或来源表")
                            .desired_width(ui.available_width()),
                    );
                });
                form.sync_selection();
                egui::Panel::bottom("migration-actions")
                    .frame(
                        egui::Frame::new()
                            .fill(egui::Color32::WHITE)
                            .inner_margin(egui::Margin::symmetric(0, 8)),
                    )
                    .show(ui, |ui| {
                        if !form.error.is_empty() {
                            ui.colored_label(RED, &form.error);
                        }
                        ui.horizontal(|ui| {
                            if let Some(index) = form.selected {
                                if form.groups[index].confirmed {
                                    if secondary(ui, !busy, "重新核对这组").clicked() {
                                        form.groups[index].confirmed = false;
                                        form.show_confirmed = false;
                                    }
                                } else {
                                    let amount = if form.groups[index].equal_count() {
                                        form.groups[index].members.len()
                                    } else {
                                        form.groups[index].chosen.len()
                                    };
                                    if primary(
                                        ui,
                                        !busy && form.can_confirm(index),
                                        &format!("确认 {amount} 处并继续"),
                                    )
                                    .clicked()
                                    {
                                        form.confirm(index);
                                    }
                                    if secondary(ui, !busy, "下一组").clicked() {
                                        form.next();
                                    }
                                }
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if primary(
                                        ui,
                                        !busy && confirmed > 0,
                                        &format!("保存迁移（{confirmed} 段）"),
                                    )
                                    .clicked()
                                    {
                                        self.submit(
                                            "migration_commit",
                                            form.payload(),
                                            "正在保存迁移记录…",
                                        );
                                    }
                                },
                            );
                        });
                        ui.add_space(3.);
                        ui.colored_label(
                            MUTED,
                            "保存时自动备份，可撤销；预览并应用后才会修改游戏。",
                        );
                    });
                let area = ui.available_rect_before_wrap();
                let left_width = (area.width() * 0.30).clamp(230., 300.);
                let left = egui::Rect::from_min_max(
                    area.min,
                    egui::pos2(area.left() + left_width, area.bottom()),
                );
                let right =
                    egui::Rect::from_min_max(egui::pos2(left.right() + 20., area.top()), area.max);
                let mut list_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                list_ui.shrink_clip_rect(left);
                let visible = form.visible();
                egui::ScrollArea::vertical()
                    .id_salt(("migration-groups", form.show_confirmed, &form.filter))
                    .min_scrolled_height(0.)
                    .auto_shrink([false, false])
                    .show_rows(&mut list_ui, 60., visible.len(), |ui, range| {
                        for position in range {
                            let index = visible[position];
                            let group = &form.groups[index];
                            let caption = if group.confirmed {
                                format!("已确认 {} 处", group.chosen.len())
                            } else {
                                format!(
                                    "原 {} 处 → 新 {} 处",
                                    group.members.len(),
                                    group.targets.len()
                                )
                            };
                            let summary = text(&group.item, "original").replace(['\r', '\n'], " ");
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 60.),
                                egui::Sense::click(),
                            );
                            if form.selected == Some(index) {
                                ui.painter().rect_filled(rect, 6, SOFT_BLUE);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 6, BG);
                            }
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    ui.is_enabled(),
                                    form.selected == Some(index),
                                    format!("{caption} {summary}"),
                                )
                            });
                            for (offset, label) in [
                                (7., RichText::new(caption).small().color(MUTED)),
                                (30., RichText::new(summary)),
                            ] {
                                let mut line = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(egui::Rect::from_min_size(
                                            rect.min + egui::vec2(10., offset),
                                            egui::vec2(rect.width() - 20., 22.),
                                        ))
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                line.add(egui::Label::new(label).selectable(false).truncate());
                            }
                            if response.clicked() {
                                form.selected = Some(index);
                            }
                        }
                    });
                let mut detail = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                detail.shrink_clip_rect(right);
                if let Some(index) = form.selected {
                    let group = &form.groups[index];
                    let item = group.item.clone();
                    let old_count = group.members.len();
                    let new_count = group.targets.len();
                    let equal = group.equal_count();
                    let competing = form.has_competing(index);
                    let is_confirmed = group.confirmed;
                    let contexts = form.context_groups(index);
                    egui::ScrollArea::vertical()
                        .id_salt(("migration-detail", index))
                        .min_scrolled_height(0.)
                        .auto_shrink([false, false])
                        .show(&mut detail, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(
                                    MUTED,
                                    bajptr_project::model::title(&text(&item, "table")),
                                );
                                ui.label(format!("原 {old_count} 处 → 新 {new_count} 处"));
                            });
                            ui.add_space(6.);
                            comparison(ui, &text(&item, "original"), &text(&item, "replacement"));
                            ui.add_space(10.);
                            ui.separator();
                            if competing {
                                ui.colored_label(
                                    ORANGE,
                                    "同一位置对应不同的旧修改，请选择要保留的结果。",
                                );
                                if !equal {
                                    ui.label(format!("最多选择 {old_count} 处。"));
                                }
                            } else {
                                ui.label(format!(
                                    "选择新版中要恢复修改的位置，最多 {old_count} 处。"
                                ));
                            }
                            for (bucket, (context, candidates)) in contexts.iter().enumerate() {
                                let reserved = candidates.iter().any(|&c| form.reserved(index, c));
                                let checked = candidates
                                    .iter()
                                    .all(|c| form.groups[index].chosen.contains(c));
                                let selected_count = candidates
                                    .iter()
                                    .filter(|c| !form.groups[index].chosen.contains(c))
                                    .count()
                                    + form.groups[index].chosen.len();
                                let selectable = !busy
                                    && !is_confirmed
                                    && !reserved
                                    && (checked || selected_count <= old_count);
                                egui::Frame::new()
                                    .fill(BG)
                                    .corner_radius(6)
                                    .inner_margin(10)
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.push_id(bucket, |ui| {
                                            if !equal {
                                                let mut value = checked;
                                                if ui
                                                    .add_enabled(
                                                        selectable,
                                                        egui::Checkbox::new(
                                                            &mut value,
                                                            format!("{} 处", candidates.len()),
                                                        ),
                                                    )
                                                    .changed()
                                                {
                                                    form.toggle_context(index, candidates, value);
                                                }
                                            } else if candidates.len() > 1 {
                                                ui.colored_label(
                                                    MUTED,
                                                    format!("{} 处上下文相同", candidates.len()),
                                                );
                                            }
                                            ui.add(
                                                egui::Label::new(if context.is_empty() {
                                                    "没有其他文本可供区分。"
                                                } else {
                                                    context.as_str()
                                                })
                                                .wrap()
                                                .selectable(true),
                                            );
                                            if reserved {
                                                ui.colored_label(ORANGE, "已分配给另一组修改。");
                                            } else if !equal && candidates.len() > old_count {
                                                ui.colored_label(
                                                    ORANGE,
                                                    "相同上下文无法区分具体位置，此组暂不迁移。",
                                                );
                                            }
                                        });
                                    });
                                ui.add_space(5.);
                            }
                        });
                } else {
                    detail.vertical_centered(|ui| {
                        ui.add_space(40.);
                        if !form.filter.is_empty() {
                            ui.label("没有匹配的文本。");
                        } else if !form.show_confirmed && pending == 0 {
                            ui.heading("核对完成");
                            ui.label(format!("{confirmed} 段修改等待保存"));
                        } else {
                            ui.label("尚未确认任何修改。");
                        }
                    });
                }
                ui.advance_cursor_after_rect(area);
            });
        form.open = open || self.write_busy();
        self.migration_form = Some(form);
    }
}
