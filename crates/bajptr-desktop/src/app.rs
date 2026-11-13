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
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ErrorRecovery {
    Baseline,
    Records,
}
pub struct Desktop {
    pub backend: Option<Backend>,
    pub(crate) data_path: Option<PathBuf>,
    pub pending: HashMap<u64, String>,
    pub obsolete: HashSet<u64>,
    pub busy: Option<Busy>,
    pub state: Arc<Value>,
    pub loaded: bool,
    pub connected: bool,
    pub game: String,
    pub skip: bool,
    pub path_dirty: bool,
    pub tab: usize,
    pub status: String,
    pub application: String,
    pub logs: Vec<crate::logging::Entry>,
    pub error: Option<String>,
    pub error_recovery: Option<(String, ErrorRecovery)>,
    pub settings_warning: Option<String>,
    pub confirm_record_reset: Option<String>,
    pub rule_selection: Option<usize>,
    pub reveal_rule: Option<usize>,
    pub search: Arc<Value>,
    pub keyword: String,
    pub search_input: SearchInput,
    pub search_source: String,
    pub selection: Selection,
    pub tables: Vec<Value>,
    pub sources_ready: bool,
    pub show_source_notices: bool,
    pub rule_form: Option<RuleForm>,
    pub scope_form: Option<ScopeForm>,
    pub template_form: Option<TemplateForm>,
    pub template_changed: Option<Instant>,
    pub text_form: Option<TextForm>,
    pub batch_form: Option<BatchForm>,
    pub combined_form: Option<CombinedForm>,
    pub batch_scope: Option<BatchScopeForm>,
    pub preview: Option<Arc<Value>>,
    pub preview_row: Option<usize>,
    pub show_edits: bool,
    pub edit_view: EditView,
    pub show_logs: bool,
    pub show_about: bool,
    pub confirm_backup: Option<Value>,
    pub confirm_restore: bool,
    pub confirm_baseline: Option<Value>,
    pub migration_notice: bool,
    pub migration_count: usize,
    pub migration_form: Option<MigrationForm>,
    migration_prompted: HashSet<String>,
    pub baseline_check: Value,
    pub baseline: Value,
    pub output_dir: PathBuf,
    pub hits: Vec<u64>,
    pub combined_hits: Vec<Value>,
    pub table_detail: Option<Value>,
    pub startup_probe: Option<crate::startup::Probe>,
}
impl Desktop {
    pub fn new(cc: &eframe::CreationContext<'_>, project: PathBuf, data: PathBuf) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        if !crate::theme::install(ctx) {
            rfd::MessageDialog::new()
                .set_title("无法加载中文字体")
                .set_description(
                    "未能读取 Windows 的微软雅黑或黑体。请安装或修复系统中文字体后重新打开工具。",
                )
                .set_level(rfd::MessageLevel::Error)
                .show();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return Self::initial();
        }
        let backend = Backend::start(&project, &data, ctx.clone());
        let mut app = Self::initial();
        app.data_path = Some(data.clone());
        app.output_dir = data.join("output");
        match backend {
            Ok(backend) => {
                if let Err(error) = std::fs::create_dir_all(&app.output_dir) {
                    let output = app.output_dir.clone();
                    app.report_file_error("open_directory", &output, error.into());
                }
                app.backend = Some(backend);
                app.submit("load", json!({}), "正在读取修改记录…");
            }
            Err(error) => {
                app.status = error.clone();
                app.error = Some(error);
            }
        }
        app
    }
    pub(crate) fn initial() -> Self {
        Self {
            backend: None,
            data_path: None,
            pending: HashMap::new(),
            obsolete: HashSet::new(),
            busy: None,
            state: Value::Null.into(),
            loaded: false,
            connected: false,
            game: String::new(),
            skip: true,
            path_dirty: false,
            tab: 0,
            status: "正在读取修改记录…".into(),
            application: "应用状态待核对，先预览".into(),
            logs: Vec::new(),
            error: None,
            error_recovery: None,
            settings_warning: None,
            confirm_record_reset: None,
            rule_selection: None,
            reveal_rule: None,
            search: Value::Null.into(),
            keyword: String::new(),
            search_input: SearchInput::default(),
            search_source: String::new(),
            selection: Selection::default(),
            tables: Vec::new(),
            sources_ready: false,
            show_source_notices: false,
            rule_form: None,
            scope_form: None,
            template_form: None,
            template_changed: None,
            text_form: None,
            batch_form: None,
            combined_form: None,
            batch_scope: None,
            preview: None,
            preview_row: None,
            show_edits: false,
            edit_view: EditView::default(),
            show_logs: false,
            show_about: false,
            confirm_backup: None,
            confirm_restore: false,
            confirm_baseline: None,
            migration_notice: false,
            migration_count: 0,
            migration_form: None,
            migration_prompted: HashSet::new(),
            baseline_check: Value::Null,
            baseline: Value::Null,
            output_dir: PathBuf::from("output"),
            hits: Vec::new(),
            combined_hits: Vec::new(),
            table_detail: None,
            startup_probe: crate::startup::Probe::from_environment(),
        }
    }
    pub fn restore_context_status(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.status = if self.path_dirty {
            "路径已更改，等待重新检查…".into()
        } else if self.tab == 1 && !self.search.is_null() {
            format!(
                "共找到 {} 处，已加载 {} 处",
                count(&self.search, "total"),
                slice(&self.search, "rows").len()
            )
        } else if self.connected {
            if self.tab == 0 && !self.application.is_empty() {
                self.application.clone()
            } else {
                "底稿已就绪。".into()
            }
        } else {
            "请选择游戏目录并检查底稿。".into()
        };
    }
    pub fn log(&mut self, message: String, level: Option<crate::logging::Level>) {
        if message.trim().is_empty() {
            return;
        }
        self.logs.push(crate::logging::Entry::new(message, level));
        if self.logs.len() > 2000 {
            self.logs.drain(..500);
        }
    }
    pub fn log_text(&self) -> String {
        self.logs
            .iter()
            .map(crate::logging::Entry::formatted)
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn export_logs(&self, path: &std::path::Path) -> bajptr_project::Result<()> {
        bajptr_project::storage::atomic_write(path, self.log_text().as_bytes(), None)?;
        Ok(())
    }
    pub fn game_path_changed(&mut self) {
        self.migration_notice = false;
        self.migration_form = None;
        self.migration_count = 0;
        self.path_dirty = true;
        self.connected = false;
        self.sources_ready = false;
        self.confirm_baseline = None;
        self.confirm_restore = false;
        self.baseline_check = Value::Null;
        self.baseline = Value::Null;
        self.preview = None;
        self.hits.clear();
        self.combined_hits.clear();
        self.search_input_changed();
        if self.busy.as_ref().is_some_and(|busy| {
            self.pending.get(&busy.id).is_some_and(|action| {
                matches!(action.as_str(), "check" | "baseline_rebuild_preview")
            })
        }) {
            let busy = self.busy.take().unwrap();
            self.obsolete.insert(busy.id);
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":busy.id}));
            }
        }
        self.status = "路径已更改，等待重新检查…".into();
    }
    pub fn search_input_changed(&mut self) {
        self.search = Value::Null.into();
        self.selection.clear();
        let stale: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, action)| matches!(action.as_str(), "search" | "search_more" | "text_open"))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.obsolete.insert(id);
            if self.busy.as_ref().is_some_and(|busy| busy.id == id) {
                self.busy = None;
            }
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":id}));
            }
        }
        self.restore_context_status();
        if self.busy.is_none() {
            self.status = "搜索条件已修改，请重新查找。".into();
        }
    }
    pub fn revision(&self) -> u64 {
        count(&self.state, "revision")
    }
    pub fn write_busy(&self) -> bool {
        self.busy.as_ref().is_some_and(|b| {
            !b.cancellable
                || self
                    .pending
                    .get(&b.id)
                    .is_some_and(|action| !bajptr_project::worker::read_only(action))
        })
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
            || self.combined_form.is_some()
            || self.batch_scope.is_some()
            || self.preview.is_some()
            || self.show_edits
            || self.confirm_backup.is_some()
            || self.confirm_restore
            || self.confirm_baseline.is_some()
            || self.migration_notice
            || self.migration_form.as_ref().is_some_and(|form| form.open)
            || self.error.is_some()
            || self.confirm_record_reset.is_some()
            || self.table_detail.is_some()
            || self.show_source_notices
            || self.show_about
    }
    pub fn submit(&mut self, action: &str, payload: Value, title: &str) {
        let cancellable = bajptr_project::worker::cancellable(action);
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
                Err(e) => self.backend_failed(e),
            }
        } else {
            self.error = Some("项目服务未连接，请点击“重新连接”。".into());
        }
    }
    fn backend_failed(&mut self, message: String) {
        self.migration_notice = false;
        self.migration_form = None;
        self.migration_count = 0;
        self.backend = None;
        self.busy = None;
        self.pending.clear();
        self.obsolete.clear();
        self.loaded = false;
        self.connected = false;
        self.sources_ready = false;
        self.path_dirty = false;
        self.template_changed = None;
        self.search = Value::Null.into();
        self.selection.clear();
        self.preview = None;
        self.preview_row = None;
        self.hits.clear();
        self.combined_hits.clear();
        self.tables.clear();
        self.baseline = Value::Null;
        self.baseline_check = Value::Null;
        self.confirm_baseline = None;
        self.confirm_restore = false;
        self.confirm_backup = None;
        self.confirm_record_reset = None;
        self.show_source_notices = false;
        if let Some(form) = &mut self.scope_form {
            form.request = None;
            form.error = "项目服务已停止，请重新连接。".into();
        }
        self.application = "应用状态待核对，先预览".into();
        self.log(message.clone(), Some(crate::logging::Level::Error));
        self.status = message;
        self.error = Some(format!(
            "{}\n重新连接将读取已保存记录，未保存的编辑需重新填写。若当时正在应用或恢复，请先核对游戏状态；原操作不会自动重试。",
            self.status
        ));
    }
    pub fn reconnect(&mut self, ctx: &egui::Context) {
        let Some(data) = self.data_path.clone() else {
            return;
        };
        match Backend::start(&data, &data, ctx.clone()) {
            Ok(backend) => {
                self.backend = Some(backend);
                self.rule_form = None;
                self.scope_form = None;
                self.template_form = None;
                self.template_changed = None;
                self.text_form = None;
                self.batch_form = None;
                self.combined_form = None;
                self.batch_scope = None;
                self.show_edits = false;
                self.table_detail = None;
                self.error = None;
                self.submit("load", json!({}), "正在重新读取已保存记录…");
            }
            Err(message) => self.backend_failed(message),
        }
    }
    pub fn background_tables(&mut self) {
        if self.sources_ready
            || !self.healthy()
            || self.pending.values().any(|a| a == "sources")
            || !self.connected
            || self.busy.is_some()
        {
            return;
        }
        self.sources_ready = false;
        self.submit("sources", json!({}), "正在核对来源表与词条范围…");
    }
    fn set_baseline(&mut self, baseline: Value) {
        if self.baseline["version_id"] != baseline["version_id"] {
            self.sources_ready = false;
        }
        self.baseline = baseline;
    }
    pub fn query_scope_sources(&mut self, form: &mut ScopeForm) {
        if self.write_busy() {
            return;
        }
        form.available.clear();
        form.error.clear();
        self.submit(
            "scope_sources",
            json!({"old":form.old,"mode":form.mode}),
            "正在查找匹配的来源表…",
        );
        form.request = self.busy.as_ref().map(|busy| busy.id);
    }
    pub fn cancel(&mut self) {
        if let Some(busy) = &self.busy
            && busy.cancellable
        {
            let writes = self
                .pending
                .get(&busy.id)
                .is_some_and(|action| !bajptr_project::worker::read_only(action));
            // A write may already have passed its commit point. Wait for the
            // actual result instead of discarding it and claiming cancellation.
            if !writes {
                self.obsolete.insert(busy.id);
                if let Some(form) = &mut self.scope_form
                    && form.request == Some(busy.id)
                {
                    form.request = None;
                    form.error = "已取消读取，可点击刷新重试。".into();
                }
            }
            if let Some(backend) = &mut self.backend {
                let _ = backend.send("cancel", json!({"target":busy.id}));
            }
            self.status = if writes {
                "正在请求取消；已进入提交阶段时将完成写入。"
            } else {
                "正在取消，保留上次结果…"
            }
            .into();
        }
    }
    pub fn save_settings(&mut self) {
        self.path_dirty = false;
        self.connected = false;
        self.sources_ready = false;
        self.show_source_notices = false;
        self.confirm_baseline = None;
        self.baseline_check = Value::Null;
        self.submit(
            "settings",
            json!({"game":self.game,"skip_kana":self.skip,"revision":self.revision()}),
            "正在保存设置…",
        );
    }
    pub fn apply_state(&mut self, value: Value) {
        if value["settings_warning"] != self.state["settings_warning"] {
            self.settings_warning = value["settings_warning"].as_str().map(str::to_string);
        }
        let changed = count(&value, "revision") != self.revision()
            || value["version"] != self.state["version"];
        let game_changed = text(&value, "game") != text(&self.state, "game");
        if !self.path_dirty {
            self.game = text(&value, "game");
        }
        self.skip = value
            .get("skip_kana")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if changed {
            self.edit_view.selected = None;
            self.hits.clear();
            self.combined_hits.clear();
            self.preview = None;
            self.rule_selection = None;
            self.reveal_rule = None;
            if let Some(form) = &mut self.batch_scope {
                form.selected = None;
                form.error.clear();
            }
        }
        if game_changed {
            self.sources_ready = false;
            self.show_source_notices = false;
            self.edit_view = EditView::default();
            self.batch_scope = None;
            self.search_source.clear();
            self.rule_selection = None;
            self.reveal_rule = None;
            self.search = Value::Null.into();
            self.selection.clear();
            self.tables.clear();
            self.baseline = Value::Null;
            self.confirm_baseline = None;
            self.baseline_check = Value::Null;
        }
        if let Some(search) = value.get("search").filter(|search| {
            search.is_null() || (!self.search.is_null() && search["nonce"] == self.search["nonce"])
        }) {
            if search.is_null() {
                self.selection.clear();
            }
            self.search = search.clone().into();
        }
        self.application = text(&value, "application");
        self.migration_count = slice(&value, "edits")
            .iter()
            .filter(|e| e["active"] == false)
            .count();
        self.state = value.into();
    }
    pub fn open_text(&mut self, index: usize) {
        if !self.connected || self.busy.is_some() || self.modal() {
            return;
        }
        self.submit(
            "text_open",
            json!({"index":index,"search_nonce":text(&self.search,"nonce")}),
            "正在读取这段文本…",
        );
    }
    pub fn open_migration(&mut self) {
        let revision = self.revision();
        let version = text(&self.baseline, "version_id");
        if let Some(form) = &mut self.migration_form
            && form.plan["revision"] == revision
            && form.plan["version"] == version
        {
            form.open = true;
            return;
        }
        self.migration_form = None;
        self.submit(
            "migration_start",
            json!({"revision":revision}),
            "正在核对旧版修改…",
        );
    }
    pub fn open_batch(&mut self, entry: &Value) {
        self.submit(
            if entry["id"].is_string() {
                "batch_open"
            } else {
                "text_open"
            },
            json!({"id":entry["id"],"edit_index":entry["index"]}),
            "正在读取批量修改…",
        );
    }
    pub fn open_combined(&mut self, rule: &Value) {
        self.submit(
            "combined_open",
            json!({"old":rule["old"],"new":rule["new"],"revision":self.revision()}),
            "正在读取替换内容与范围…",
        );
    }
    pub fn open_batch_scope(&mut self, id: &str) {
        self.batch_scope = Some(BatchScopeForm {
            id: id.into(),
            ..Default::default()
        });
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
    pub fn open_data_folder(&mut self, child: &str) {
        let Some(data) = &self.data_path else {
            return;
        };
        let folder = data.join(child);
        match std::fs::create_dir_all(&folder) {
            Ok(()) => Self::open_folder(&folder),
            Err(error) => self.report_file_error("open_directory", &folder, error.into()),
        }
    }
    pub fn report_file_error(
        &mut self,
        action: &str,
        path: &std::path::Path,
        error: bajptr_project::Error,
    ) {
        self.log(
            format!("{}：{error}", path.display()),
            Some(crate::logging::Level::Error),
        );
        self.error = Some(format!(
            "{}\n位置：{}",
            bajptr_project::diagnostics::user_message(action, &error),
            path.display()
        ));
    }
    pub fn show_recovery_error(&mut self, message: String, recovery: ErrorRecovery) {
        self.error_recovery = Some((message.clone(), recovery));
        self.error = Some(message);
    }
    pub fn events(&mut self) {
        let (events, disconnected) = self.backend.as_ref().map(Backend::poll).unwrap_or_default();
        if let Some(event) = events
            .iter()
            .find(|event| event["event"] == "backend_closed")
        {
            let detail = text(event, "detail");
            if !detail.is_empty() {
                self.log(detail, Some(crate::logging::Level::Error));
            }
            self.backend_failed(text(event, "message"));
            return;
        }
        if disconnected {
            self.backend_failed("项目服务连接已中断。".into());
            return;
        }
        for event in events {
            let id = count(&event, "id");
            let kind = text(&event, "event");
            if kind == "progress" || kind == "detail" {
                let message = text(&event, "message");
                if self.busy.as_ref().is_some_and(|b| b.id == id) {
                    self.status = message.clone();
                }
                let level = serde_json::from_value(event["level"].clone())
                    .unwrap_or(crate::logging::Level::Info);
                self.log(message, Some(level));
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
            if matches!(action.as_str(), "check" | "baseline_rebuild_preview")
                && (self.path_dirty || text(&self.state, "game") != self.game)
            {
                self.status = "路径已更改，等待重新检查…".into();
                continue;
            }
            if kind == "cancelled" {
                if let Some(form) = &mut self.scope_form
                    && form.request == Some(id)
                {
                    form.request = None;
                    form.error = "已取消读取，可点击刷新重试。".into();
                }
                self.status = "已取消，保留上次结果。".into();
                continue;
            }
            if kind == "error" {
                let message = text(&event, "message");
                let detail = text(&event, "detail");
                self.log(
                    if detail.is_empty() {
                        message.clone()
                    } else {
                        detail
                    },
                    Some(crate::logging::Level::Error),
                );
                self.status = message.clone();
                match action.as_str() {
                    "combined_save" => {
                        if let Some(form) = &mut self.combined_form {
                            form.error = message;
                        }
                    }
                    "settings" => {
                        self.path_dirty = self.game != text(&self.state, "game");
                        self.error = Some(message);
                    }
                    "scope_sources" => {
                        if let Some(form) = &mut self.scope_form
                            && form.request == Some(id)
                        {
                            form.request = None;
                            form.error = message.clone();
                        }
                    }
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
                        if action == "batch_preview" {
                            self.restore_context_status();
                        }
                    }
                    "migration_commit" => {
                        if let Some(form) = &mut self.migration_form {
                            form.error = message;
                        }
                    }
                    "remove_batch_member" => {
                        if let Some(form) = &mut self.batch_scope {
                            form.error = message.clone();
                        }
                    }
                    "template" => {
                        if let Some(f) = &mut self.template_form {
                            f.error = message.clone();
                        }
                        self.restore_context_status();
                    }
                    "prepare_patch" => {
                        if let Some(preview) = &mut self.preview {
                            let preview = Arc::make_mut(preview);
                            preview["preparation_error"] = json!(message);
                            preview["prepared"] = json!(false);
                        }
                    }
                    "check" => {
                        self.connected = false;
                        self.baseline = Value::Null;
                        self.baseline_check = Value::Null;
                        self.confirm_baseline = None;
                        self.show_recovery_error(
                            format!("检查底稿失败：{message}"),
                            if self.healthy() {
                                ErrorRecovery::Baseline
                            } else {
                                ErrorRecovery::Records
                            },
                        );
                    }
                    "baseline_commit" | "baseline_rebuild_commit" => {
                        self.connected = false;
                        self.baseline = Value::Null;
                        self.baseline_check = Value::Null;
                        self.show_recovery_error(message, ErrorRecovery::Baseline);
                    }
                    "baseline_rebuild_preview" | "baseline" | "restore" => {
                        self.show_recovery_error(message, ErrorRecovery::Baseline)
                    }
                    "reset_record" => self.show_recovery_error(message, ErrorRecovery::Records),
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
                "promotion_start" => {
                    self.apply_state(data["state"].clone());
                    self.status = if count(&data, "count") > 0 {
                        format!(
                            "已整理 {} 个整表范围，另有 {} 个范围保留指定文本。",
                            count(&data, "count"),
                            count(&data, "retained")
                        )
                    } else {
                        "没有可转换的整表范围，现有批量修改已保留。".into()
                    };
                    if data["backup"].is_string() {
                        self.log(
                            format!("整理前的记录已备份：{}", text(&data, "backup")),
                            None,
                        );
                    }
                    for item in slice(&data, "retained_items") {
                        self.log(
                            format!(
                                "保留批量修改：{} · {} → {}：{}",
                                text(item, "label"),
                                text(item, "old"),
                                text(item, "new"),
                                text(item, "reason")
                            ),
                            None,
                        );
                    }
                }
                "combined_open" => {
                    self.combined_form = Some(CombinedForm::new(data));
                    self.status = "正在编辑替换。".into();
                }
                "load" => {
                    self.apply_state(data);
                    self.loaded = true;
                    for message in list(&self.state, "messages")
                        .iter()
                        .filter_map(Value::as_str)
                    {
                        self.log(message.into(), Some(crate::logging::Level::Warning));
                    }
                    if !self.healthy() {
                        let errors = self.state["errors"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .map(|(name, error)| {
                                format!("{name}：{}", error.as_str().unwrap_or(""))
                            })
                            .collect::<Vec<_>>();
                        for error in errors {
                            self.log(error, Some(crate::logging::Level::Error));
                        }
                        self.show_recovery_error(
                            "部分记录无法读取，原文件已保留。".into(),
                            ErrorRecovery::Records,
                        );
                        self.connected = false;
                        self.status = "记录文件读取失败，请先处理损坏文件。".into();
                    } else if !self.game.is_empty() {
                        self.submit("check", json!({}), "正在检查游戏目录…");
                    } else {
                        self.status = "请选择游戏目录，或恢复已有备份。".into();
                    }
                }
                "settings" => {
                    self.apply_state(data);
                    if !self.path_dirty {
                        self.submit("check", json!({}), "正在检查游戏目录…");
                    }
                }
                "check" => {
                    if self.path_dirty || text(&self.state, "game") != self.game {
                        self.status = "路径已更改，等待重新检查…".into();
                        continue;
                    }
                    self.baseline_check = data["baseline_status"].clone();
                    self.set_baseline(self.baseline_check["baseline"].clone());
                    self.connected = data["connected"] == true;
                    self.application = text(&data, "application");
                    match text(&self.baseline_check, "state").as_str() {
                        "ready" => {
                            self.confirm_baseline = None;
                            self.migration_count = count(&data, "migration_count") as usize;
                            let prompt_key =
                                format!("{}:{}", self.game, text(&self.baseline, "version_id"));
                            if self.migration_count > 0
                                && self.migration_prompted.insert(prompt_key)
                            {
                                self.migration_notice = true;
                            }
                            self.status = "底稿已就绪。".into();
                            self.background_tables();
                        }
                        "blocked" => {
                            self.confirm_baseline = None;
                            self.baseline = Value::Null;
                            self.status = "未找到初始底稿，当前资源已被本工具修改。".into();
                            self.show_recovery_error("当前资源是本工具替换后的内容，不能作为初始底稿。请先通过游戏修复资源，或找回原底稿，再点击“检查底稿”。".into(),ErrorRecovery::Baseline);
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
                "baseline_rebuild_preview" => {
                    self.confirm_baseline = Some(data);
                    self.status = "已找到校验通过的原始资源，请确认重建底稿。".into();
                }
                "baseline_commit" | "baseline_rebuild_commit" => {
                    if let Some(folder) = data["damaged_folder"].as_str() {
                        self.log(
                            format!("重建前的损坏底稿已保留：{folder}"),
                            Some(crate::logging::Level::Warning),
                        );
                    }
                    self.set_baseline(data);
                    self.sources_ready = false;
                    self.confirm_baseline = None;
                    self.search = Value::Null.into();
                    self.selection.clear();
                    self.preview = None;
                    self.hits.clear();
                    self.combined_hits.clear();
                    self.tables.clear();
                    self.submit("check", json!({}), "底稿已保存，正在核对…");
                }
                "reset_record" => {
                    self.apply_state(data["state"].clone());
                    self.sources_ready = false;
                    self.selection.clear();
                    self.log(
                        format!("损坏记录已归档：{}", text(&data, "archived")),
                        Some(crate::logging::Level::Warning),
                    );
                    self.status = "记录已清空，原文件已保留。".into();
                    if self.healthy() && !self.game.is_empty() && !self.path_dirty {
                        self.submit("check", json!({}), "记录已重置，正在重新核对底稿…");
                    } else if !self.healthy() {
                        self.show_recovery_error(
                            "已归档选中的记录，其余文件仍有读取错误。请继续检查。".into(),
                            ErrorRecovery::Records,
                        );
                    }
                }
                "migration_preview" => {
                    self.migration_form = Some(MigrationForm::new(data));
                    self.status = "已核对旧版修改，请选择要迁移的文本。".into();
                }
                "migration_start" => {
                    self.apply_state(data["state"].clone());
                    let review = list(&data["plan"], "items");
                    self.migration_form =
                        (!review.is_empty()).then(|| MigrationForm::new(data["plan"].clone()));
                    self.status = if review.is_empty() {
                        format!("已自动迁移 {} 段。", count(&data, "automatic_count"))
                    } else {
                        format!(
                            "已自动迁移 {} 段，{} 段待核对。",
                            count(&data, "automatic_count"),
                            review.len()
                        )
                    };
                    if data["backup"].is_string() {
                        self.log(
                            format!("迁移前的修改记录已备份：{}", text(&data, "backup")),
                            None,
                        );
                    }
                    if count(&data, "unavailable_count") > 0 {
                        self.log(
                            format!(
                                "{} 段原文已变化、缺失或已有新版修改，旧记录已保留。",
                                count(&data, "unavailable_count")
                            ),
                            Some(crate::logging::Level::Warning),
                        );
                    }
                }
                "migration_commit" => {
                    self.apply_state(data["state"].clone());
                    self.migration_form = None;
                    self.migration_count = slice(&self.state, "edits")
                        .iter()
                        .filter(|e| e["active"] == false)
                        .count();
                    self.status = format!("已迁移 {} 段文本。", count(&data, "count"));
                    self.log(
                        format!("迁移前的修改记录已备份：{}", text(&data, "backup")),
                        None,
                    );
                }
                "tables" => {
                    self.tables = data.as_array().cloned().unwrap_or_default();
                }
                "scope_sources" => {
                    if let Some(form) = &mut self.scope_form
                        && form.request == Some(id)
                    {
                        form.request = None;
                        form.available = data.as_array().cloned().unwrap_or_default();
                    }
                    self.restore_context_status();
                }
                "sources" => {
                    self.apply_state(data["state"].clone());
                    self.tables = list(&data, "tables");
                    self.sources_ready = data["missing_history"] != true;
                    if !self.sources_ready {
                        self.error = Some("没有找到旧词条上次记录的来源表。请逐条选择来源范围后再预览；未自动把当前新表加入旧词条。".into());
                    } else if !slice(&self.state, "source_notices").is_empty() {
                        self.show_source_notices = true;
                    }
                    if count(&data, "migrated") > 0 {
                        self.log(
                            format!(
                                "已将 {} 条旧词条固定到上次记录的来源范围。",
                                count(&data, "migrated")
                            ),
                            None,
                        );
                    }
                    self.restore_context_status();
                }
                "sources_ack" => {
                    self.apply_state(data);
                    self.show_source_notices = false;
                }
                "save_rule"
                | "remove_rule"
                | "history"
                | "text_save"
                | "batch_save"
                | "remove_edit"
                | "remove_batch"
                | "remove_batch_member"
                | "import_commit" => {
                    self.apply_state(data);
                    if action == "save_rule" {
                        self.rule_form = None;
                        if !self.sources_ready {
                            self.background_tables();
                        }
                    }
                    if action == "text_save" {
                        self.text_form = None;
                    }
                    if action == "batch_save" {
                        self.batch_form = None;
                    }
                    self.status = "修改已保存。".into();
                }
                "combined_save" | "combined_remove" => {
                    self.apply_state(data);
                    self.combined_form = None;
                    self.rule_selection = None;
                    self.status = "修改已保存。".into();
                }
                "search" => {
                    self.search = data.into();
                    self.selection.clear();
                    self.search_source = text(&self.search, "source");
                    self.restore_context_status();
                }
                "search_more" => {
                    self.search = data.into();
                    self.restore_context_status();
                }
                "text_open" => {
                    self.status = "正在编辑文本。".into();
                    self.text_form = Some(TextForm::new(data));
                }
                "batch_open" => {
                    self.status = "正在编辑批量修改。".into();
                    self.batch_form = Some(BatchForm {
                        group_id: text(&data, "id"),
                        indices: (0..count(&data, "count") as usize).collect(),
                        old: text(&data, "old"),
                        new: text(&data, "new"),
                        nonce: String::new(),
                        changes: Vec::new(),
                        error: String::new(),
                    });
                }
                "batch_preview" => {
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
                    self.restore_context_status();
                }
                "template" => {
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
                    self.restore_context_status();
                }
                "preview" => {
                    if let Some(application) = data["application"].as_str() {
                        self.application = application.into();
                    }
                    self.combined_hits = list(&data["stats"], "combined_hits");
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
                    let can_prepare = count(&data, "total_changes") > 0
                        && count(&data["stats"], "parse_failed") == 0;
                    let nonce = text(&data, "nonce");
                    self.preview = Some(data.into());
                    if can_prepare {
                        self.submit("prepare_patch", json!({"nonce":nonce}), "正在准备应用…");
                    }
                }
                "prepare_patch" => {
                    if let Some(preview) = &mut self.preview
                        && preview["nonce"] == data["nonce"]
                    {
                        let preview = Arc::make_mut(preview);
                        preview["prepared"] = data["ready"].clone();
                        preview["preparation_error"] = json!("");
                        preview["prepared_reused"] = data["reused"].clone();
                    }
                    self.status = "已准备就绪。".into();
                }
                "baseline" => {
                    self.set_baseline(data);
                    self.status = "初始底稿已检查。".into();
                }
                "backup_read" => {
                    self.confirm_backup = Some(data);
                }
                "backup_restore" => {
                    self.path_dirty = false;
                    self.apply_state(data["state"].clone());
                    self.connected = false;
                    self.sources_ready = false;
                    self.tables.clear();
                    self.baseline = Value::Null;
                    self.baseline_check = Value::Null;
                    self.confirm_baseline = None;
                    self.confirm_backup = None;
                    self.error = None;
                    self.error_recovery = None;
                    self.confirm_record_reset = None;
                    self.rule_selection = None;
                    self.edit_view = EditView::default();
                    self.selection.clear();
                    self.search = Value::Null.into();
                    self.preview = None;
                    self.rule_form = None;
                    self.text_form = None;
                    self.batch_form = None;
                    self.combined_form = None;
                    self.batch_scope = None;
                    self.scope_form = None;
                    self.table_detail = None;
                    self.show_source_notices = false;
                    self.template_form = None;
                    self.template_changed = None;
                    self.show_edits = false;
                    self.migration_form = None;
                    self.migration_notice = false;
                    self.migration_prompted.clear();
                    self.log(
                        format!("恢复备份前的数据已保留：{}", text(&data, "recovery")),
                        None,
                    );
                    self.status = "备份已恢复，请检查游戏目录。".into();
                    if !self.healthy() {
                        self.show_recovery_error(
                            "备份已恢复，但应用状态记录仍有读取错误，请查看日志并修复。".into(),
                            ErrorRecovery::Records,
                        );
                    } else if !self.game.is_empty() {
                        self.submit("check", json!({}), "备份已恢复，正在检查游戏目录…");
                    }
                }
                "apply" | "restore" | "export_text" | "export_rules" | "backup_create" => {
                    self.status = text(&data, "message");
                    let warning = text(&data, "warning");
                    if !warning.is_empty() {
                        self.log(warning.clone(), Some(crate::logging::Level::Warning));
                        self.error = Some(warning);
                    }
                    if let Some(v) = data.get("application").and_then(Value::as_str) {
                        self.application = v.into();
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
        if self.busy.is_some() || !self.pending.is_empty() {
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
                            let application = (self.connected
                                && self.busy.is_none()
                                && self.application != self.status)
                                .then_some(self.application.as_str())
                                .filter(|label| !label.is_empty());
                            let application_width = application.map_or(0., |label| {
                                ui.painter()
                                    .layout_no_wrap(
                                        label.into(),
                                        egui::FontId::proportional(13.),
                                        MUTED,
                                    )
                                    .size()
                                    .x
                                    .min(260.)
                            });
                            let reconnect = self.data_path.is_some()
                                && (self.backend.is_none()
                                    || (!self.loaded && self.busy.is_none()));
                            let reserve = 120.
                                + if reconnect { 128. } else { 0. }
                                + if cancellable { 128. } else { 0. }
                                + if application.is_some() {
                                    application_width + ui.spacing().item_spacing.x
                                } else {
                                    0.
                                };
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
                            if let Some(label) = application {
                                ui.add_sized(
                                    [application_width, 36.],
                                    egui::Label::new(RichText::new(label).size(13.).color(MUTED))
                                        .halign(egui::Align::RIGHT)
                                        .truncate(),
                                )
                                .on_hover_text(label);
                            }
                            if cancellable && secondary(ui, true, "取消任务").clicked() {
                                self.cancel();
                            }
                            if reconnect && secondary(ui, true, "重新连接").clicked() {
                                self.reconnect(&ctx);
                            }
                            if action(ui, true, "查看日志", ButtonKind::Quiet).clicked() {
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
                        _ => self.tools_page(ui),
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
    fn empty_rule_table_only_suggests_adding_when_the_action_is_available() {
        for case in ["loading", "failed", "empty"] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            app.loaded = case != "loading";
            app.state = json!({"rules":[],"batches":[],"errors":if case=="failed" {json!({"rules.tsv":"invalid"})} else {json!({})}}).into();
            if case == "loading" {
                app.pending.insert(7, "load".into());
                app.busy = Some(Busy {
                    id: 7,
                    cancellable: true,
                });
            }
            let mut frame = 0;
            let mut draw = |app: &mut Desktop, events| {
                frame += 1;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 620.),
                        )),
                        time: Some(f64::from(frame) / 60.),
                        events,
                        ..Default::default()
                    },
                    |ui| app.replacement(ui),
                );
                output.textures_delta.clear();
                output
                    .shapes
                    .into_iter()
                    .filter_map(|shape| match shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_string(),
                            egui::Rect::from_min_size(text.pos, text.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let mut labels = Vec::new();
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            assert_eq!(
                labels
                    .iter()
                    .any(|(text, _)| text.contains("点击「添加词条」")),
                case == "empty"
            );
            if case == "loading" {
                assert!(labels.iter().any(|(text, _)| text.contains("正在读取词表")));
            }
            if case == "failed" {
                assert!(
                    labels
                        .iter()
                        .any(|(text, _)| text.contains("记录文件读取失败"))
                );
            }
            let position = labels
                .iter()
                .find(|(text, _)| text == "添加词条")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            assert_eq!(app.rule_form.is_some(), case == "empty");
        }
    }

    #[test]
    fn file_error_dialog_uses_chinese_advice_and_retains_system_details_in_logs() {
        let mut app = Desktop::initial();
        app.report_file_error(
            "export_logs",
            std::path::Path::new("output/log.txt"),
            bajptr_project::Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "raw system detail",
            )),
        );
        assert!(app.error.as_ref().unwrap().contains("权限"));
        assert!(!app.error.as_ref().unwrap().contains("raw system detail"));
        assert!(app.log_text().contains("raw system detail"));
    }

    #[test]
    fn damaged_record_popup_is_not_replaced_by_an_automatic_baseline_check() {
        let data = std::env::temp_dir().join(format!(
            "bajptr-record-popup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(data.join(".gui")).unwrap();
        std::fs::write(
            data.join(".gui/settings.json"),
            br#"{"format":2,"game":"invalid-game","skip_kana":true}"#,
        )
        .unwrap();
        std::fs::write(data.join(".gui/rules.tsv"), b"broken").unwrap();
        let mut app = Desktop::initial();
        app.backend = Some(Backend::start(&data, &data, egui::Context::default()).unwrap());
        app.submit("load", json!({}), "loading");
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.busy.is_some() && Instant::now() < deadline {
            app.events();
            std::thread::yield_now();
        }
        assert!(app.loaded);
        assert!(!app.healthy());
        assert!(
            app.pending.is_empty(),
            "a failed record load must wait for recovery before checking the game"
        );
        assert_eq!(
            app.error_recovery
                .as_ref()
                .map(|(_, recovery)| matches!(recovery, ErrorRecovery::Records)),
            Some(true)
        );
        drop(app);
        bajptr_project::storage::retire(&data).unwrap();
    }

    #[test]
    fn recovery_actions_live_in_error_dialogs_and_settings_warning_can_be_dismissed() {
        for width in [900., 1080.] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            app.loaded = true;
            app.tab = 2;
            app.data_path = Some(PathBuf::from("test-data"));
            let state = json!({"game":"test-game","revision":0,"settings_warning":"设置读取失败：格式无效","errors":{"text-edits.json":"invalid JSON"}});
            app.apply_state(state.clone());
            let mut frame = 0;
            let mut draw = |app: &mut Desktop, events| {
                frame += 1;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 620.),
                        )),
                        time: Some(f64::from(frame) / 60.),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        app.header(ui);
                        app.tools_page(ui);
                        app.dialogs(ui.ctx());
                    },
                );
                output.textures_delta.clear();
                output
                    .shapes
                    .into_iter()
                    .filter_map(|shape| match shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_owned(),
                            egui::Rect::from_min_size(text.pos, text.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let click = |pos, pressed| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ]
            };
            let mut labels = Vec::new();
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            assert!(labels.iter().any(|(text, _)| text == "关闭警告"));
            for label in ["打开数据目录", "清空记录…", "打开备份位置", "尝试重建"]
            {
                assert!(
                    !labels.iter().any(|(text, _)| text == label),
                    "recovery control must not be a permanent page action: {label}"
                );
            }
            let position = labels
                .iter()
                .find(|(text, _)| text == "关闭警告")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                draw(&mut app, click(position, pressed));
            }
            assert!(app.settings_warning.is_none());
            app.apply_state(state.clone());
            assert!(
                app.settings_warning.is_none(),
                "unchanged state must not reopen a dismissed warning"
            );
            app.show_recovery_error("底稿损坏".into(), ErrorRecovery::Baseline);
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            for label in ["打开备份位置", "尝试重建"] {
                assert!(labels.iter().any(|(text, _)| text == label));
            }
            assert!(!labels.iter().any(|(text, _)| text == "打开数据目录"));
            app.error = None;
            app.error_recovery = None;
            app.show_recovery_error("记录文件损坏".into(), ErrorRecovery::Records);
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            assert!(labels.iter().any(|(text, _)| text == "打开数据目录"));
            assert!(labels.iter().any(|(text, _)| text == "单条与批量修改"));
            assert!(!labels.iter().any(|(text, _)| text == "尝试重建"));
            let position = labels
                .iter()
                .find(|(text, _)| text == "清空记录…")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                draw(&mut app, click(position, pressed));
            }
            assert_eq!(app.confirm_record_reset.as_deref(), Some("text-edits.json"));
            assert!(app.pending.is_empty(), "reset requires confirmation");
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            assert!(labels.iter().any(|(text, _)| text == "清空单条与批量修改"));
            assert!(labels.iter().any(|(text, _)| text == "确认清空"));
            app.confirm_record_reset = None;
            app.apply_state(
                json!({"game":"test-game","revision":1,"errors":{},"settings_warning":null}),
            );
            assert!(app.healthy());
            assert!(app.settings_warning.is_none());
        }
    }

    #[test]
    fn migration_review_confirms_then_advances_and_keeps_long_lists_above_actions() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        let mut items = vec![
            json!({"group_key":"subset","original":"原文二","replacement":"结果二","table":"LocalizeDBSchema","candidates":[{"row":["Text",1,0,"原文二"],"context":"上下文甲"},{"row":["Text",2,0,"原文二"],"context":"上下文乙"}]}),
        ];
        for n in 0..504 {
            items.push(json!({"group_key":format!("item-{n}"),"original":format!("列表原文{n}"),"replacement":format!("列表结果{n}"),"table":"LocalizeDBSchema","candidates":[{"row":["Text",n*2+3,0,format!("列表原文{n}")],"context":"上下文甲"},{"row":["Text",n*2+4,0,format!("列表原文{n}")],"context":"上下文乙"}]}));
        }
        app.migration_form = Some(MigrationForm::new(
            json!({"nonce":"plan","revision":0,"items":items}),
        ));
        assert_eq!(app.migration_form.as_ref().unwrap().confirmed_count(), 0);
        let mut frame = 0;
        let mut draw = |app: &mut Desktop, events| {
            frame += 1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 620.),
                    )),
                    time: Some(f64::from(frame) / 60.),
                    events,
                    ..Default::default()
                },
                |ui| app.migration_dialogs(ui.ctx()),
            );
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_owned(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                        shape.clip_rect,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let mut labels = Vec::new();
        for _ in 0..3 {
            labels = draw(&mut app, vec![]);
        }
        let save = labels
            .iter()
            .find(|(t, ..)| t == "保存已确认（0 段）")
            .unwrap()
            .1;
        let footer = labels
            .iter()
            .find(|(t, ..)| t.starts_with("保存时自动备份"))
            .unwrap()
            .1;
        assert!(footer.bottom() < 620.);
        let rows: Vec<_> = labels
            .iter()
            .filter(|(t, ..)| t.starts_with("列表原文"))
            .collect();
        assert!(!rows.is_empty());
        for (_, rect, clip) in rows {
            assert!(clip.bottom() < save.top());
            assert!(rect.left() < 150.);
        }
        let click = |position: egui::Pos2, pressed| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let position = labels
            .iter()
            .find(|(t, ..)| t == "1 处")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            draw(&mut app, click(position, pressed));
        }
        labels = draw(&mut app, vec![]);
        let position = labels
            .iter()
            .find(|(t, ..)| t == "确认本组（1 处）")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            draw(&mut app, click(position, pressed));
        }
        assert_eq!(app.migration_form.as_ref().unwrap().confirmed_count(), 1);
        assert_eq!(app.migration_form.as_ref().unwrap().selected, Some(1));
        assert_eq!(
            app.migration_form.as_ref().unwrap().payload()["choices"][0],
            json!({"item":0,"candidate":0})
        );
        app.migration_form.as_mut().unwrap().filter = "列表原文499".into();
        for _ in 0..3 {
            labels = draw(&mut app, vec![]);
        }
        assert_eq!(app.migration_form.as_ref().unwrap().selected, Some(500));
        assert!(labels.iter().any(|(t, ..)| t == "列表结果499"));
        app.migration_form.as_mut().unwrap().filter = "没有这个文本".into();
        for _ in 0..3 {
            labels = draw(&mut app, vec![]);
        }
        assert_eq!(app.migration_form.as_ref().unwrap().selected, None);
        assert!(labels.iter().any(|(t, ..)| t == "没有匹配的文本。"));
        assert!(!labels.iter().any(|(t, ..)| t == "列表结果499"));
        app.migration_form.as_mut().unwrap().filter.clear();
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let position = egui::pos2(180., 300.);
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -600.),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..6 {
            labels = draw(&mut app, vec![]);
        }
        for (t, _, clip) in &labels {
            if t.starts_with("列表原文") {
                assert!(clip.bottom() < save.top());
            }
        }
        assert_eq!(app.migration_form.as_ref().unwrap().confirmed_count(), 1);
        assert!(labels.iter().any(|(t, ..)| t == "保存已确认（1 段）"));
        let position = labels
            .iter()
            .find(|(t, ..)| t == "已确认 1 组")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            draw(&mut app, click(position, pressed));
        }
        for _ in 0..3 {
            labels = draw(&mut app, vec![]);
        }
        assert!(labels.iter().any(|(t, ..)| t == "重新核对这组"));
        app.migration_form.as_mut().unwrap().plan["version"] = json!("test-version");
        app.baseline = json!({"version_id":"test-version"});
        app.migration_form.as_mut().unwrap().open = false;
        assert!(!app.modal());
        app.open_migration();
        assert!(app.migration_form.as_ref().unwrap().open);
        assert_eq!(app.migration_form.as_ref().unwrap().confirmed_count(), 1);
        assert!(app.pending.is_empty());
    }
    #[test]
    fn search_toolbar_preserves_headers_and_row_positions_and_status_follows_navigation() {
        for width in [900., 1080.] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            app.loaded = true;
            app.connected = true;
            app.tab = 1;
            let mut frame = 0;
            let mut draw = |app: &mut Desktop, events| {
                frame += 1;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 680.),
                        )),
                        time: Some(f64::from(frame) / 60.),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        app.header(ui);
                        if app.tab == 1 {
                            app.search_page(ui);
                        } else {
                            app.replacement(ui);
                        }
                    },
                );
                output.textures_delta.clear();
                output
                    .shapes
                    .into_iter()
                    .filter_map(|shape| match shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_owned(),
                            egui::Rect::from_min_size(text.pos, text.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let empty = draw(&mut app, vec![]);
            assert!(
                empty
                    .iter()
                    .any(|(text, _)| text == "输入关键词查找游戏文本")
            );
            assert!(!empty.iter().any(|(text, _)| text == "导出底稿文本"));
            app.search = Arc::new(
                json!({"nonce":"fixed","total":60,"rows":(0..60).map(|i|json!({"text":format!("result-{i:03}"),"label":"来源","table":"test"})).collect::<Vec<_>>()}),
            );
            let mut labels = Vec::new();
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            let row = labels
                .iter()
                .find(|(text, _)| text == "result-000")
                .unwrap()
                .1;
            assert!(!labels.iter().any(|(text, _)| text == "替换选中文本"));
            app.selection.click(0, false, false);
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            let selected_row = labels
                .iter()
                .find(|(text, _)| text == "result-000")
                .unwrap()
                .1;
            let edit = labels
                .iter()
                .find(|(text, _)| text == "编辑文本")
                .unwrap()
                .1;
            assert_eq!(row, selected_row, "selection must not move the table");
            for title in ["本次文本", "来源表"] {
                let header = labels.iter().find(|(text, _)| text == title).unwrap().1;
                assert!(edit.bottom() < header.top(), "toolbar overlaps {title}");
                assert!(header.bottom() < row.top());
            }
            assert!(
                edit.bottom() <= row.top(),
                "toolbar overlaps the first result"
            );
            assert!(labels.iter().any(|(text, _)| text == "用此表添加词条"));
            app.selection.click(1, true, false);
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            assert!(!labels.iter().any(|(text, _)| text == "编辑文本"));
            assert!(labels.iter().any(|(text, _)| text == "替换选中文本"));
            assert_eq!(
                row,
                labels
                    .iter()
                    .find(|(text, _)| text == "result-000")
                    .unwrap()
                    .1
            );
            app.restore_context_status();
            assert!(app.status.contains("共找到 60"));
            let replace = labels
                .iter()
                .find(|(text, _)| text == "替换")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(replace),
                        egui::Event::PointerButton {
                            pos: replace,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            assert_eq!(app.tab, 0);
            assert!(!app.status.contains("共找到"));
            assert_eq!(count(&app.search, "total"), 60);
        }
    }

    #[test]
    fn log_copy_and_export_preserve_colored_selectable_messages_without_timestamps() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        app.show_logs = true;
        app.log("读取完成，错误数为 0".into(), None);
        app.log("跳过非文本表".into(), Some(crate::logging::Level::Warning));
        app.log(
            "读取失败\n详细原因".into(),
            Some(crate::logging::Level::Error),
        );
        let mut frame = 0;
        let mut draw = |app: &mut Desktop, events| {
            frame += 1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1080., 680.),
                    )),
                    time: Some(f64::from(frame) / 60.),
                    events,
                    ..Default::default()
                },
                |ui| app.dialogs(ui.ctx()),
            );
            output.textures_delta.clear();
            output
        };
        let mut output = draw(&mut app, vec![]);
        for _ in 0..3 {
            output = draw(&mut app, vec![]);
        }
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let warning = texts
            .iter()
            .find(|text| text.galley.text().contains("跳过非文本表"))
            .unwrap();
        let info = texts
            .iter()
            .find(|text| text.galley.text().contains("错误数为 0"))
            .unwrap();
        assert!(
            info.galley
                .job
                .sections
                .iter()
                .all(|section| section.format.color == TEXT)
        );
        let error = texts
            .iter()
            .find(|text| text.galley.text().contains("详细原因"))
            .unwrap();
        assert!(
            warning
                .galley
                .job
                .sections
                .iter()
                .any(|section| section.format.color == ORANGE)
        );
        assert!(
            error
                .galley
                .job
                .sections
                .iter()
                .any(|section| section.format.color == RED)
        );
        let copy = texts
            .iter()
            .find(|text| text.galley.text() == "复制全部")
            .unwrap();
        assert!(
            copy.pos.y >= error.pos.y + error.galley.size().y,
            "log actions must stay below the scrollable text"
        );
        let position = egui::Rect::from_min_size(copy.pos, copy.galley.size()).center();
        draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        let copied = draw(
            &mut app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        )
        .platform_output
        .commands
        .into_iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            copied,
            "读取完成，错误数为 0\n[WARN ] 跳过非文本表\n[ERROR] 读取失败\n[ERROR] 详细原因"
        );
        let data = std::env::temp_dir().join(format!(
            "bajptr-log-export-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = data.join("log.txt");
        app.export_logs(&path).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), copied);
        bajptr_project::storage::retire(&data).unwrap();
    }

    #[test]
    fn edited_path_waits_for_save_and_late_checks_cannot_replace_its_status() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let data = std::env::temp_dir().join(format!(
            "bajptr-path-save-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = Desktop::initial();
        app.backend = Some(Backend::start(&data, &data, ctx.clone()).unwrap());
        app.loaded = true;
        app.game = "old-game".into();
        app.state = Arc::new(json!({"game":"old-game","revision":0}));
        app.submit("check", json!({}), "正在检查…");
        app.game = "new-game".into();
        app.game_path_changed();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !app.pending.is_empty() {
            assert!(Instant::now() < deadline);
            app.events();
            std::thread::yield_now();
        }
        assert!(app.path_dirty && !app.connected);
        assert!(app.error.is_none());
        assert_eq!(app.status, "路径已更改，等待重新检查…");
        assert!(!data.join(".gui/settings.json").exists());
        let mut frame = 0;
        let mut draw = |app: &mut Desktop, events| {
            frame += 1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1080., 680.),
                    )),
                    time: Some(f64::from(frame) / 60.),
                    events,
                    ..Default::default()
                },
                |ui| app.tools_page(ui),
            );
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_string(),
                        egui::Rect::from_min_size(text.pos, text.galley.size()),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let mut labels = Vec::new();
        for _ in 0..3 {
            labels = draw(&mut app, vec![]);
        }
        let save = labels.iter().find(|(label, _)| label == "保存").unwrap().1;
        let path = labels
            .iter()
            .find(|(label, _)| label == "游戏目录")
            .unwrap()
            .1;
        let check = labels
            .iter()
            .find(|(label, _)| label == "检查底稿")
            .unwrap()
            .1;
        let baseline = labels
            .iter()
            .find(|(label, _)| label == "初始底稿")
            .unwrap()
            .1;
        assert!((save.center().y - path.center().y).abs() < 8.);
        assert!((check.center().y - baseline.center().y).abs() < 8.);
        assert!(check.top() > save.bottom());
        assert!(!data.exists());
        for pressed in [true, false] {
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(save.center()),
                    egui::Event::PointerButton {
                        pos: save.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(!app.path_dirty);
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.busy.is_some() {
            assert!(Instant::now() < deadline);
            app.events();
            std::thread::yield_now();
        }
        let settings: Value =
            serde_json::from_slice(&std::fs::read(data.join(".gui/settings.json")).unwrap())
                .unwrap();
        assert_eq!(settings["game"], "new-game");
        app.backend = None;
        bajptr_project::storage::retire(&data).unwrap();
    }

    #[test]
    fn stopped_backend_releases_pending_work_and_reconnects_without_replaying_writes() {
        for send_after_disconnect in [false, true] {
            let ctx = egui::Context::default();
            let data = std::env::temp_dir().join(format!(
                "bajptr-reconnect-test-{}-{send_after_disconnect}",
                std::process::id()
            ));
            let mut app = Desktop::initial();
            app.data_path = Some(data.clone());
            app.backend = Some(Backend::start(&data, &data, ctx.clone()).unwrap());
            app.loaded = true;
            app.connected = true;
            app.sources_ready = true;
            app.pending.insert(99, "apply".into());
            app.busy = Some(Busy {
                id: 99,
                cancellable: false,
            });
            app.obsolete.insert(98);
            app.search = Arc::new(json!({"rows":[{}]}));
            app.selection.rows.insert(0);
            app.preview = Some(Arc::new(json!({"nonce":"old","prepared":true})));
            app.scope_form = Some(ScopeForm::new("", vec![]));
            app.scope_form.as_mut().unwrap().request = Some(97);
            app.backend
                .as_mut()
                .unwrap()
                .send("quit", json!({}))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            if send_after_disconnect {
                while !app.backend.as_ref().unwrap().poll().1 {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                app.busy = None;
                app.submit("load", json!({}), "load");
            } else {
                while app.backend.is_some() {
                    assert!(Instant::now() < deadline);
                    app.events();
                    std::thread::yield_now();
                }
            }
            assert!(app.backend.is_none());
            assert!(app.busy.is_none());
            assert!(app.pending.is_empty() && app.obsolete.is_empty());
            assert!(!app.loaded && !app.connected && !app.sources_ready);
            assert!(app.preview.is_none() && app.search.is_null());
            assert!(app.selection.rows.is_empty());
            assert!(app.scope_form.as_ref().unwrap().request.is_none());
            assert!(app.error.as_ref().unwrap().contains("不会自动重试"));
            app.reconnect(&ctx);
            assert_eq!(app.pending.values().collect::<Vec<_>>(), vec!["load"]);
            assert!(app.scope_form.is_none());
            let deadline = Instant::now() + Duration::from_secs(3);
            while app.busy.is_some() {
                assert!(Instant::now() < deadline);
                app.events();
                std::thread::yield_now();
            }
            assert!(app.loaded && app.backend.is_some());
            assert!(app.error.is_none());
            assert!(app.pending.is_empty());
            assert!(slice(&app.state, "rules").is_empty());
        }
    }

    #[test]
    fn single_edit_manager_shows_one_full_comparison_and_preserves_filtered_selection() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let first = json!({"index":7,"active":true,"label":"技能说明","edit":{"table":"Skill","row_id":9606,"slot":2,"original":"独有原文\n保留换行","replacement":"独有修改"}});
        let second = json!({"index":9,"active":false,"label":"剧情","edit":{"table":"Scenario","row_id":9607,"slot":1,"original":"另一条原文","replacement":"另一条修改"}});
        let mut app = Desktop::initial();
        app.show_edits = true;
        app.state = json!({"edits":[first.clone()]}).into();
        app.edit_view.filter = "以前的筛选".into();
        let draw = |app: &mut Desktop| {
            let mut output = None;
            for _ in 0..3 {
                let mut frame_output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 620.),
                        )),
                        ..Default::default()
                    },
                    |ui| app.dialogs(ui.ctx()),
                );
                frame_output.textures_delta.clear();
                output = Some(frame_output);
            }
            output
                .unwrap()
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let text = draw(&mut app);
        assert_eq!(
            app.edit_view.selected,
            Some(7),
            "the only record should be ready for editing"
        );
        assert_eq!(
            text.iter()
                .filter(|v| v.as_str() == "独有原文\n保留换行")
                .count(),
            1
        );
        assert_eq!(text.iter().filter(|v| v.as_str() == "独有修改").count(), 1);
        app.state = json!({"edits":[first,second.clone()]}).into();
        app.edit_view.filter = "另一条修改".into();
        draw(&mut app);
        assert_eq!(
            app.edit_view.selected,
            Some(9),
            "filtering must not leave actions pointing at a hidden record"
        );
        app.state = json!({"edits":[second]}).into();
        draw(&mut app);
        assert_eq!(
            app.edit_view.selected,
            Some(9),
            "removing another record must preserve the real edit index"
        );
    }

    #[test]
    fn checking_baseline_keeps_directory_layout_until_the_result() {
        for connected in [false, true] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = Desktop::initial();
            let data = std::env::temp_dir().join(format!(
                "bajptr-header-check-test-{}-{connected}",
                std::process::id()
            ));
            app.backend = Some(Backend::start(&data, &data, ctx.clone()).unwrap());
            app.connected = connected;
            app.loaded = true;
            app.game = "connected-game".into();
            app.state = Arc::new(json!({"game":"connected-game","revision":0}));
            let mut frame = 0;
            let mut draw = |app: &mut Desktop, events| {
                frame += 1;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1080., 680.),
                        )),
                        time: Some(f64::from(frame) / 60.),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        app.header(ui);
                        if app.tab == 2 {
                            app.tools_page(ui);
                        }
                    },
                );
                output.textures_delta.clear();
                output
                    .shapes
                    .into_iter()
                    .filter_map(|shape| match shape.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_string(),
                            egui::Rect::from_min_size(text.pos, text.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let mut header = Vec::new();
            for _ in 0..3 {
                header = draw(&mut app, vec![]);
            }
            assert_eq!(
                header.iter().any(|(label, _)| label == "游戏目录"),
                !connected,
                "the main page must expose directory controls only when needed"
            );
            for label in ["工具", "检查底稿"] {
                for _ in 0..3 {
                    header = draw(&mut app, vec![]);
                }
                let position = header
                    .iter()
                    .find(|(text, _)| text == label)
                    .unwrap()
                    .1
                    .center();
                for pressed in [true, false] {
                    draw(
                        &mut app,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::default(),
                            },
                        ],
                    );
                }
                if label == "工具" {
                    assert_eq!(app.tab, 2);
                    assert!(
                        app.pending.is_empty(),
                        "opening tools must not start a check"
                    );
                }
            }
            assert!(app.pending.values().any(|action| action == "check"));
            assert_eq!(
                app.connected, connected,
                "starting a check must not report a lost connection"
            );
            let header = draw(&mut app, vec![]);
            assert_eq!(
                header
                    .iter()
                    .filter(|(label, _)| label == "游戏目录")
                    .count(),
                1,
                "checking must preserve the chosen directory layout"
            );
            // The worker has no loaded game, so this check must fail. Consume
            // the real error to verify that failure still exposes the editor.
            let deadline = Instant::now() + Duration::from_secs(2);
            while app.busy.is_some() && Instant::now() < deadline {
                app.events();
                std::thread::yield_now();
            }
            assert!(app.busy.is_none(), "the check must have completed");
            assert!(
                app.error
                    .as_ref()
                    .is_some_and(|message| message.starts_with("检查底稿失败："))
            );
            assert!(
                !app.connected,
                "a failed check must invalidate the connection"
            );
            app.tab = 0;
            let header = draw(&mut app, vec![]);
            assert!(header.iter().any(|(label, _)| label == "游戏目录"));
        }
    }

    #[test]
    fn ready_source_inventory_does_not_queue_a_blocking_sync() {
        let ctx = egui::Context::default();
        let mut app = Desktop::initial();
        let data = std::env::temp_dir().join(format!("bajptr-header-test-{}", std::process::id()));
        app.backend = Some(Backend::start(&data, &data, ctx).unwrap());
        app.connected = true;
        app.game = "connected-game".into();
        app.baseline = json!({"version_id":"current"});
        app.sources_ready = true;
        app.background_tables();
        assert!(
            app.pending.is_empty(),
            "an unchanged baseline must not queue another source sync"
        );
        assert!(
            !app.write_busy(),
            "rechecking an unchanged baseline must not disable header buttons"
        );
        assert!(app.sources_ready);
        app.set_baseline(json!({"version_id":"current","verified":true}));
        app.background_tables();
        assert!(
            app.pending.is_empty(),
            "refreshing the same baseline must reuse sources"
        );
        app.set_baseline(json!({"version_id":"updated"}));
        app.background_tables();
        assert!(
            app.pending.values().any(|action| action == "sources"),
            "a new baseline must still check for new source tables"
        );
        assert!(
            app.write_busy(),
            "source migration must retain its write protection"
        );
        assert!(!app.sources_ready);
    }

    #[test]
    fn preview_wheel_scrolls_only_the_hovered_pane() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        let original = (0..80)
            .map(|i| format!("detail-{i:03}\n"))
            .collect::<String>();
        let changes: Vec<_> = (0..80)
            .map(|i| json!({"kind":"新增","table":"test","original":original,"before":format!("row-{i:03}\n{original}"),"after":format!("result-{i:03}")}))
            .collect();
        app.preview = Some(Arc::new(
            json!({"total_changes":80,"prepared":true,"stats":{"added":80,"changes":changes}}),
        ));
        app.preview_row = Some(0);
        let mut frame = 0;
        let mut draw = |events: Vec<egui::Event>| {
            frame += 1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1080., 680.),
                    )),
                    time: Some(f64::from(frame) / 60.),
                    events,
                    ..Default::default()
                },
                |ui| app.dialogs(ui.ctx()),
            );
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|clipped| {
                    if let egui::Shape::Text(text) = clipped.shape {
                        Some((text.galley.text().to_string(), text.pos, clipped.clip_rect))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut baseline = Vec::new();
        for _ in 0..6 {
            baseline = draw(vec![]);
        }
        let table = baseline
            .iter()
            .find(|v| v.0.starts_with("row-000 "))
            .unwrap()
            .2;
        let detail = baseline
            .iter()
            .find(|v| v.0.starts_with("row-000\n"))
            .unwrap()
            .clone();
        let wheel = |position| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -120.),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::default(),
                },
            ]
        };
        draw(vec![egui::Event::PointerMoved(table.center())]);
        let mut after_table = draw(wheel(table.center()));
        for _ in 0..30 {
            after_table = draw(vec![]);
        }
        let visible_rows = |shapes: &[(String, egui::Pos2, egui::Rect)]| {
            shapes
                .iter()
                .filter(|v| v.0.starts_with("row-") && !v.0.contains('\n'))
                .map(|v| v.0.clone())
                .collect::<Vec<_>>()
        };
        assert_ne!(
            visible_rows(&baseline),
            visible_rows(&after_table),
            "wheel over the table must move the table"
        );
        let detail_after_table = after_table
            .iter()
            .find(|v| v.0.starts_with("row-000\n"))
            .unwrap();
        assert!(
            (detail_after_table.1.y - detail.1.y).abs() < 1.,
            "table scrolling moved the detail pane"
        );
        let rows_after_table = visible_rows(&after_table);
        let detail_pointer = egui::pos2(
            detail_after_table.1.x + 20.,
            detail_after_table.2.center().y,
        );
        draw(vec![egui::Event::PointerMoved(detail_pointer)]);
        let mut after_detail = draw(wheel(detail_pointer));
        for _ in 0..30 {
            after_detail = draw(vec![]);
        }
        assert_eq!(
            rows_after_table,
            visible_rows(&after_detail),
            "detail scrolling moved the table"
        );
        let detail_after = after_detail
            .iter()
            .find(|v| v.0.starts_with("row-000\n"))
            .unwrap();
        assert!(
            detail_after.1.y < detail_after_table.1.y - 20.,
            "wheel over the detail must move the detail"
        );
    }

    #[test]
    fn cancelling_source_lookup_releases_loading_state_and_keeps_scope() {
        let mut app = Desktop::initial();
        let mut form = ScopeForm::new("A", vec![]);
        form.old = "任务".into();
        form.request = Some(7);
        app.scope_form = Some(form);
        app.pending.insert(7, "scope_sources".into());
        app.busy = Some(Busy {
            id: 7,
            cancellable: true,
        });
        app.cancel();
        let form = app.scope_form.as_ref().unwrap();
        assert!(
            form.request.is_none(),
            "refresh must be available after cancellation"
        );
        assert_eq!(form.encode().unwrap(), "A");
        assert!(app.obsolete.contains(&7), "late results must be discarded");
    }

    #[test]
    fn search_actions_are_disabled_when_disconnected_or_loading_more() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = Desktop::initial();
        app.search = json!({"nonce":"page","keyword":"任务","total":1000,"rows":[{"text":"任务结果","label":"来源"}]}).into();
        app.selection.click(0, false, false);
        app.baseline_check = json!({"state":"blocked"});
        let mut frame = 0;
        let mut draw = |app: &mut Desktop, events| {
            frame += 1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 620.),
                    )),
                    time: Some(f64::from(frame) / 60.),
                    events,
                    ..Default::default()
                },
                |ui| app.search_page(ui),
            );
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        egui::Rect::from_min_size(text.pos, text.galley.size()),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let click = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        for loading in [false, true] {
            app.connected = loading;
            if loading {
                app.pending.insert(7, "search_more".into());
                app.busy = Some(Busy {
                    id: 7,
                    cancellable: true,
                });
            }
            let mut labels = Vec::new();
            for _ in 0..3 {
                labels = draw(&mut app, vec![]);
            }
            for label in [
                "加载更多",
                "编辑文本",
                "替换选中文本",
                "任务结果",
                "任务结果",
            ] {
                let pos = labels
                    .iter()
                    .find(|(text, _)| text == label)
                    .unwrap()
                    .1
                    .center();
                for pressed in [true, false] {
                    draw(&mut app, click(pos, pressed));
                }
            }
            assert!(app.error.is_none());
            assert!(app.text_form.is_none());
            assert!(app.batch_form.is_none());
            assert!(!app.obsolete.contains(&7));
            assert_eq!(app.busy.as_ref().map(|busy| busy.id), loading.then_some(7));
        }
    }

    #[test]
    fn typing_new_search_clears_old_results_selection_and_late_requests() {
        let mut app = Desktop::initial();
        app.keyword = "技能".into();
        app.connected = true;
        app.search =
            json!({"keyword":"任务","nonce":"old","total":1,"rows":[{"text":"任务"}]}).into();
        app.selection.click(0, false, false);
        app.pending.insert(7, "search_more".into());
        app.busy = Some(Busy {
            id: 7,
            cancellable: true,
        });
        app.search_input_changed();
        assert!(app.search.is_null());
        assert!(app.selection.rows.is_empty());
        assert!(app.obsolete.contains(&7));
        assert!(app.busy.is_none());
        app.apply_state(json!({"game":"","revision":0,"search":{"nonce":"old","keyword":"任务","total":1,"rows":[{}]}}));
        assert!(
            app.search.is_null(),
            "a later document state must not restore the cleared search"
        );
    }

    #[test]
    fn whole_row_preserves_ctrl_shift_and_double_click() {
        let ctx = egui::Context::default();
        let mut selection = Selection::default();
        let mut rects = [egui::Rect::NOTHING; 3];
        let mut draw = |mut events: Vec<egui::Event>, modifiers: egui::Modifiers, time: f64| {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let mut double = false;
            let mut output = ctx.run_ui(
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
            output.textures_delta.clear();
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
#[test]
fn backup_controls_and_restore_summary_remain_visible_at_minimum_size() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = Desktop::initial();
    app.loaded = true;
    app.state = json!({"errors":{"rules.tsv":"损坏"}}).into();
    let mut frame = 0;
    let mut draw = |app: &mut Desktop, dialog: bool| {
        frame += 1;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 620.),
                )),
                time: Some(f64::from(frame) / 60.),
                ..Default::default()
            },
            |ui| {
                if dialog {
                    app.dialogs(ui.ctx());
                } else {
                    app.tools_page(ui);
                }
            },
        );
        output.textures_delta.clear();
        output
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
            .collect::<Vec<_>>()
    };
    let mut labels = vec![];
    for _ in 0..3 {
        labels = draw(&mut app, false);
    }
    for label in ["导出备份", "恢复备份"] {
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == label).unwrap();
        assert!(clip.contains_rect(*rect));
    }
    assert!(
        !labels
            .iter()
            .any(|(text, _, _)| text == "导入词表" || text == "导出词表")
    );
    app.confirm_backup = Some(
        json!({"nonce":"test","rule_count":12,"edit_count":34,"game":"D:/日服游戏","skip_kana":false}),
    );
    for _ in 0..3 {
        labels = draw(&mut app, true);
    }
    for label in [
        "词条 12 条 · 指定文本修改 34 条",
        "游戏目录：D:/日服游戏",
        "跳过含日文假名的文本：未勾选",
        "取消",
    ] {
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == label).unwrap();
        assert!(
            clip.contains_rect(*rect) && rect.bottom() < 620.,
            "{label} is clipped"
        );
    }
    assert!(app.confirm_backup.is_some());
    assert!(app.pending.is_empty());
}
