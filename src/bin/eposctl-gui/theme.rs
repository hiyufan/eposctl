//! Palette, fonts and widget styling. Dark, close to the official suite:
//! charcoal surfaces with the EPOS teal as the only accent.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x17, 0x18, 0x1E);
pub const PANEL: Color32 = Color32::from_rgb(0x1F, 0x20, 0x27);
pub const RAISED: Color32 = Color32::from_rgb(0x2A, 0x2C, 0x34);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x3A, 0x3D, 0x45);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
pub const SECONDARY: Color32 = Color32::from_rgb(0x8E, 0x90, 0x99);
pub const FAINT: Color32 = Color32::from_rgb(0x5C, 0x5F, 0x68);
pub const ACCENT: Color32 = Color32::from_rgb(0x44, 0xEA, 0xD6);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1E, 0x4A, 0x47);
pub const WARN: Color32 = Color32::from_rgb(0xF2, 0xA9, 0x3B);
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);

pub const R_SMALL: u8 = 6;
pub const R_CARD: u8 = 12;

/// Semibold face for headings and numbers.
pub fn strong() -> FontFamily {
    FontFamily::Name("strong".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_strong(size: f32) -> FontId {
    FontId::new(size, strong())
}

fn load(defs: &mut FontDefinitions, key: &str, file: &str, index: u32) -> bool {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    match std::fs::read(dir.join("Fonts").join(file)) {
        Ok(bytes) => {
            let mut fd = FontData::from_owned(bytes);
            fd.index = index;
            defs.font_data.insert(key.to_owned(), Arc::new(fd));
            true
        }
        Err(_) => false,
    }
}

/// Use the system's Segoe UI for Latin and Microsoft YaHei for CJK. Nothing is
/// bundled; missing files fall back to egui's built-in fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut regular = Vec::new();
    let mut bold = Vec::new();
    for (key, file, is_bold) in [
        ("segoe", "segoeui.ttf", false),
        ("yahei", "msyh.ttc", false),
        ("segoe-sb", "seguisb.ttf", true),
        ("yahei-b", "msyhbd.ttc", true),
    ] {
        if load(&mut defs, key, file, 0) {
            if is_bold { &mut bold } else { &mut regular }.push(key.to_owned());
        }
    }
    let builtin = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = regular.clone();
    prop.extend(builtin.iter().cloned());
    let mut strong_list = bold;
    strong_list.extend(regular);
    strong_list.extend(builtin);
    let mut families = BTreeMap::new();
    families.insert(FontFamily::Proportional, prop);
    families.insert(strong(), strong_list);
    if let Some(mono) = defs.families.get(&FontFamily::Monospace) {
        families.insert(FontFamily::Monospace, mono.clone());
    }
    defs.families = families;
    ctx.set_fonts(defs);
}

pub fn install_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.window_fill = PANEL;
    v.panel_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = PANEL;
    v.override_text_color = None;
    v.window_corner_radius = CornerRadius::same(R_CARD);
    v.window_stroke = Stroke::new(1.0, HAIRLINE);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let w = &mut v.widgets;
    let r = CornerRadius::same(R_SMALL);
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, SECONDARY);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = RAISED;
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = Color32::from_rgb(0x34, 0x37, 0x40);
    w.hovered.weak_bg_fill = Color32::from_rgb(0x34, 0x37, 0x40);
    w.hovered.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, TEXT);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open = w.active;

    // Always dark: with the default "follow the system" preference, a light
    // Windows theme switched egui's stock widgets (combo boxes, drag values,
    // text fields, menus) to light visuals inside the dark panels.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, font(12.0)),
            (TextStyle::Body, font(14.0)),
            (TextStyle::Button, font(14.0)),
            (TextStyle::Heading, font_strong(20.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.spacing.slider_rail_height = 4.0;
        s.spacing.interact_size.y = 30.0;
        s.interaction.selectable_labels = false;
    });
}
