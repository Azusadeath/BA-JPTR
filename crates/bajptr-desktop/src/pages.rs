use crate::{app::*, state::*};
use eframe::egui::{self, Color32};
use serde_json::json;
use std::{path::PathBuf, time::Instant};

impl Desktop {
    pub fn header(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1., LINE))
            .corner_radius(10)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("游戏目录").size(14.).color(MUTED));
                    let reserve = action_width(ui, "选择文件夹")
                        + action_width(ui, "检查底稿")
                        + 60.
                        + ui.spacing().item_spacing.x * 3.;
                    let width = (ui.available_width() - reserve).max(180.);
                    let response = ui
                        .add_enabled_ui(!self.write_busy(), |ui| {
                            ui.add_sized(
                                [width, 36.],
                                egui::TextEdit::singleline(&mut self.game)
                                    .font(egui::TextStyle::Body)
                                    .margin(egui::Margin::symmetric(10, 8))
                                    .desired_width(width),
                            )
                        })
                        .inner;
                    if response.changed() {
                        self.connected = false;
                        self.path_changed = Some(Instant::now());
                    }
                    if secondary(ui, !self.write_busy(), "选择文件夹").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_title("选择日服游戏安装目录")
                            .pick_folder()
                    {
                        self.game = path.to_string_lossy().into();
                        self.save_settings();
                    }
                    if secondary(
                        ui,
                        !self.write_busy() && !self.game.trim().is_empty(),
                        "检查底稿",
                    )
                    .clicked()
                    {
                        self.connected = false;
                        self.submit("check", json!({}), "正在检查游戏资源与底稿…", true);
                    }
                    pill(
                        ui,
                        if self.connected {
                            "已连接"
                        } else if self.confirm_baseline.is_some()
                            || matches!(
                                text(&self.baseline_check, "state").as_str(),
                                "missing" | "update" | "changed"
                            )
                        {
                            "待确认"
                        } else {
                            "未连接"
                        },
                        if self.connected {
                            Color32::from_rgb(44, 115, 84)
                        } else {
                            MUTED
                        },
                        if self.connected {
                            Color32::from_rgb(233, 246, 239)
                        } else {
                            BG
                        },
                    );
                });
            });
        ui.add_space(2.);
        ui.horizontal(|ui| {
            for (index, title) in ["替换", "查找", "恢复"].iter().enumerate() {
                if action(
                    ui,
                    true,
                    title,
                    if self.tab == index {
                        ButtonKind::Soft
                    } else {
                        ButtonKind::Quiet
                    },
                )
                .clicked()
                {
                    self.tab = index;
                    if index == 2 && !self.game.trim().is_empty() {
                        self.submit("check", json!({}), "正在检查初始底稿…", true);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [ui.available_width().min(420.), 24.],
                    egui::Label::new(
                        egui::RichText::new(&self.application)
                            .size(13.)
                            .color(MUTED),
                    )
                    .halign(egui::Align::RIGHT)
                    .truncate(),
                )
                .on_hover_text(&self.application);
            });
        });
    }
    pub fn replacement(&mut self, ui: &mut egui::Ui) {
        let state = self.state.clone();
        let rules = slice(&state, "rules");
        let can_edit = self.loaded && self.healthy() && !self.write_busy();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(can_edit, ToolButton::new("添加词条"))
                .clicked()
            {
                self.rule_form = Some(RuleForm::new(None, self.revision(), None));
            }
            if ui
                .add_enabled(
                    can_edit && self.rule_selection.is_some_and(|i| i < rules.len()),
                    ToolButton::new("编辑"),
                )
                .clicked()
            {
                let i = self.rule_selection.unwrap();
                self.rule_form = Some(RuleForm::new(Some(i), self.revision(), rules.get(i)));
            }
            if ui
                .add_enabled(
                    can_edit && self.rule_selection.is_some_and(|i| i < rules.len()),
                    ToolButton::new("移除"),
                )
                .clicked()
            {
                self.submit(
                    "remove_rule",
                    json!({"index":self.rule_selection,"revision":self.revision()}),
                    "正在移除词条…",
                    false,
                );
                self.rule_selection = None;
            }
            if ui
                .add_enabled(
                    can_edit && self.state["can_undo"] == true,
                    ToolButton::new("撤销"),
                )
                .clicked()
            {
                self.submit("history", json!({"undo":true}), "正在撤销…", false);
            }
            if ui
                .add_enabled(
                    can_edit && self.state["can_redo"] == true,
                    ToolButton::new("重做"),
                )
                .clicked()
            {
                self.submit("history", json!({"undo":false}), "正在重做…", false);
            }
            if ui
                .add_enabled(can_edit, ToolButton::new("导入词表"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("替换词表", &["tsv", "txt"])
                    .pick_file()
            {
                self.submit("import_read", json!({"path":path}), "正在读取词表…", true);
            }
            if ui
                .add_enabled(can_edit && !rules.is_empty(), ToolButton::new("导出词表"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("替换词表", &["tsv"])
                    .set_file_name("我的译名.tsv")
                    .save_file()
            {
                self.submit("export_rules", json!({"path":path}), "正在导出词表…", false);
            }
            if ui
                .add_enabled(
                    can_edit,
                    ToolButton::new(format!("单条修改（{}）", slice(&self.state, "edits").len())),
                )
                .clicked()
            {
                self.show_edits = true;
            }
        });
        ui.add_space(6.);
        egui::Panel::bottom("replace-actions")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                top: 14,
                ..Default::default()
            }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_edit,
                            egui::Checkbox::new(&mut self.skip, "跳过含日文假名的文本"),
                        )
                        .changed()
                    {
                        self.save_settings();
                    }
                    ui.label(
                        egui::RichText::new(format!("{} 条词条", rules.len()))
                            .size(13.)
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = self.connected && self.healthy() && self.busy.is_none();
                        if primary(ui, ready, "应用到游戏").clicked() {
                            self.submit("preview", json!({}), "正在比较本次最终变化…", true);
                        }
                        if secondary(ui, ready, "预览替换").clicked() {
                            self.submit("preview", json!({}), "正在比较本次最终变化…", true);
                        }
                        if action(ui, ready, "仅生成补丁", ButtonKind::Quiet).clicked() {
                            self.submit("generate", json!({}), "正在生成并校验补丁…", false);
                        }
                    });
                });
            });
        let width = ui.available_width() - 12.;
        let widths = [(width - 350.) * 0.5, (width - 350.) * 0.5, 110., 170., 70.];
        table_header(
            ui,
            &["原文字", "替换为", "匹配方式", "来源表", "命中"],
            &widths,
        );
        egui::Frame::new()
            .fill(Color32::WHITE)
            .corner_radius(10)
            .stroke(egui::Stroke::new(1., LINE))
            .show(ui, |ui| {
                if rules.is_empty() {
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.colored_label(MUTED, "点击「添加词条」开始，或导入已有词表。");
                        },
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_HEIGHT, rules.len(), |ui, range| {
                            for i in range {
                                let row = &rules[i];
                                let new = text(row, "display_new");
                                let values = [
                                    text(row, "display_old"),
                                    if new.is_empty() {
                                        "（删除匹配文字）".into()
                                    } else {
                                        new
                                    },
                                    if !row["draft"].is_null() {
                                        "模板".into()
                                    } else {
                                        mode_name(&text(&row["rule"], "mode")).into()
                                    },
                                    text(row, "scope"),
                                    self.hits.get(i).map(u64::to_string).unwrap_or("—".into()),
                                ];
                                let response = data_row(
                                    ui,
                                    i,
                                    self.rule_selection == Some(i),
                                    &values,
                                    &widths,
                                );
                                if response.clicked() {
                                    self.rule_selection = Some(i);
                                }
                                if response.double_clicked() && can_edit {
                                    self.rule_form =
                                        Some(RuleForm::new(Some(i), self.revision(), Some(row)));
                                }
                            }
                        });
                }
            });
    }
    pub fn search_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("底稿原文");
            ui.add_sized(
                [ui.available_width() - 255., 36.],
                egui::TextEdit::singleline(&mut self.keyword)
                    .font(egui::TextStyle::Body)
                    .hint_text("输入要查找的文字"),
            );
            if primary(
                ui,
                self.connected && self.busy.is_none() && !self.keyword.trim().is_empty(),
                "查找",
            )
            .clicked()
            {
                self.submit(
                    "search",
                    json!({"keyword":self.keyword}),
                    "正在查找，保留上次结果…",
                    true,
                );
            }
            if ui
                .add_enabled(
                    self.connected && self.busy.is_none(),
                    ToolButton::new("导出文本"),
                )
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title("选择导出文件夹")
                    .pick_folder()
            {
                self.submit(
                    "export_text",
                    json!({"path":path}),
                    "正在导出底稿文本…",
                    false,
                );
            }
        });
        let search = self.search.clone();
        let rows = slice(&search, "rows");
        egui::Panel::bottom("search-actions").show(ui,|ui|{
            ui.horizontal_wrapped(|ui|{let selected=self.selection.rows.len();
                if ui.add_enabled(selected==1&&!self.write_busy(),ToolButton::new("编辑文本")).clicked(){self.open_text(*self.selection.rows.first().unwrap());}
                if ui.add_enabled(selected>0&&!self.write_busy(),ToolButton::new("替换选中文本")).clicked(){self.batch_form=Some(BatchForm{indices:self.selection.rows.iter().copied().collect(),old:text(&self.search,"keyword"),new:String::new(),nonce:String::new(),changes:Vec::new(),error:String::new()});}
                if ui.add_enabled(selected==1&&!self.write_busy(),ToolButton::new("用此表添加词条")).clicked(){let row=&rows[*self.selection.rows.first().unwrap()];let keyword=text(&self.search,"keyword");let value=json!({"rule":{"old":keyword,"new":keyword,"mode":"sub","source":"-","table":row["table"]}});self.rule_form=Some(RuleForm::new(None,self.revision(),Some(&value)));}
                if ui.add_enabled(selected==1, ToolButton::new("来源表详情")).clicked() {
                    let row=&rows[*self.selection.rows.first().unwrap()];
                    self.table_detail=Some(self.tables.iter().find(|v|v["name"]==row["table"]).cloned().unwrap_or(json!({"name":row["table"],"label":row["label"]})));
                }
                if secondary(ui,true,"单条修改").clicked() { self.show_edits=true; }
                ui.colored_label(MUTED,format!("已选 {selected} 条（Ctrl 多选，Shift 连选）"));
            });
            ui.colored_label(MUTED,if self.search.is_null(){"输入文字后点击「查找」。".into()}else{format!("共 {} 处，显示前 {} 条。双击可编辑完整文本。",count(&self.search,"total"),rows.len())});
        });
        let width = ui.available_width() - 12.;
        let widths = [width - 380., 180., 90., 110.];
        table_header(ui, &["本次文本", "来源表", "记录", "状态"], &widths);
        egui::Frame::new()
            .fill(Color32::WHITE)
            .corner_radius(10)
            .stroke(egui::Stroke::new(1., LINE))
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.colored_label(
                                MUTED,
                                if self.search.is_null() {
                                    "输入文字后点击「查找」。"
                                } else {
                                    "未找到匹配文本。"
                                },
                            );
                        },
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                            for i in range {
                                let row = &rows[i];
                                let values = [
                                    text(row, "text").replace('\n', " [换行] "),
                                    text(row, "label"),
                                    format!("{}:{}", row["row_id"], row["slot"]).replace('"', ""),
                                    text(row, "state"),
                                ];
                                let response = data_row(
                                    ui,
                                    i,
                                    self.selection.rows.contains(&i),
                                    &values,
                                    &widths,
                                );
                                if response.clicked() {
                                    let modifiers = ui.input(|i| i.modifiers);
                                    self.selection.click(i, modifiers.ctrl, modifiers.shift);
                                }
                                if response.double_clicked() && !self.write_busy() {
                                    self.open_text(i);
                                }
                            }
                        });
                }
            });
    }
    pub fn restore_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(egui::Stroke::new(1., LINE))
                    .corner_radius(12)
                    .inner_margin(24)
                    .show(ui, |ui| {
                        let width = (ui.available_width() - 48.).min(720.);
                        ui.set_min_width(width);
                        ui.set_max_width(width);
                        ui.heading("恢复初始底稿");
                        ui.colored_label(MUTED, "恢复游戏文字，保留词表和单条修改。");
                        ui.add_space(15.);
                        if self.baseline.is_null() {
                            ui.label("点击刷新检查底稿；首次使用或资源更新后需确认保存。");
                        } else {
                            for (label, key) in [("资源版本", "version"), ("建立时间", "created")]
                            {
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [96., 24.],
                                        egui::Label::new(label).halign(egui::Align::LEFT),
                                    );
                                    ui.add(egui::Label::new(text(&self.baseline, key)).wrap());
                                });
                            }
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [96., 24.],
                                    egui::Label::new("校验状态").halign(egui::Align::LEFT),
                                );
                                ui.colored_label(Color32::from_rgb(37, 107, 78), "已验证");
                            });
                        }
                        if !self.baseline.is_null() {
                            egui::CollapsingHeader::new("详情").show(ui, |ui| {
                                for (label, key) in [
                                    ("底稿位置", "folder"),
                                    ("资源标识", "resource"),
                                    ("版本标识", "version_id"),
                                ] {
                                    ui.label(label);
                                    ui.add(
                                        egui::Label::new(text(&self.baseline, key))
                                            .wrap()
                                            .selectable(true),
                                    );
                                }
                            });
                        }
                        ui.add_space(18.);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !self.game.trim().is_empty() && self.busy.is_none(),
                                    ToolButton::new("刷新"),
                                )
                                .clicked()
                            {
                                self.submit("check", json!({}), "正在检查初始底稿…", true);
                            }
                            if ui
                                .add_enabled(
                                    !self.baseline.is_null(),
                                    ToolButton::new("打开底稿位置"),
                                )
                                .clicked()
                            {
                                Self::open_folder(&PathBuf::from(text(&self.baseline, "folder")));
                            }
                            if primary(
                                ui,
                                self.connected && self.busy.is_none() && !self.baseline.is_null(),
                                "恢复底稿",
                            )
                            .clicked()
                            {
                                self.confirm_restore = true;
                            }
                        });
                    });
            });
    }
}
