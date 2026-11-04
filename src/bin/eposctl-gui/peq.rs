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

/// The curve plot with one draggable point per band. `fitted` is the 9-band
/// fit of the curve and the level shift it was given, drawn as what the
/// headset makes of it (`None` when the bands go to Equalizer APO instead).
pub fn plot(ui: &mut Ui, peq: &mut Peq, fitted: Option<(&[f32; BANDS], f32)>, height: f32) -> Changed {
    let mut out = Changed::default();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let painter = ui.painter_at(rect.expand(2.0));
    let plot = Rect::from_min_max(rect.min + vec2(40.0, 10.0), rect.max - vec2(12.0, 26.0));
    let lf = |f: f32| f.max(1.0).log10();
    let x_of = |f: f32| egui::remap(lf(f), lf(F_MIN)..=lf(F_MAX), plot.left()..=plot.right());
    let f_of = |x: f32| 10f32.powf(egui::remap_clamp(x, plot.left()..=plot.right(), lf(F_MIN)..=lf(F_MAX)));
    let y_of = |db: f32| egui::remap(db.clamp(-DB, DB), -DB..=DB, plot.bottom()..=plot.top());
    let db_of = |y: f32| egui::remap_clamp(y, plot.bottom()..=plot.top(), -DB..=DB);
    let font = theme::font(11.0);

    // Grid: dB lines, and the headset's band centres.
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        let c = if db == 0.0 { Color32::from_rgb(0x34, 0x37, 0x40) } else { Color32::from_rgb(0x22, 0x24, 0x2A) };
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, c));
        painter.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{db:+.0}"), font.clone(), FAINT);
    }
    for (i, &c) in CENTRES_HZ.iter().enumerate() {
        let x = x_of(c);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, Color32::from_rgb(0x22, 0x24, 0x2A)),
        );
        painter.text(pos2(x, rect.bottom() - 4.0), Align2::CENTER_BOTTOM, band_label(i), font.clone(), SECONDARY);
    }

    // Curves.
    let n = (plot.width() / 3.0).max(40.0) as usize;
    let pts = |f: &dyn Fn(f64) -> f64| -> Vec<Pos2> {
        (0..=n)
            .map(|k| {
                let x = plot.left() + plot.width() * k as f32 / n as f32;
                pos2(x, y_of(f(f_of(x) as f64) as f32))
            })
            .collect()
    };
    if let Some((g, shift)) = fitted {
        // Undo the shift so the fit lines up with the target it was made from.
        let heard = pts(&|f| epos_db(g, f) - shift as f64);
        painter.line(heard, Stroke::new(2.5, ACCENT));
    }
    let target = pts(&|f| peq.db_at(f));
    let target_color = if fitted.is_some() { TEXT.gamma_multiply(0.55) } else { ACCENT };
    painter.add(egui::Shape::dashed_line(&target, Stroke::new(1.5, target_color), 5.0, 4.0));

    // Points: drag to move (frequency, gain), scroll to change Q.
    let id = ui.scope_id().with("peq_drag");
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    let pos_of = |b: &Band| pos2(x_of(b.freq), if b.kind.has_gain() { y_of(b.gain) } else { y_of(0.0) });
    let nearest = |p: Pos2, peq: &Peq| {
        peq.bands
            .iter()
            .enumerate()
            .map(|(i, b)| (i, pos_of(b).distance(p)))
            .filter(|(_, d)| *d < 14.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(id));
    if resp.drag_started()
        && let Some(p) = pointer
    {
        dragging = nearest(p, peq);
        ui.data_mut(|d| d.insert_temp(id, dragging));
    }
    if resp.dragged()
        && let (Some(i), Some(p)) = (dragging, pointer)
        && let Some(b) = peq.bands.get_mut(i)
    {
        b.freq = (f_of(p.x) * 10.0).round() / 10.0;
        if b.kind.has_gain() {
            b.gain = (db_of(p.y) * 10.0).round() / 10.0;
        }
        out.curve = true;
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<usize>>(id));
        out.settled = dragging.is_some();
        dragging = None;
    }
    let hovered = pointer.filter(|p| plot.expand(14.0).contains(*p)).and_then(|p| nearest(p, peq));
    if let Some(i) = hovered.or(dragging) {
        let scroll = ui.input(|inp| inp.smooth_scroll_delta.y);
        if scroll != 0.0 && resp.hovered() {
            let b = &mut peq.bands[i];
            b.q = (b.q * (1.0 + scroll * 0.004)).clamp(0.1, 20.0);
            out.curve = true;
            out.settled = true;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    for (i, b) in peq.bands.iter().enumerate() {
        let p = pos_of(b);
        let active = Some(i) == hovered || Some(i) == dragging;
        let fill = if b.on { ACCENT } else { FAINT };
        painter.circle_filled(p, if active { 7.0 } else { 5.5 }, fill);
        painter.text(p + vec2(0.0, -12.0), Align2::CENTER_BOTTOM, format!("{}", i + 1), font.clone(), SECONDARY);
    }
    out
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
