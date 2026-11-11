use eframe::egui::{self, Color32, FontId, Stroke, pos2, vec2};

pub const BLUE: Color32 = Color32::from_rgb(31, 105, 181);
pub const TEXT: Color32 = Color32::from_rgb(37, 54, 77);
pub const MUTED: Color32 = Color32::from_rgb(96, 111, 132);
pub const RED: Color32 = Color32::from_rgb(174, 51, 64);
pub const ORANGE: Color32 = Color32::from_rgb(157, 83, 13);
pub const BG: Color32 = Color32::from_rgb(244, 247, 251);
pub const LINE: Color32 = Color32::from_rgb(226, 233, 242);
pub const SOFT_BLUE: Color32 = Color32::from_rgb(233, 243, 254);
pub const ROW_HEIGHT: f32 = 40.;
const DISABLED_FILL: Color32 = Color32::from_rgb(232, 237, 243);
const DISABLED_TEXT: Color32 = Color32::from_rgb(83, 100, 122);

pub fn install(ctx: &egui::Context) {
    let root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(std::path::PathBuf::from);
    let (fonts, _) = crate::fonts::definitions(root.as_deref());
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.extreme_bg_color = Color32::WHITE;
    style.visuals.text_edit_bg_color = Some(Color32::from_rgb(249, 251, 254));
    style.visuals.selection.bg_fill = Color32::from_rgb(213, 233, 254);
    style.visuals.selection.stroke = Stroke::new(1., BLUE);
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.window_highlight_topmost = false;
    style.visuals.window_stroke = Stroke::new(1., LINE);
    style.visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(30),
    };
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(22),
    };
    style.visuals.faint_bg_color = Color32::from_rgb(248, 250, 253);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.noninteractive,
    ] {
        widget.corner_radius = egui::CornerRadius::same(7);
        widget.expansion = 0.;
    }
    style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1., LINE);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, TEXT);
    style.visuals.widgets.hovered.bg_fill = SOFT_BLUE;
    style.visuals.widgets.hovered.weak_bg_fill = SOFT_BLUE;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1., Color32::from_rgb(165, 197, 231));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, BLUE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(219, 236, 254);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(219, 236, 254);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1., BLUE);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.5, BLUE);
    style.visuals.widgets.open.bg_fill = SOFT_BLUE;
    style.visuals.widgets.open.weak_bg_fill = Color32::from_rgb(247, 250, 254);
    style.visuals.widgets.open.bg_stroke = Stroke::new(1., LINE);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.5, BLUE);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(7);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., LINE);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1., MUTED);
    style.spacing.item_spacing = vec2(8., 10.);
    style.spacing.button_padding = vec2(12., 8.);
    style.spacing.interact_size.y = 36.;
    style.spacing.icon_width = 14.;
    style.spacing.icon_width_inner = 8.;
    style.spacing.combo_height = 240.;
    style.spacing.window_margin = egui::Margin::same(18);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style.spacing.scroll.foreground_color = true;
    style.spacing.scroll.bar_width = 12.;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(16.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.5));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(20.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.5));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(14.));
    ctx.set_theme(egui::ThemePreference::Light);
    ctx.set_style_of(egui::Theme::Light, style);
}

pub fn tool_section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1., LINE))
        .corner_radius(10)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(17.).strong().color(BLUE));
            ui.add_space(4.);
            content(ui);
        });
}

#[derive(Clone, Copy)]
pub enum Icon {
    Add,
    Edit,
    Remove,
    Undo,
    Redo,
    Import,
    Export,
    Text,
    Preview,
    Apply,
    Folder,
    Refresh,
    Search,
    Info,
    Close,
    Replace,
    Restore,
    Tools,
}
impl Icon {
    fn for_label(label: &str) -> Option<Self> {
        Some(match label {
            "添加词条" | "用此表添加词条" => Self::Add,
            "编辑" | "编辑文本" => Self::Edit,
            "移除" | "移除单条修改" => Self::Remove,
            "撤销" => Self::Undo,
            "重做" => Self::Redo,
            "导入词表" => Self::Import,
            "导出词表" | "导出底稿文本" | "导出日志" => Self::Export,
            "预览替换" | "预览选中" | "预览并应用" => Self::Preview,
            "应用到游戏" | "保存词条" | "保存修改" | "填入词条" | "确定" => {
                Self::Apply
            }
            "选择文件夹" | "打开输出目录" | "打开底稿位置" => Self::Folder,
            "检查底稿" | "刷新" | "查看最新版本" => Self::Refresh,
            "查找" => Self::Search,
            "查看日志" | "来源表详情" | "关于 BA-JPTR" | "使用说明" => Self::Info,
            "取消任务" => Self::Close,
            "替换选中文本" | "替换" => Self::Replace,
            "恢复底稿" | "恢复初始底稿" | "恢复" => Self::Restore,
            "工具" => Self::Tools,
            _ if label.starts_with("单条修改") => Self::Text,
            _ => return None,
        })
    }
}

