//! Parametric EQ editor: any number of bands, shown against what the headset's
//! 9-band EQ makes of them.

use eframe::egui::{self, Align2, Color32, DragValue, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use eposctl::apo::layout::BANDS;
use eposctl::eqfile::{CENTRES_HZ, Kind, band_label};
use eposctl::peq::{Band, Peq, epos_db};

use crate::strings::Strings;
use crate::theme::{self, ACCENT, DANGER, FAINT, SECONDARY, TEXT};
use crate::widgets;

const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20000.0;
const DB: f32 = 12.0;
pub const MAX_BANDS: usize = 20;

/// What changed this frame.
#[derive(Default)]
pub struct Changed {
    /// The curve changed (apply it).
    pub curve: bool,
    /// A drag or edit finished (a good moment for slower outputs).
    pub settled: bool,
}

impl Changed {
    /// Both at once: an edit that is complete in one go (a file, a new band).
    pub const DONE: Self = Self { curve: true, settled: true };

    pub fn merge(&mut self, other: Self) {
        self.curve |= other.curve;
        self.settled |= other.settled;
    }

    pub fn any(&self) -> bool {
        self.curve || self.settled
    }
}

/// Log-frequency by dB axes of the plot.
struct Axes {
    rect: Rect,
    plot: Rect,
}

fn lf(f: f32) -> f32 {
    f.max(1.0).log10()
}

impl Axes {
    fn new(rect: Rect) -> Self {
        Self { rect, plot: Rect::from_min_max(rect.min + vec2(40.0, 10.0), rect.max - vec2(12.0, 26.0)) }
    }

    fn x_of(&self, f: f32) -> f32 {
        egui::remap(lf(f), lf(F_MIN)..=lf(F_MAX), self.plot.left()..=self.plot.right())
    }

    fn f_of(&self, x: f32) -> f32 {
        10f32.powf(egui::remap_clamp(x, self.plot.left()..=self.plot.right(), lf(F_MIN)..=lf(F_MAX)))
    }

    fn y_of(&self, db: f32) -> f32 {
        egui::remap(db.clamp(-DB, DB), -DB..=DB, self.plot.bottom()..=self.plot.top())
    }

    fn db_of(&self, y: f32) -> f32 {
        egui::remap_clamp(y, self.plot.bottom()..=self.plot.top(), -DB..=DB)
    }

    /// Where a band's point is drawn (bands without gain sit on 0 dB).
    fn pos_of(&self, b: &Band) -> Pos2 {
        pos2(self.x_of(b.freq), self.y_of(if b.kind.has_gain() { b.gain } else { 0.0 }))
    }

    /// The band whose point is within reach of `p`.
    fn nearest(&self, p: Pos2, peq: &Peq) -> Option<usize> {
        peq.bands
            .iter()
            .enumerate()
            .map(|(i, b)| (i, self.pos_of(b).distance(p)))
            .filter(|(_, d)| *d < 14.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The response `db(f)` sampled every few pixels across the plot.
    fn trace(&self, db: &dyn Fn(f64) -> f64) -> Vec<Pos2> {
        let (plot, n) = (self.plot, (self.plot.width() / 3.0).max(40.0) as usize);
        (0..=n)
            .map(|k| {
                let x = plot.left() + plot.width() * k as f32 / n as f32;
                pos2(x, self.y_of(db(self.f_of(x) as f64) as f32))
            })
            .collect()
    }
}

/// The curve plot with one draggable point per band. `fitted` is the 9-band
/// fit of the curve and the level shift it was given, drawn as what the
/// headset makes of it (`None` when the bands go to Equalizer APO instead).
pub fn plot(ui: &mut Ui, peq: &mut Peq, fitted: Option<(&[f32; BANDS], f32)>, height: f32) -> Changed {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let axes = Axes::new(rect);
    let painter = ui.painter_at(rect.expand(2.0));
    draw_grid(&painter, &axes);
    draw_curves(&painter, &axes, peq, fitted);

    // Points: drag to move (frequency, gain), scroll to change Q.
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    let (dragging, mut out) = drag(ui, &resp, &axes, pointer, peq);
    let hovered = pointer.filter(|p| axes.plot.expand(14.0).contains(*p)).and_then(|p| axes.nearest(p, peq));
    if let Some(i) = hovered.or(dragging) {
        out.merge(scroll_q(ui, &resp, &mut peq.bands[i]));
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    draw_points(&painter, &axes, peq, |i| Some(i) == hovered || Some(i) == dragging);
    out
}

/// Grid: dB lines, and the headset's band centres.
fn draw_grid(painter: &egui::Painter, axes: &Axes) {
    let (plot, font) = (axes.plot, theme::font(11.0));
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = axes.y_of(db);
        let c = if db == 0.0 { Color32::from_rgb(0x34, 0x37, 0x40) } else { Color32::from_rgb(0x22, 0x24, 0x2A) };
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, c));
        painter.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{db:+.0}"), font.clone(), FAINT);
    }
    let column = Stroke::new(1.0, Color32::from_rgb(0x22, 0x24, 0x2A));
    for (i, &c) in CENTRES_HZ.iter().enumerate() {
        let x = axes.x_of(c);
        painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], column);
        let label_at = pos2(x, axes.rect.bottom() - 4.0);
        painter.text(label_at, Align2::CENTER_BOTTOM, band_label(i), font.clone(), SECONDARY);
    }
}

