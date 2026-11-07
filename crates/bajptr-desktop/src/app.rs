pub use crate::theme::*;
use crate::{state::*, transport::Backend};
use eframe::egui::{self, Color32, RichText};
use serde_json::{Value, json};
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};
pub struct Busy {
    pub id: u64,
    pub cancellable: bool,
}
pub struct Desktop {
    pub backend: Option<Backend>,
    pub pending: HashMap<u64, String>,
    pub obsolete: HashSet<u64>,
    pub busy: Option<Busy>,
    pub state: Arc<Value>,
    pub loaded: bool,
    pub connected: bool,
    pub game: String,
    pub skip: bool,
    pub path_changed: Option<Instant>,
    pub tab: usize,
    pub status: String,
    pub application: String,
    pub logs: Vec<String>,
    pub error: Option<String>,
    pub rule_selection: Option<usize>,
    pub search: Arc<Value>,
    pub keyword: String,
    pub selection: Selection,
    pub tables: Vec<Value>,
    pub rule_form: Option<RuleForm>,
    pub scope_form: Option<ScopeForm>,
    pub template_form: Option<TemplateForm>,
    pub template_changed: Option<Instant>,
    pub text_form: Option<TextForm>,
    pub batch_form: Option<BatchForm>,
    pub preview: Option<Arc<Value>>,
    pub preview_row: Option<usize>,
    pub show_edits: bool,
    pub show_logs: bool,
    pub confirm_import: Option<Value>,
    pub confirm_restore: bool,
    pub confirm_baseline: Option<Value>,
    pub baseline_check: Value,
    pub baseline: Value,
    pub output: Option<PathBuf>,
    pub hits: Vec<u64>,
    pub table_detail: Option<Value>,
    pub startup_probe: Option<crate::startup::Probe>,
}
impl Desktop {
    pub fn new(cc: &eframe::CreationContext<'_>, project: PathBuf, data: PathBuf) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        crate::theme::install(ctx);
        let backend = Backend::start(&project, &data, ctx.clone());
        let mut app = Self {
            backend: None,
            pending: HashMap::new(),
            obsolete: HashSet::new(),
            busy: None,
            state: Value::Null.into(),
            loaded: false,
            connected: false,
            game: String::new(),
            skip: true,
            path_changed: None,
            tab: 0,
            status: "正在读取修改记录…".into(),
            application: "应用状态待核对，先预览".into(),
            logs: Vec::new(),
            error: None,
            rule_selection: None,
            search: Value::Null.into(),
            keyword: String::new(),
            selection: Selection::default(),
            tables: Vec::new(),
            rule_form: None,
            scope_form: None,
            template_form: None,
            template_changed: None,
            text_form: None,
            batch_form: None,
            preview: None,
            preview_row: None,
            show_edits: false,
            show_logs: false,
            confirm_import: None,
            confirm_restore: false,
            confirm_baseline: None,
            baseline_check: Value::Null,
            baseline: Value::Null,
            output: None,
            hits: Vec::new(),
            table_detail: None,
            startup_probe: crate::startup::Probe::from_environment(),
        };
        match backend {
            Ok(backend) => {
                app.backend = Some(backend);
                app.submit("load", json!({}), "正在读取修改记录…", false);
            }
            Err(error) => {
                app.status = error.clone();
                app.error = Some(error);
            }
        }
        app
    }
    pub fn revision(&self) -> u64 {
        count(&self.state, "revision")
    }
    pub fn write_busy(&self) -> bool {
        self.busy.as_ref().is_some_and(|b| !b.cancellable)
    }
    pub fn healthy(&self) -> bool {
        self.state
            .get("errors")
            .and_then(Value::as_object)
            .is_none_or(|v| v.is_empty())
    }
    pub fn modal(&self) -> bool {
        self.rule_form.is_some()
            || self.scope_form.is_some()
            || self.template_form.is_some()
            || self.text_form.is_some()
            || self.batch_form.is_some()
            || self.preview.is_some()
            || self.show_edits
            || self.confirm_import.is_some()
            || self.confirm_restore
            || self.confirm_baseline.is_some()
            || self.error.is_some()
            || self.table_detail.is_some()
    }
    pub fn submit(&mut self, action: &str, payload: Value, title: &str, cancellable: bool) {
        if self.write_busy() {
            return;
        }
        if let Some(previous) = self.busy.take() {
            self.obsolete.insert(previous.id);
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":previous.id}));
            }
        }
        let warm: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, a)| a.as_str() == "tables")
            .map(|(id, _)| *id)
            .collect();
        for id in warm {
            self.obsolete.insert(id);
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":id}));
            }
        }
        if let Some(backend) = &mut self.backend {
            match backend.send(action, payload) {
                Ok(id) => {
                    self.pending.insert(id, action.into());
                    self.busy = Some(Busy { id, cancellable });
                    self.status = title.into();
                }
                Err(e) => {
                    self.error = Some(e.clone());
                    self.status = e;
                }
            }
        }
    }
    pub fn background_tables(&mut self) {
        if self.pending.values().any(|a| a == "tables") || !self.connected {
            return;
        }
        if let Some(backend) = &mut self.backend
            && let Ok(id) = backend.send("tables", json!({}))
        {
            self.pending.insert(id, "tables".into());
        }
    }
    pub fn cancel(&mut self) {
        if let Some(busy) = &self.busy
            && busy.cancellable
        {
            self.obsolete.insert(busy.id);
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":busy.id}));
            }
            self.status = "正在取消，保留上次结果…".into();
        }
    }
    pub fn save_settings(&mut self) {
        self.path_changed = None;
        self.connected = false;
        self.confirm_baseline = None;
        self.baseline_check = Value::Null;
        self.submit(
            "settings",
            json!({"game":self.game,"skip_kana":self.skip,"revision":self.revision()}),
            "正在保存设置…",
            false,
        );
    }
    pub fn apply_state(&mut self, value: Value) {
        let changed = count(&value, "revision") != self.revision();
        let game_changed = text(&value, "game") != text(&self.state, "game");
        if self.path_changed.is_none() {
            self.game = text(&value, "game");
        }
        self.skip = value
            .get("skip_kana")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if changed {
            self.hits.clear();
            self.preview = None;
        }
        if game_changed {
            self.search = Value::Null.into();
            self.selection.clear();
            self.tables.clear();
            self.baseline = Value::Null;
            self.confirm_baseline = None;
            self.baseline_check = Value::Null;
        }
        if let Some(search) = value.get("search").filter(|v| !v.is_null()) {
            self.search = search.clone().into();
        }
        self.application = text(&value, "application");
        self.state = value.into();
    }
    pub fn open_text(&mut self, index: usize) {
        self.submit(
            "text_open",
            json!({"index":index,"search_nonce":text(&self.search,"nonce")}),
            "正在读取这段文本…",
            true,
        );
    }
    pub fn open_folder(path: &std::path::Path) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("explorer.exe")
                .arg(path)
                .creation_flags(0x08000000)
                .spawn();
        }
    }
    pub fn events(&mut self) {
        let events: Vec<_> = self
            .backend
            .as_ref()
            .map(|b| b.events().try_iter().take(200).collect())
            .unwrap_or_default();
        for event in events {
            let id = count(&event, "id");
            let kind = text(&event, "event");
            if kind == "backend_closed" {
                self.busy = None;
                self.pending.clear();
                self.connected = false;
                self.backend = None;
                self.status = text(&event, "message");
                self.error = Some(self.status.clone());
                continue;
            }
            if kind == "progress" || kind == "detail" {
                let message = text(&event, "message");
                if self.busy.as_ref().is_some_and(|b| b.id == id) {
                    self.status = message.clone();
                }
                self.logs.push(message);
                if self.logs.len() > 2000 {
                    self.logs.drain(..500);
                }
                continue;
            }
            let action = self.pending.remove(&id).unwrap_or_default();
            let active = self.busy.as_ref().is_some_and(|b| b.id == id);
            if active {
                self.busy = None;
            }
            if self.obsolete.remove(&id) {
                if active {
                    self.status = "已取消，保留上次结果。".into();
                }
                continue;
            }
            if kind == "cancelled" {
                self.status = "已取消，保留上次结果。".into();
                continue;
            }
            if kind == "error" {
                let message = text(&event, "message");
                self.logs.push(text(&event, "detail"));
                self.status = message.clone();
                match action.as_str() {
                    "save_rule" => {
                        if let Some(f) = &mut self.rule_form {
                            f.error = message.clone();
                        }
                    }
                    "text_save" => {
                        if let Some(f) = &mut self.text_form {
                            f.error = message.clone();
                        }
                    }
                    "batch_preview" | "batch_save" => {
                        if let Some(f) = &mut self.batch_form {
                            f.error = message.clone();
                        }
                    }
                    "template" => {
                        if let Some(f) = &mut self.template_form {
                            f.error = message.clone();
                        }
                    }
                    "check" => {
                        self.connected = false;
                        self.baseline = Value::Null;
                        self.baseline_check = Value::Null;
                        self.confirm_baseline = None;
                    }
                    "baseline_commit" => {
                        self.connected = false;
                        self.baseline = Value::Null;
                        self.baseline_check = Value::Null;
                        self.error = Some(message);
                    }
                    "tables" => {}
                    _ => self.error = Some(message),
                }
                continue;
            }
            if kind != "result" {
                continue;
            }
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("accept", json!({"target":id}));
            }
            let data = event.get("data").cloned().unwrap_or(Value::Null);
            match action.as_str() {
                "load" => {
                    self.apply_state(data);
                    self.loaded = true;
                    self.logs.extend(
                        list(&self.state, "messages")
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string),
                    );
                    if !self.healthy() {
                        self.error = Some(
                            "修改记录读取失败，原文件已保留。请检查运行详情并修复文件后重启。"
                                .into(),
                        );
                    }
                    if !self.game.is_empty() {
                        self.submit("check", json!({}), "正在检查游戏目录…", true);
                    } else {
                        self.status = "先选择游戏目录，再添加或导入词条。".into();
                    }
                }
                "settings" => {
                    self.apply_state(data);
                    if self.path_changed.is_none() {
                        self.submit("check", json!({}), "正在检查游戏目录…", true);
                    }
                }
                "check" => {
                    if self.path_changed.is_some() || text(&self.state, "game") != self.game {
                        continue;
                    }
                    self.baseline_check = data["baseline_status"].clone();
                    self.baseline = self.baseline_check["baseline"].clone();
                    self.connected = data["connected"] == true;
                    self.application = text(&data, "application");
                    match text(&self.baseline_check, "state").as_str() {
                        "ready" => {
                            self.confirm_baseline = None;
                            self.status = "底稿已就绪。修改自动保存，预览后应用。".into();
                            self.background_tables();
                        }
                        "blocked" => {
                            self.confirm_baseline = None;
                            self.baseline = Value::Null;
                            self.status = "未找到初始底稿，当前资源已被本工具修改。".into();
                            self.error = Some("当前资源是本工具替换后的内容，不能作为初始底稿。请先通过游戏修复资源，或找回原底稿，再点击“检查底稿”。".into());
                        }
                        _ => {
                            self.connected = false;
                            self.baseline = Value::Null;
                            self.search = Value::Null.into();
                            self.selection.clear();
                            self.preview = None;
                            self.tables.clear();
                            self.status = "请确认初始底稿，确认前不会保存或替换底稿。".into();
                            self.confirm_baseline = Some(self.baseline_check.clone());
                        }
                    }
                }
                "baseline_commit" => {
                    self.baseline = data;
                    self.confirm_baseline = None;
                    self.search = Value::Null.into();
                    self.selection.clear();
                    self.preview = None;
                    self.hits.clear();
                    self.tables.clear();
                    self.submit("check", json!({}), "底稿已保存，正在核对…", true);
                }
                "tables" => {
                    self.tables = data.as_array().cloned().unwrap_or_default();
                    if let Some(f) = &mut self.scope_form {
                        f.available = self.tables.clone();
                    }
                }
                "save_rule" | "remove_rule" | "history" | "text_save" | "batch_save"
                | "remove_edit" | "import_commit" => {
                    self.apply_state(data);
                    if action == "save_rule" {
                        self.rule_form = None;
                    }
                    if action == "text_save" {
                        self.text_form = None;
                    }
                    if action == "batch_save" {
                        self.batch_form = None;
                    }
                    self.status = "修改已保存，预览后应用到游戏。".into();
                }
                "search" => {
                    self.search = data.into();
                    self.selection.clear();
                    self.status = format!(
                        "共找到 {} 处；双击编辑，Ctrl、Shift 多选。",
                        count(&self.search, "total")
                    );
                }
                "text_open" => {
                    self.status = "编辑完整文本，保存后预览并应用。".into();
                    self.text_form = Some(TextForm {
                        nonce: text(&data, "nonce"),
                        label: text(&data, "label"),
                        original: text(&data, "original"),
                        value: text(&data, "text"),
                        show_original: false,
                        error: String::new(),
                    });
                }
                "batch_preview" => {
                    self.status = "选中替换预览已完成。".into();
                    if let Some(f) = &mut self.batch_form {
                        if !f.matches(&data["draft"]) {
                            continue;
                        }
                        f.nonce = text(&data, "nonce");
                        f.changes = list(&data, "changes");
                        f.error = if f.changes.is_empty() {
                            "所选文本无需修改。".into()
                        } else {
                            String::new()
                        };
                    }
                }
                "template" => {
                    self.status = "模板试写已完成。".into();
                    if let Some(f) = &mut self.template_form {
                        let draft = &data["draft"];
                        if text(draft, "key") == f.key
                            && list(draft, "values")
                                == f.values.iter().map(|s| json!(s)).collect::<Vec<_>>()
                            && text(draft, "sample") == f.sample
                        {
                            f.result = format!(
                                "{}（{} 处）",
                                text(&data, "sample"),
                                count(&data, "count")
                            );
                            f.built = Some(data);
                            f.error.clear();
                        }
                    }
                }
                "preview" => {
                    self.hits = list(&data["stats"], "rule_hits")
                        .iter()
                        .map(|r| {
                            r.as_array()
                                .and_then(|a| a.last())
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                        })
                        .collect();
                    self.preview_row = if list(&data["stats"], "changes").is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    self.status = format!("预览完成：{} 处变化。", count(&data, "total_changes"));
                    self.preview = Some(data.into());
                }
                "baseline" => {
                    self.baseline = data;
                    self.status = "初始底稿已检查。".into();
                }
                "import_read" => {
                    if list(&self.state, "rules").is_empty() {
                        self.submit("import_commit", data, "正在导入词表…", false);
                    } else {
                        self.confirm_import = Some(data);
                    }
                }
                "generate" | "apply" | "restore" | "export_text" | "export_rules" => {
                    self.status = text(&data, "message");
                    if let Some(v) = data.get("application").and_then(Value::as_str) {
                        self.application = v.into();
                    }
                    let output = text(&data, "output");
                    if !output.is_empty() {
                        let path = PathBuf::from(output);
                        self.output = if action == "export_text" {
                            Some(path)
                        } else {
                            path.parent().map(PathBuf::from)
                        };
                    }
                }
                _ => {}
            }
        }
    }
}