pub fn paint_icon(painter: &egui::Painter, origin: egui::Pos2, icon: Icon, color: Color32) {
    let p = |x, y| origin + vec2(x, y);
    let stroke = Stroke::new(1.6, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|v| p(v.0, v.1)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Tools => {
            line((2., 4.), (14., 4.));
            line((2., 12.), (14., 12.));
            painter.circle_filled(p(6., 4.), 2.5, color);
            painter.circle_filled(p(10., 12.), 2.5, color);
        }
        Icon::Add => {
            line((8., 2.), (8., 14.));
            line((2., 8.), (14., 8.));
        }
        Icon::Close => {
            line((3., 3.), (13., 13.));
            line((3., 13.), (13., 3.));
        }
        Icon::Apply => path(&[(2., 8.), (6., 12.), (14., 4.)]),
        Icon::Edit => {
            path(&[
                (3., 10.),
                (11., 2.),
                (14., 5.),
                (6., 13.),
                (2., 14.),
                (3., 10.),
            ]);
            line((9., 4.), (12., 7.));
        }
        Icon::Remove => {
            path(&[(4., 5.), (5., 14.), (12., 14.), (13., 5.)]);
            line((2., 4.), (14., 4.));
            line((6., 2.), (10., 2.));
            line((7., 7.), (7., 11.));
            line((10., 7.), (10., 11.));
        }
        Icon::Undo | Icon::Restore => {
            path(&[(6., 3.), (2., 7.), (6., 10.)]);
            path(&[(3., 7.), (10., 7.), (13., 9.), (13., 13.)]);
        }
        Icon::Redo => {
            path(&[(10., 3.), (14., 7.), (10., 10.)]);
            path(&[(13., 7.), (6., 7.), (3., 9.), (3., 13.)]);
        }
        Icon::Import | Icon::Export => {
            path(&[(2., 10.), (2., 14.), (14., 14.), (14., 10.)]);
            if matches!(icon, Icon::Import) {
                line((8., 1.), (8., 10.));
                path(&[(4., 6.), (8., 10.), (12., 6.)]);
            } else {
                line((8., 10.), (8., 1.));
                path(&[(4., 5.), (8., 1.), (12., 5.)]);
            }
        }
        Icon::Folder => path(&[
            (1., 4.),
            (6., 4.),
            (8., 6.),
            (15., 6.),
            (14., 13.),
            (2., 13.),
            (1., 4.),
        ]),
        Icon::Refresh => {
            path(&[
                (13., 6.),
                (11., 3.),
                (6., 2.),
                (2., 5.),
                (2., 10.),
                (6., 14.),
                (11., 13.),
                (14., 10.),
            ]);
            path(&[(13., 1.), (13., 6.), (8., 6.)]);
        }
        Icon::Search => {
            painter.circle_stroke(p(6.5, 6.5), 4.8, stroke);
            line((10., 10.), (14., 14.));
        }
        Icon::Preview => {
            path(&[
                (1., 8.),
                (5., 4.),
                (11., 4.),
                (15., 8.),
                (11., 12.),
                (5., 12.),
                (1., 8.),
            ]);
            painter.circle_stroke(p(8., 8.), 2., stroke);
        }
        Icon::Text => {
            path(&[
                (3., 1.),
                (10., 1.),
                (14., 5.),
                (14., 15.),
                (3., 15.),
                (3., 1.),
            ]);
            path(&[(10., 1.), (10., 5.), (14., 5.)]);
            line((6., 8.), (11., 8.));
            line((6., 11.), (11., 11.));
        }
        Icon::Info => {
            painter.circle_stroke(p(8., 8.), 6.5, stroke);
            line((8., 7.), (8., 11.));
            painter.circle_filled(p(8., 4.5), 0.9, color);
        }
        Icon::Replace => {
            line((2., 4.), (14., 4.));
            path(&[(11., 1.), (14., 4.), (11., 7.)]);
            line((14., 12.), (2., 12.));
            path(&[(5., 9.), (2., 12.), (5., 15.)]);
        }
    }
}

