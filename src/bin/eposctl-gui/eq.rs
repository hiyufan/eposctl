//! The 9-band EQ curve editor.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Key, Mesh, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
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

type Gains = [f32; CENTRES_HZ.len()];

/// Where things go: the plot area inside the widget, with room for dB labels on
/// the left and band labels below.
struct Geometry {
    rect: Rect,
    plot: Rect,
    n: usize,
}

impl Geometry {
    fn new(rect: Rect, n: usize) -> Self {
        Self { rect, plot: Rect::from_min_max(rect.min + vec2(40.0, 14.0), rect.max - vec2(14.0, 34.0)), n }
    }

    fn x_of(&self, i: usize) -> f32 {
        self.plot.left() + self.plot.width() * i as f32 / (self.n - 1) as f32
    }

    fn y_of(&self, db: f32) -> f32 {
        egui::remap(db, MIN_DB..=MAX_DB, self.plot.bottom()..=self.plot.top())
    }

    /// The gain at height `y`, rounded to 0.1 dB.
    fn db_of(&self, y: f32) -> f32 {
        let db = egui::remap_clamp(y, self.plot.bottom()..=self.plot.top(), MIN_DB..=MAX_DB);
        (db * 10.0).round() / 10.0
    }

    /// The band whose column is closest to `x`.
    fn nearest(&self, x: f32) -> usize {
        let last = (self.n - 1) as f32;
        ((x - self.plot.left()) / self.plot.width() * last).round().clamp(0.0, last) as usize
    }

    fn node(&self, i: usize, gains: &Gains) -> Pos2 {
        pos2(self.x_of(i), self.y_of(gains[i]))
    }
}

/// Selection and drag, kept across frames in egui's temp storage.
#[derive(Clone, Copy, Default)]
struct Interaction {
    selected: Option<usize>,
    dragging: Option<usize>,
}

impl Interaction {
    fn load(ui: &Ui, id: egui::Id) -> Self {
        ui.data(|d| d.get_temp(id)).unwrap_or_default()
    }

    fn store(self, ui: &Ui, id: egui::Id) {
        ui.data_mut(|d| d.insert_temp(id, self));
    }
}

/// Set band `i` to `v`; true if that changed it.
fn set(gains: &mut Gains, i: usize, v: f32) -> bool {
    let changed = gains[i] != v;
    gains[i] = v;
    changed
}

/// Draws the editor and returns true if `gains` changed this frame.
/// `levels` (dB, `METER_FLOOR_DB..=0`, one per band) are drawn as bars behind the
/// curve on their own scale: the full plot height spans the meter range.
pub fn editor(ui: &mut Ui, id_salt: &str, gains: &mut Gains, height: f32, levels: Option<&[f32]>) -> bool {
    let id = ui.scope_id().with(id_salt).with("state");
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let g = Geometry::new(rect, gains.len());
    let painter = ui.painter_at(rect.expand(2.0));

    let mut state = Interaction::load(ui, id);
    let mut changed = pointer(&g, &resp, &mut state, gains);
    if resp.has_focus() {
        changed |= keys(ui, &resp, &mut state, gains);
    }
    state.store(ui, id);

    let hovered = resp.hover_pos().filter(|p| g.plot.expand2(vec2(12.0, 12.0)).contains(*p)).map(|p| g.nearest(p.x));
    let focused = resp.has_focus();
    draw_grid(&painter, &g);
    if let Some(levels) = levels {
        draw_levels(&painter, &g, levels);
    }
    draw_curve(&painter, &g, gains);
    draw_nodes(&painter, &g, gains, state, hovered, focused);
    let show = state.dragging.or(hovered).or(state.selected.filter(|_| focused));
    if let Some(i) = show {
        draw_readout(&painter, &g, gains, i);
    }

    if resp.hovered() {
        let grab = if state.dragging.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab };
        ui.ctx().set_cursor_icon(grab);
    }
    changed
}

// ---- input ----

/// Drag a node; click a column to move its node there; double-click to reset.
fn pointer(g: &Geometry, resp: &Response, state: &mut Interaction, gains: &mut Gains) -> bool {
    let mut changed = false;
    let at = resp.interact_pointer_pos();
    if resp.drag_started()
        && let Some(p) = at
    {
        state.dragging = Some(g.nearest(p.x));
        state.selected = state.dragging;
        resp.request_focus();
    }
    if let (Some(i), Some(p)) = (state.dragging, at)
        && resp.dragged()
    {
        changed |= set(gains, i, g.db_of(p.y));
    }
    if resp.drag_stopped() {
        state.dragging = None;
    }
    if resp.clicked()
        && let Some(p) = at
    {
        let i = g.nearest(p.x);
        state.selected = Some(i);
        resp.request_focus();
        // A click on the column (not on the node) moves the node there.
        if (g.y_of(gains[i]) - p.y).abs() > 10.0 {
            changed |= set(gains, i, g.db_of(p.y));
        }
    }
    if resp.double_clicked()
        && let Some(p) = at
    {
        changed |= set(gains, g.nearest(p.x), 0.0);
    }
    changed
}