impl eframe::App for Desktop {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.events();
        let ctx = ui.ctx().clone();
        if self
            .path_changed
            .is_some_and(|t| t.elapsed() > Duration::from_millis(300))
            && !self.write_busy()
        {
            self.save_settings();
        }
        if self.path_changed.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.write_busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error = Some("请等待当前写入完成后再关闭。".into());
        }
        let modal = self.modal();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(18))
            .show(ui, |ui| {
                egui::Panel::bottom("status")
                    .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                        top: 12,
                        ..Default::default()
                    }))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let cancellable = self.busy.as_ref().is_some_and(|b| b.cancellable);
                            let reserve = 120.
                                + if self.output.is_some() { 160. } else { 0. }
                                + if cancellable { 128. } else { 0. };
                            if self.busy.is_some() {
                                ui.spinner();
                            }
                            let width = (ui.available_width() - reserve).max(100.);
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, 36.),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_width(width);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&self.status).size(13.).color(MUTED),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&self.status);
                                },
                            );
                            if let Some(path) = &self.output
                                && action(ui, true, "打开结果位置", ButtonKind::Quiet).clicked()
                            {
                                Self::open_folder(path);
                            }
                            if cancellable && secondary(ui, true, "取消任务").clicked() {
                                self.cancel();
                            }
                            if action(ui, true, "运行详情", ButtonKind::Quiet).clicked() {
                                self.show_logs = true;
                            }
                        });
                    });
                ui.add_enabled_ui(!modal && self.backend.is_some(), |ui| {
                    egui::Panel::top("header")
                        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                            bottom: 12,
                            ..Default::default()
                        }))
                        .show(ui, |ui| self.header(ui));
                    match self.tab {
                        0 => self.replacement(ui),
                        1 => self.search_page(ui),
                        _ => self.restore_page(ui),
                    }
                });
            });
        self.dialogs(&ctx);
        if let Some(probe) = &mut self.startup_probe {
            probe.frame(&ctx, self.loaded, self.connected, self.error.is_some());
        }
    }
}
pub fn table_header(ui: &mut egui::Ui, values: &[&str], widths: &[f32]) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 7, Color32::from_rgb(247, 250, 254));
    let mut x = rect.left();
    for (i, value) in values.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x + 12., rect.top()),
            egui::vec2((widths[i] - 24.).max(1.), rect.height()),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.add(
            egui::Label::new(RichText::new(*value).size(13.).color(MUTED))
                .selectable(false)
                .halign(egui::Align::LEFT)
                .truncate(),
        );
        x += widths[i];
    }
}