#[derive(Clone, Copy)]
pub enum ButtonKind {
    Standard,
    Primary,
    Soft,
    Quiet,
}
pub fn action_width(ui: &egui::Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(label.into(), FontId::proportional(14.5), TEXT)
        .size()
        .x
        + 24.
        + if Icon::for_label(label).is_some() {
            24.
        } else {
            0.
        }
}
pub fn action(ui: &mut egui::Ui, enabled: bool, label: &str, kind: ButtonKind) -> egui::Response {
    let icon = Icon::for_label(label);
    let active = enabled && ui.is_enabled();
    let color = if active {
        match kind {
            ButtonKind::Primary => Color32::WHITE,
            ButtonKind::Soft => BLUE,
            _ => TEXT,
        }
    } else {
        DISABLED_TEXT
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(14.5), color);
    let width = galley.size().x + 24. + if icon.is_some() { 24. } else { 0. };
    let mut button = egui::Button::new("").min_size(vec2(width, 36.));
    if active {
        match kind {
            ButtonKind::Primary => button = button.fill(BLUE).stroke(Stroke::NONE),
            ButtonKind::Soft => button = button.fill(SOFT_BLUE).stroke(Stroke::NONE),
            ButtonKind::Quiet => button = button.frame(false),
            _ => {}
        }
    } else {
        button = button.frame(false);
    }
    let response = ui.add_enabled(enabled, button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, active, label));
    let mut painter = ui.painter().clone();
    if !active {
        // The explicit disabled palette already expresses the state. Applying
        // ancestor disabled alpha again would wash out its text and icons.
        painter.set_opacity(1.);
        if !matches!(kind, ButtonKind::Quiet) {
            painter.rect(
                response.rect,
                7,
                DISABLED_FILL,
                Stroke::new(1., LINE),
                egui::StrokeKind::Inside,
            );
        }
    }
    if let Some(icon) = icon {
        paint_icon(
            &painter,
            pos2(response.rect.left() + 12., response.rect.center().y - 8.),
            icon,
            color,
        );
    }
    painter.galley(
        pos2(
            response.rect.left() + 12. + if icon.is_some() { 24. } else { 0. },
            response.rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}
pub fn secondary(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    action(ui, enabled, label, ButtonKind::Standard)
}
pub fn primary(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    action(ui, enabled, label, ButtonKind::Primary)
}

pub struct ToolButton {
    label: String,
}
impl ToolButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}
impl egui::Widget for ToolButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let kind = if self.label == "添加词条" {
            ButtonKind::Soft
        } else {
            ButtonKind::Standard
        };
        action(ui, true, &self.label, kind)
    }
}

pub fn app_icon() -> egui::IconData {
    egui::IconData {
        rgba: bajptr_brand::rgba(64),
        width: 64,
        height: 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composite(color: Color32, background: [f32; 3]) -> [f32; 3] {
        let rgba = color.to_srgba_unmultiplied();
        let alpha = f32::from(rgba[3]) / 255.;
        std::array::from_fn(|i| f32::from(rgba[i]) / 255. * alpha + background[i] * (1. - alpha))
    }
    fn luminance(color: [f32; 3]) -> f32 {
        color
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(v, w)| {
                w * if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            })
            .sum()
    }
    #[test]
    fn disabled_primary_is_readable_and_cannot_activate_directly_or_through_parent() {
        for inherited in [false, true] {
            let ctx = egui::Context::default();
            install(&ctx);
            let draw = |events| {
                let mut rect = egui::Rect::NOTHING;
                let mut enabled = true;
                let mut clicked = false;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui
                            .add_enabled_ui(!inherited, |ui| primary(ui, inherited, "保存修改"))
                            .inner;
                        rect = response.rect;
                        enabled = response.enabled();
                        clicked = response.clicked();
                    },
                );
                output.textures_delta.clear();
                (output, rect, enabled, clicked)
            };
            let (output, rect, _, _) = draw(vec![]);
            let text_color = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "保存修改" => {
                        text.override_text_color.or_else(|| {
                            text.galley
                                .rows
                                .first()?
                                .visuals
                                .mesh
                                .vertices
                                .first()
                                .map(|v| v.color)
                        })
                    }
                    _ => None,
                })
                .expect("rendered label");
            let fill = output
                .shapes
                .iter()
                .rev()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.fill),
                    _ => None,
                })
                .expect("rendered button background");
            let bg = composite(fill, [1.; 3]);
            let fg = composite(text_color, bg);
            let ratio = (luminance(bg) + 0.05) / (luminance(fg) + 0.05);
            assert!(ratio >= 4.5, "inherited={inherited}: contrast {ratio}");
            let pos = rect.center();
            let events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let (_, _, enabled, clicked) = draw(events);
            assert!(
                !enabled && !clicked,
                "disabled painting must not enable interaction"
            );
        }
    }
}