/// Left/Right select a band, Up/Down move it by 0.5 dB (1 dB with Shift).
fn keys(ui: &Ui, resp: &Response, state: &mut Interaction, gains: &mut Gains) -> bool {
    let n = gains.len();
    let sel = state.selected.get_or_insert(n / 2);
    let (up, down, left, right, big) = ui.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::ArrowLeft),
            i.key_pressed(Key::ArrowRight),
            i.modifiers.shift,
        )
    });
    if left && *sel > 0 {
        *sel -= 1;
    }
    if right && *sel + 1 < n {
        *sel += 1;
    }
    // Keep arrow keys from moving focus to another widget.
    ui.memory_mut(|m| {
        let filter = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
        m.set_focus_lock_filter(resp.id, filter);
    });
    let step = if big { 1.0 } else { 0.5 };
    let delta = match (up, down) {
        (true, _) => step,
        (false, true) => -step,
        _ => return false,
    };
    let v = ((gains[*sel] + delta) * 2.0).round() / 2.0;
    gains[*sel] = v.clamp(MIN_DB, MAX_DB);
    true
}

// ---- drawing ----

fn draw_grid(painter: &Painter, g: &Geometry) {
    let (plot, font) = (g.plot, theme::font(11.0));
    for db in [-6.0f32, -3.0, 0.0, 3.0, 6.0] {
        let y = g.y_of(db);
        let c = if db == 0.0 { HAIRLINE } else { Color32::from_rgb(0x26, 0x28, 0x2F) };
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, c));
        let txt = if db > 0.0 { format!("+{db:.0}") } else { format!("{db:.0}") };
        painter.text(pos2(plot.left() - 10.0, y), Align2::RIGHT_CENTER, txt, font.clone(), FAINT);
    }
    let column = Stroke::new(1.0, Color32::from_rgb(0x22, 0x24, 0x2A));
    for i in 0..g.n {
        let x = g.x_of(i);
        painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], column);
    }
    let bottom = g.rect.bottom() - 4.0;
    for i in 0..g.n {
        painter.text(pos2(g.x_of(i), bottom), Align2::CENTER_BOTTOM, band_label(i), font.clone(), SECONDARY);
    }
    painter.text(pos2(plot.left() - 10.0, bottom), Align2::RIGHT_BOTTOM, "Hz", font, FAINT);
}

/// Live level bars, under everything else that moves.
fn draw_levels(painter: &Painter, g: &Geometry, levels: &[f32]) {
    let plot = g.plot;
    let w = (plot.width() / (g.n - 1) as f32 * 0.42).min(30.0);
    for (i, &db) in levels.iter().enumerate().take(g.n) {
        let t = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
        if t <= 0.0 {
            continue;
        }
        let x = g.x_of(i);
        let top = plot.bottom() - plot.height() * t;
        let bar = Rect::from_min_max(pos2(x - w / 2.0, top), pos2(x + w / 2.0, plot.bottom()));
        painter.rect_filled(bar, CornerRadius::same(3), SECONDARY.gamma_multiply(0.16));
    }
}

/// A smooth curve through the nodes, kept inside the plot.
fn curve_points(g: &Geometry, gains: &Gains) -> Vec<Pos2> {
    const STEPS: usize = 16;
    let n = g.n;
    let pts: Vec<Pos2> = (0..n).map(|i| g.node(i, gains)).collect();
    let mut curve = Vec::with_capacity((n - 1) * STEPS + 1);
    for i in 0..n - 1 {
        let (p0, p3) = (pts[i.saturating_sub(1)], pts[(i + 2).min(n - 1)]);
        curve.extend((0..STEPS).map(|s| {
            let p = catmull_rom(p0, pts[i], pts[i + 1], p3, s as f32 / STEPS as f32);
            pos2(p.x, p.y.clamp(g.plot.top(), g.plot.bottom()))
        }));
    }
    curve.push(pts[n - 1]);
    curve
}

/// The curve, with a fill down (or up) to 0 dB.
fn draw_curve(painter: &Painter, g: &Geometry, gains: &Gains) {
    let curve = curve_points(g, gains);
    let zero = g.y_of(0.0);
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
}

fn draw_nodes(
    painter: &Painter,
    g: &Geometry,
    gains: &Gains,
    state: Interaction,
    hovered: Option<usize>,
    focused: bool,
) {
    for i in 0..g.n {
        let p = g.node(i, gains);
        let active = state.dragging == Some(i) || (state.dragging.is_none() && hovered == Some(i));
        let is_sel = focused && state.selected == Some(i);
        if active || is_sel {
            let guide = Stroke::new(1.0, ACCENT.gamma_multiply(0.25));
            painter.line_segment([pos2(p.x, g.plot.top()), pos2(p.x, g.plot.bottom())], guide);
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
}

/// Frequency and gain over node `i`, kept inside the widget.
fn draw_readout(painter: &Painter, g: &Geometry, gains: &Gains, i: usize) {
    let p = g.node(i, gains);
    let txt = format!("{}  {:+.1} dB", band_freq(i), gains[i]);
    let galley = painter.layout_no_wrap(txt, theme::font_strong(12.0), TEXT);
    let size = galley.size() + vec2(14.0, 6.0);
    let offset = vec2(0.0, 14.0 + size.y / 2.0);
    let above = p.y - 14.0 - size.y > g.plot.top() - 10.0;
    let r = Rect::from_center_size(if above { p - offset } else { p + offset }, size);
    let r = r.translate(vec2((g.rect.left() - r.left()).max(0.0) + (g.rect.right() - r.right()).min(0.0), 0.0));
    painter.rect(r, CornerRadius::same(5), theme::RAISED, Stroke::new(1.0, HAIRLINE), StrokeKind::Inside);
    painter.galley(r.center() - galley.size() / 2.0, galley, TEXT);
}