pub fn data_row(
    ui: &mut egui::Ui,
    index: usize,
    selected: bool,
    values: &[String],
    widths: &[f32],
) -> egui::Response {
    ui.push_id(index, |ui| {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::Button,
                ui.is_enabled(),
                selected,
                values.join(" | "),
            )
        });
        if ui.is_rect_visible(rect) {
            let fill = if selected {
                SOFT_BLUE
            } else if response.hovered() && ui.is_enabled() {
                Color32::from_rgb(243, 248, 254)
            } else if index.is_multiple_of(2) {
                Color32::WHITE
            } else {
                Color32::from_rgb(249, 251, 254)
            };
            ui.painter().rect_filled(rect, 5, fill);
            if selected {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.left_top() + egui::vec2(0., 7.),
                        egui::vec2(3., rect.height() - 14.),
                    ),
                    2,
                    BLUE,
                );
            }
            let mut x = rect.left();
            for (i, value) in values.iter().enumerate() {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(x + 12., rect.top()),
                    egui::vec2((widths[i] - 24.).max(1.), rect.height()),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                child.add(
                    egui::Label::new(RichText::new(value).size(15.5))
                        .selectable(false)
                        .halign(egui::Align::LEFT)
                        .truncate(),
                );
                x += widths[i];
            }
        }
        response
    })
    .inner
}

