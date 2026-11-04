//! Small custom widgets in the suite's style: pill chips, switches, sliders, panels.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Key, Margin, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::theme::{self, ACCENT, ACCENT_DIM, BG, DANGER, FAINT, HAIRLINE, PANEL, RAISED, SECONDARY, TEXT, WARN};

/// The surface used for grouped controls.
pub fn panel() -> Frame {
    Frame::new().fill(PANEL).corner_radius(CornerRadius::same(theme::R_CARD)).inner_margin(Margin::same(20))
}

/// Preset pill. `selected` = currently applied.
pub fn chip(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::font(14.0), TEXT);
    let size = vec2(galley.size().x + 32.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let (fill, stroke, fg) = if selected {
        (ACCENT_DIM, Stroke::new(1.0, ACCENT), TEXT)
    } else if hovered {
        (Color32::from_rgb(0x34, 0x37, 0x40), Stroke::NONE, TEXT)
    } else {
        (RAISED, Stroke::NONE, SECONDARY)
    };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(17), fill, stroke, StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, fg);
    focus_ring(ui, &resp, rect, 17);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Outlined pill for a secondary action ("Save as preset", "Reset").
pub fn ghost(ui: &mut Ui, text: &str) -> Response {
    ghost_toggle(ui, text, true, false)
}

/// `ghost` with an enabled state and an "on" state (accent outline).
pub fn ghost_toggle(ui: &mut Ui, text: &str, enabled: bool, on: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::font(13.0), TEXT);
    let size = vec2(galley.size().x + 24.0, 30.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let hot = enabled && resp.hovered();
    let fg = if !enabled {
        FAINT
    } else if on {
        ACCENT
    } else if hot {
        TEXT
    } else {
        SECONDARY
    };
    let stroke = if on {
        ACCENT
    } else if hot {
        SECONDARY
    } else {
        HAIRLINE
    };
    ui.painter().rect(rect, CornerRadius::same(15), Color32::TRANSPARENT, Stroke::new(1.0, stroke), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    focus_ring(ui, &resp, rect, 15);
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Filled accent button for the primary action in a small form.
pub fn primary(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::font_strong(13.0), BG);
    let size = vec2(galley.size().x + 28.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() { Color32::from_rgb(0x6C, 0xF0, 0xE0) } else { ACCENT };
    ui.painter().rect_filled(rect, CornerRadius::same(15), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, BG);
    focus_ring(ui, &resp, rect, 15);
    resp
}

fn focus_ring(ui: &Ui, resp: &Response, rect: Rect, r: u8) {
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            CornerRadius::same(r + 3),
            Stroke::new(1.5, ACCENT.gamma_multiply(0.7)),
            StrokeKind::Outside,
        );
    }
}

/// On/off switch. Returns a response that is `changed()` when toggled.
pub fn switch(ui: &mut Ui, on: &mut bool, enabled: bool) -> Response {
    let size = vec2(42.0, 24.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(size, sense);
    if enabled && resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let track = if *on { ACCENT } else { RAISED };
    let track = if enabled { track } else { track.gamma_multiply(0.4) };
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(12),
        track,
        Stroke::new(1.0, if *on { ACCENT } else { HAIRLINE }),
        StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), t);
    let knob = if *on { BG } else { SECONDARY };
    p.circle_filled(pos2(x, rect.center().y), 8.0, if enabled { knob } else { FAINT });
    focus_ring(ui, &resp, rect, 12);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Horizontal slider over 0..=1 with an accent fill. Arrow keys step 1%.
pub fn slider(ui: &mut Ui, value: &mut f32, enabled: bool) -> Response {
    let h = 26.0;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), sense);
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 16.0, 4.0));
    if enabled {
        if let Some(p) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                let v = egui::remap_clamp(p.x, rail.left()..=rail.right(), 0.0..=1.0);
                let v = (v * 100.0).round() / 100.0;
                if v != *value {
                    *value = v;
                    resp.mark_changed();
                }
            }
            resp.request_focus();
        }
        if resp.has_focus() {
            let (l, r) = ui.input(|i| (i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight)));
            let d = if r {
                0.01
            } else if l {
                -0.01
            } else {
                0.0
            };
            if d != 0.0 {
                *value = ((*value + d) * 100.0).round().clamp(0.0, 100.0) / 100.0;
                resp.mark_changed();
            }
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(resp.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() })
            });
        }
    }
    let p = ui.painter();
    let x = egui::lerp(rail.left()..=rail.right(), value.clamp(0.0, 1.0));
    let (fill, knob) = if enabled { (ACCENT, TEXT) } else { (FAINT, FAINT) };
    p.rect_filled(rail, CornerRadius::same(2), RAISED);
    p.rect_filled(Rect::from_min_max(rail.min, pos2(x, rail.max.y)), CornerRadius::same(2), fill);
    let active = enabled && (resp.hovered() || resp.dragged() || resp.has_focus());
    p.circle_filled(pos2(x, rail.center().y), if active { 9.0 } else { 8.0 }, knob);
    if resp.has_focus() {
        p.circle_stroke(pos2(x, rail.center().y), 12.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.6)));
    }
    resp
}