/// The target curve (dashed) and, when fitted, what the headset makes of it.
fn draw_curves(painter: &egui::Painter, axes: &Axes, peq: &Peq, fitted: Option<(&[f32; BANDS], f32)>) {
    if let Some((g, shift)) = fitted {
        // Undo the shift so the fit lines up with the target it was made from.
        let heard = axes.trace(&|f| epos_db(g, f) - shift as f64);
        painter.line(heard, Stroke::new(2.5, ACCENT));
    }
    let target = axes.trace(&|f| peq.db_at(f));
    let target_color = if fitted.is_some() { TEXT.gamma_multiply(0.55) } else { ACCENT };
    painter.add(egui::Shape::dashed_line(&target, Stroke::new(1.5, target_color), 5.0, 4.0));
}

/// Dragging a point moves its band. Returns the band being dragged.
fn drag(ui: &Ui, resp: &egui::Response, axes: &Axes, pointer: Option<Pos2>, peq: &mut Peq) -> (Option<usize>, Changed) {
    let id = ui.scope_id().with("peq_drag");
    let mut out = Changed::default();
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(id));
    if resp.drag_started()
        && let Some(p) = pointer
    {
        dragging = axes.nearest(p, peq);
        ui.data_mut(|d| d.insert_temp(id, dragging));
    }
    if resp.dragged()
        && let (Some(i), Some(p)) = (dragging, pointer)
        && let Some(b) = peq.bands.get_mut(i)
    {
        b.freq = (axes.f_of(p.x) * 10.0).round() / 10.0;
        if b.kind.has_gain() {
            b.gain = (axes.db_of(p.y) * 10.0).round() / 10.0;
        }
        out.curve = true;
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<usize>>(id));
        out.settled = dragging.is_some();
        dragging = None;
    }
    (dragging, out)
}

/// Scrolling over a point changes its band's Q.
fn scroll_q(ui: &Ui, resp: &egui::Response, band: &mut Band) -> Changed {
    let scroll = ui.input(|inp| inp.smooth_scroll_delta.y);
    if scroll == 0.0 || !resp.hovered() {
        return Changed::default();
    }
    band.q = (band.q * (1.0 + scroll * 0.004)).clamp(0.1, 20.0);
    Changed::DONE
}

fn draw_points(painter: &egui::Painter, axes: &Axes, peq: &Peq, active: impl Fn(usize) -> bool) {
    let font = theme::font(11.0);
    for (i, b) in peq.bands.iter().enumerate() {
        let p = axes.pos_of(b);
        let fill = if b.on { ACCENT } else { FAINT };
        painter.circle_filled(p, if active(i) { 7.0 } else { 5.5 }, fill);
        painter.text(p + vec2(0.0, -12.0), Align2::CENTER_BOTTOM, format!("{}", i + 1), font.clone(), SECONDARY);
    }
}

fn kind_label(s: &Strings, k: Kind) -> &'static str {
    match k {
        Kind::LowShelf => s.peq_low_shelf,
        Kind::HighShelf => s.peq_high_shelf,
        _ => s.peq_peak,
    }
}

/// The band list. Returns what changed.
pub fn table(ui: &mut Ui, s: &Strings, peq: &mut Peq) -> Changed {
    let mut out = Changed::default();
    let mut remove = None;
    egui::Grid::new("peq_bands").num_columns(7).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        for h in ["", s.peq_type, s.peq_freq, s.peq_gain, "Q", "", ""] {
            ui.label(egui::RichText::new(h).font(theme::font(12.0)).color(FAINT));
        }
        ui.end_row();
        for (i, b) in peq.bands.iter_mut().enumerate() {
            ui.label(egui::RichText::new(format!("{}", i + 1)).color(SECONDARY));
            let before = *b;
            egui::ComboBox::from_id_salt(("peq_kind", i)).selected_text(kind_label(s, b.kind)).width(70.0).show_ui(
                ui,
                |ui| {
                    for k in [Kind::Peak, Kind::LowShelf, Kind::HighShelf] {
                        ui.selectable_value(&mut b.kind, k, kind_label(s, k));
                    }
                },
            );
            let speed = b.freq * 0.01;
            let f = ui.add(DragValue::new(&mut b.freq).range(F_MIN..=F_MAX).speed(speed).suffix(" Hz").max_decimals(1));
            let g = ui.add_enabled(
                b.kind.has_gain(),
                DragValue::new(&mut b.gain).range(-DB..=DB).speed(0.1).suffix(" dB").max_decimals(1),
            );
            let q = ui.add(DragValue::new(&mut b.q).range(0.1..=20.0).speed(0.01).max_decimals(2));
            if widgets::switch(ui, &mut b.on, true).changed() {
                out.settled = true;
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("×").font(theme::font(18.0)).color(DANGER)).frame(false))
                .clicked()
            {
                remove = Some(i);
            }
            ui.end_row();
            if *b != before {
                out.curve = true;
            }
            if f.drag_stopped()
                || g.drag_stopped()
                || q.drag_stopped()
                || f.lost_focus()
                || g.lost_focus()
                || q.lost_focus()
            {
                out.settled = true;
            }
            if before.kind != b.kind {
                out.settled = true;
            }
        }
    });
    if let Some(i) = remove {
        peq.bands.remove(i);
        out.curve = true;
        out.settled = true;
    }
    if peq.bands.len() < MAX_BANDS && widgets::ghost(ui, &format!("+  {}", s.peq_add)).clicked() {
        // Put a new band where the curve has the most room: the widest gap.
        let mut fs: Vec<f32> = peq.bands.iter().map(|b| b.freq).collect();
        fs.extend([F_MIN, F_MAX]);
        fs.sort_by(f32::total_cmp);
        let gap = fs.windows(2).max_by(|a, b| (a[1] / a[0]).total_cmp(&(b[1] / b[0]))).map(|w| (w[0] * w[1]).sqrt());
        peq.bands.push(Band::peak(gap.unwrap_or(1000.0).round(), 0.0, 1.41));
        out.curve = true;
        out.settled = true;
    }
    out
}