#[cfg(test)]
mod view_tests {
    use super::*;

    #[test]
    fn whole_row_preserves_ctrl_shift_and_double_click() {
        let ctx = egui::Context::default();
        let mut selection = Selection::default();
        let mut rects = [egui::Rect::NOTHING; 3];
        let mut draw = |mut events: Vec<egui::Event>, modifiers: egui::Modifiers, time: f64| {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let mut double = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 300.),
                    )),
                    events,
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    for (i, rect) in rects.iter_mut().enumerate() {
                        let response = data_row(
                            ui,
                            i,
                            selection.rows.contains(&i),
                            &[format!("row {i}"), "source".into()],
                            &[400., 200.],
                        );
                        *rect = response.rect;
                        if response.clicked() {
                            let keys = ui.input(|input| input.modifiers);
                            selection.click(i, keys.ctrl, keys.shift);
                        }
                        double |= response.double_clicked();
                    }
                },
            );
            (rects, selection.rows.clone(), double)
        };
        let positions = draw(vec![], egui::Modifiers::NONE, 0.).0;
        let click = |index: usize, modifiers: egui::Modifiers| {
            let pos = positions[index].left_center()
                + egui::vec2(if index == 2 { 430. } else { 30. }, 0.);
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            ]
        };
        assert_eq!(
            draw(click(0, egui::Modifiers::NONE), egui::Modifiers::NONE, 1.).1,
            std::collections::BTreeSet::from([0])
        );
        assert_eq!(
            draw(click(2, egui::Modifiers::CTRL), egui::Modifiers::CTRL, 2.).1,
            std::collections::BTreeSet::from([0, 2])
        );
        assert_eq!(
            draw(click(1, egui::Modifiers::SHIFT), egui::Modifiers::SHIFT, 3.).1,
            std::collections::BTreeSet::from([1, 2])
        );
        assert!(draw(click(1, egui::Modifiers::NONE), egui::Modifiers::NONE, 3.1).2);
    }
}