/// Title on the left, optional right-aligned value, used above a slider.
pub fn row_title(ui: &mut Ui, title: &str, value: Option<String>, enabled: bool) {
    ui.horizontal(|ui| {
        let c = if enabled { TEXT } else { SECONDARY };
        ui.label(egui::RichText::new(title).font(theme::font_strong(15.0)).color(c));
        if let Some(v) = value {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(v).font(theme::font_strong(15.0)).color(if enabled {
                    ACCENT
                } else {
                    FAINT
                }));
            });
        }
    });
}

pub fn desc(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::font(12.5)).color(SECONDARY));
}

/// Battery glyph with level fill; colour warns when low.
pub fn battery(ui: &mut Ui, level: u8) {
    let (rect, _) = ui.allocate_exact_size(vec2(28.0, 14.0), Sense::hover());
    let body = Rect::from_min_size(rect.min, vec2(25.0, 14.0));
    let p = ui.painter();
    p.rect_stroke(body, CornerRadius::same(3), Stroke::new(1.2, SECONDARY), StrokeKind::Inside);
    p.rect_filled(
        Rect::from_min_size(pos2(body.right() + 1.0, body.center().y - 3.0), vec2(2.0, 6.0)),
        CornerRadius::same(1),
        SECONDARY,
    );
    let color = match level {
        0..=10 => DANGER,
        11..=20 => WARN,
        _ => ACCENT,
    };
    let inner = body.shrink(2.5);
    let w = inner.width() * level.min(100) as f32 / 100.0;
    p.rect_filled(Rect::from_min_size(inner.min, vec2(w.max(1.0), inner.height())), CornerRadius::same(1), color);
}

/// Status dot.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Thin divider spanning the available width.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, HAIRLINE));
}

/// Label/value line for the device page.
pub fn info_row(ui: &mut Ui, label: &str, value: &str, value_color: Color32) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(egui::RichText::new(label).color(SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).color(value_color));
        });
    });
}

/// Small rotating arc used while waiting.
pub fn spinner(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let c = rect.center();
    let pts: Vec<_> = (0..=16)
        .map(|i| {
            let a = t * 4.0 + i as f32 / 16.0 * 4.0;
            c + vec2(a.cos(), a.sin()) * 6.0
        })
        .collect();
    ui.painter().line(pts, Stroke::new(1.8, ACCENT));
    ui.ctx().request_repaint();
}

/// Left-aligned section heading inside the page.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::font_strong(15.0)).color(TEXT));
}

/// Height of `level_meter`, so a neighbouring panel can reserve the same space.
pub const LEVEL_ROW_H: f32 = 20.0;

/// Live input level under a slider, on one row: a bar aligned with the slider
/// rail, a peak tick, and the value in dB. `floor..=0` dB spans the bar.
pub fn level_meter(ui: &mut Ui, label: &str, level: f32, peak: f32, floor: f32) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), LEVEL_ROW_H), Sense::hover());
    let value = if level > floor { format!("{level:.0} dB") } else { format!("< {floor:.0} dB") };
    let galley = ui.painter().layout_no_wrap(value, theme::font(12.0), FAINT);
    let text_w = 56.0;
    // Same left inset as the slider rail so the two line up.
    let bar = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.center().y - 3.0),
        pos2(rect.right() - 8.0 - text_w, rect.center().y + 3.0),
    );
    let x = |db: f32| egui::lerp(bar.left()..=bar.right(), ((db - floor) / -floor).clamp(0.0, 1.0));
    let p = ui.painter();
    p.rect_filled(bar, CornerRadius::same(3), RAISED);
    if level > floor {
        p.rect_filled(
            Rect::from_min_max(bar.min, pos2(x(level), bar.max.y)),
            CornerRadius::same(3),
            TEXT.gamma_multiply(0.55),
        );
    }
    if peak > floor {
        let px = x(peak);
        p.line_segment([pos2(px, bar.top() - 2.0), pos2(px, bar.bottom() + 2.0)], Stroke::new(2.0, TEXT));
    }
    p.galley(pos2(rect.right() - 8.0 - galley.size().x, rect.center().y - galley.size().y / 2.0), galley, FAINT);
    resp.on_hover_text(label);
}
