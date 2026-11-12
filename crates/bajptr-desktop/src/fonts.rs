use eframe::egui;
use std::path::Path;

pub fn definitions(windows_dir: Option<&Path>) -> Option<egui::FontDefinitions> {
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
    })?;
    fonts
        .font_data
        .insert("interface".into(), std::sync::Arc::new(system));
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
    Some(fonts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_fonts_render_chinese_labels_and_missing_fonts_are_reported() {
        assert!(definitions(None).is_none());
        let root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("WINDIR"))
            .map(std::path::PathBuf::from);
        let fonts = definitions(root.as_deref()).expect("Windows Chinese font required");
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
