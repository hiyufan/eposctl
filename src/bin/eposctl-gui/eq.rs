//! The 9-band EQ curve editor.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Key, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use eposctl::eqfile::{CENTRES_HZ, band_freq, band_label};

use crate::theme::{self, ACCENT, BG, FAINT, HAIRLINE, SECONDARY, TEXT};

pub const MIN_DB: f32 = -6.0;
pub const MAX_DB: f32 = 6.0;

fn catmull_rom(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let t2 = t * t;
    let t3 = t2 * t;
    let f = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    pos2(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y))
}

/// Range of the optional live level bars drawn behind the curve.
pub const METER_FLOOR_DB: f32 = -60.0;

/// Draws the editor and returns true if `gains` changed this frame.
/// `levels` (dB, `METER_FLOOR_DB..=0`, one per band) are drawn as bars behind the
/// curve on their own scale: the full plot height spans the meter range.
pub fn editor(
    ui: &mut Ui,
    id_salt: &str,
    gains: &mut [f32; CENTRES_HZ.len()],
    height: f32,
    levels: Option<&[f32]>,
) -> bool {
    let n = gains.len();
    let id = ui.scope_id().with(id_salt);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let painter = ui.painter_at(rect.expand(2.0));

    // Plot area inside the widget: room for dB labels on the left and band labels below.
    let plot = Rect::from_min_max(rect.min + vec2(40.0, 14.0), rect.max - vec2(14.0, 34.0));
    let x_of = |i: usize| plot.left() + plot.width() * i as f32 / (n - 1) as f32;
    let y_of = |db: f32| egui::remap(db, MIN_DB..=MAX_DB, plot.bottom()..=plot.top());
    let db_of = |y: f32| egui::remap_clamp(y, plot.bottom()..=plot.top(), MIN_DB..=MAX_DB);
    let nearest =
        |x: f32| ((x - plot.left()) / plot.width() * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;

    // Grid.
    let label_font = theme::font(11.0);
    for db in [-6.0f32, -3.0, 0.0, 3.0, 6.0] {
        let y = y_of(db);
        let (w, c) = if db == 0.0 { (1.0, HAIRLINE) } else { (1.0, Color32::from_rgb(0x26, 0x28, 0x2F)) };
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(w, c));
        let txt = if db > 0.0 { format!("+{db:.0}") } else { format!("{db:.0}") };
        painter.text(pos2(plot.left() - 10.0, y), Align2::RIGHT_CENTER, txt, label_font.clone(), FAINT);
    }
    for i in 0..n {
        let x = x_of(i);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, Color32::from_rgb(0x22, 0x24, 0x2A)),
        );
    }
    for i in 0..n {
        painter.text(
            pos2(x_of(i), rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            band_label(i),
            label_font.clone(),
            SECONDARY,
        );
    }
    painter.text(pos2(plot.left() - 10.0, rect.bottom() - 4.0), Align2::RIGHT_BOTTOM, "Hz", label_font.clone(), FAINT);

    // Interaction state kept across frames.
    let sel_id = id.with("sel");
    let drag_id = id.with("drag");
    let mut selected: Option<usize> = ui.data(|d| d.get_temp(sel_id));
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let mut changed = false;

    let pointer = resp.hover_pos();
    let hovered = pointer.filter(|p| plot.expand2(vec2(12.0, 12.0)).contains(*p)).map(|p| nearest(p.x));

    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        dragging = Some(nearest(p.x));
        selected = dragging;
        resp.request_focus();
    }
    if let (Some(i), Some(p)) = (dragging, resp.interact_pointer_pos())
        && resp.dragged()
    {
        let v = (db_of(p.y) * 10.0).round() / 10.0;
        if v != gains[i] {
            gains[i] = v;
            changed = true;
        }
    }
    if resp.drag_stopped() {
        dragging = None;
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let i = nearest(p.x);
        selected = Some(i);
        resp.request_focus();
        // A click on the column (not on the node) moves the node there.
        let v = (db_of(p.y) * 10.0).round() / 10.0;
        if (y_of(gains[i]) - p.y).abs() > 10.0 && v != gains[i] {
            gains[i] = v;
            changed = true;
        }
    }
    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let i = nearest(p.x);
        if gains[i] != 0.0 {
            gains[i] = 0.0;
            changed = true;
        }
    }
    if resp.has_focus() {
        let sel = selected.get_or_insert(n / 2);
        let (up, down, left, right, big) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.modifiers.shift,
            )
        });
        let step = if big { 1.0 } else { 0.5 };
        if left && *sel > 0 {
            *sel -= 1;
        }
        if right && *sel + 1 < n {
            *sel += 1;
        }
        let delta = if up {
            step
        } else if down {
            -step
        } else {
            0.0
        };
        if delta != 0.0 {
            let v = ((gains[*sel] + delta) * 2.0).round() / 2.0;
            gains[*sel] = v.clamp(MIN_DB, MAX_DB);
            changed = true;
        }
        // Keep arrow keys from moving focus to another widget.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
            )
        });
    }
    ui.data_mut(|d| {
        d.insert_temp(sel_id, selected);
        d.insert_temp(drag_id, dragging);
    });

    // Live level bars, under everything else that moves.
    if let Some(levels) = levels {
        let w = (plot.width() / (n - 1) as f32 * 0.42).min(30.0);
        for (i, &db) in levels.iter().enumerate().take(n) {
            let t = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
            if t <= 0.0 {
                continue;
            }
            let x = x_of(i);
            let top = plot.bottom() - plot.height() * t;
            let bar = Rect::from_min_max(pos2(x - w / 2.0, top), pos2(x + w / 2.0, plot.bottom()));
            painter.rect_filled(bar, CornerRadius::same(3), SECONDARY.gamma_multiply(0.16));
        }
    }

    // Curve and fill.
    let pts: Vec<Pos2> = (0..n).map(|i| pos2(x_of(i), y_of(gains[i]))).collect();
    let mut curve = Vec::with_capacity((n - 1) * 16 + 1);
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p3 = pts[(i + 2).min(n - 1)];
        for s in 0..16 {
            let mut p = catmull_rom(p0, pts[i], pts[i + 1], p3, s as f32 / 16.0);
            p.y = p.y.clamp(plot.top(), plot.bottom());
            curve.push(p);
        }
    }
    curve.push(pts[n - 1]);

    let zero = y_of(0.0);
    let fill = ACCENT.gamma_multiply(0.14);
    let mut mesh = Mesh::default();
    for w in curve.windows(2) {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(w[0], fill);
        mesh.colored_vertex(w[1], fill);
        mesh.colored_vertex(pos2(w[1].x, zero), fill);
        mesh.colored_vertex(pos2(w[0].x, zero), fill);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(mesh);
    painter.line(curve, Stroke::new(2.5, ACCENT));

    // Nodes.
    let focused = resp.has_focus();
    for (i, &p) in pts.iter().enumerate() {
        let active = dragging == Some(i) || (dragging.is_none() && hovered == Some(i));
        let is_sel = focused && selected == Some(i);
        if active || is_sel {
            painter.line_segment(
                [pos2(p.x, plot.top()), pos2(p.x, plot.bottom())],
                Stroke::new(1.0, ACCENT.gamma_multiply(0.25)),
            );
        }
        let r = if active { 7.5 } else { 6.0 };
        if active {
            painter.circle_filled(p, r, ACCENT);
        } else {
            painter.circle_filled(p, r, BG);
            painter.circle_stroke(p, r, Stroke::new(2.0, ACCENT));
        }
        if is_sel {
            painter.circle_stroke(p, r + 4.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.6)));
        }
    }

    // dB readout over the active node.
    let show = dragging.or(hovered).or(if focused { selected } else { None });
    if let Some(i) = show {
        let p = pts[i];
        let txt = format!("{}  {:+.1} dB", band_freq(i), gains[i]);
        let galley = painter.layout_no_wrap(txt, theme::font_strong(12.0), TEXT);
        let size = galley.size() + vec2(14.0, 6.0);
        let above = p.y - 14.0 - size.y > plot.top() - 10.0;
        let center = if above { p - vec2(0.0, 14.0 + size.y / 2.0) } else { p + vec2(0.0, 14.0 + size.y / 2.0) };
        let mut r = Rect::from_center_size(center, size);
        r = r.translate(vec2((rect.left() - r.left()).max(0.0) + (rect.right() - r.right()).min(0.0), 0.0));
        painter.rect(r, CornerRadius::same(5), theme::RAISED, Stroke::new(1.0, HAIRLINE), StrokeKind::Inside);
        painter.galley(r.center() - galley.size() / 2.0, galley, TEXT);
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(if dragging.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    changed
}
