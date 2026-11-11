use eframe::egui;
use std::path::Path;

const FALLBACK: &[u8] = include_bytes!("../assets/NotoSansSC-Regular.otf");
pub fn definitions(windows_dir: Option<&Path>) -> (egui::FontDefinitions, bool) {
    let mut fonts = egui::FontDefinitions::default();
    let system = windows_dir.and_then(|root| {
        [("msyh.ttc", 1), ("simhei.ttf", 0)]
            .into_iter()
            .find_map(|(name, index)| {
                std::fs::read(root.join("Fonts").join(name))
                    .ok()
                    .map(|bytes| {
                        let mut font = egui::FontData::from_owned(bytes);
                        font.index = index;
                        font
                    })
            })
    });
    let fallback = system.is_none();
    let data = system.unwrap_or_else(|| egui::FontData::from_static(FALLBACK));
    fonts
        .font_data
        .insert("interface".into(), std::sync::Arc::new(data));
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "interface".into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("interface".into());
    (fonts, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_system_cjk_fonts_still_render_chinese_labels() {
        let (fonts, fallback) = definitions(None);
        assert!(fallback);
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.fonts_mut(|fonts| {
                let font = egui::FontId::proportional(16.);
                for ch in "替换查找恢复词表预览应用游戏目录请输入中文".chars()
                {
                    assert!(fonts.has_glyph(&font, ch), "missing Chinese glyph {ch}");
                }
            });
        });
        // Headless tests inspect UI output without uploading textures to a renderer.
        output.textures_delta.clear();
    }
}
