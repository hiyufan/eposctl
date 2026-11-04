//! Mic page: noise gate with the live level, sidetone, the headset's noise
//! reduction, and the microphone EQ with its spectrum.

use std::time::{Duration, Instant};

use eframe::egui::{Sense, Ui, vec2};
use eposctl::apo::layout::BANDS;

use super::super::{App, EqTarget};
use crate::worker::{Cmd, DevState};
use crate::{eq, widgets};

/// A sidetone value set here is shown this long before the headset's own reading.
const SIDETONE_HOLD: Duration = Duration::from_millis(1500);

/// Raw sidetone runs from 0xFF (quietest) to 0 (loudest); the UI from 0 to 1.
fn sidetone_ui(raw: u16) -> f32 {
    (255.0 - raw.min(255) as f32) / 255.0
}

fn sidetone_raw(v: f32) -> u16 {
    (255.0 * (1.0 - v)).round() as u16
}

impl App {
    pub(super) fn page_mic(&mut self, ui: &mut Ui, d: &DevState) {
        let Some(gate) = self.editor.settings().map(eposctl::apo::Settings::noise_gate) else {
            return self.apo_missing(ui);
        };
        ui.columns(2, |cols| {
            let (a, b) = cols.split_at_mut(1);
            widgets::panel().show(&mut a[0], |ui| self.noise_gate(ui, gate));
            widgets::panel().show(&mut b[0], |ui| self.sidetone(ui, d));
        });
        ui.add_space(12.0);
        // Hardware noise reduction (HID 0A/11); shown only when the headset reports a level.
        if let Some(level) = d.noise_reduction.filter(|_| d.linked == Some(true)) {
            widgets::panel().show(ui, |ui| self.noise_reduction(ui, level));
            ui.add_space(12.0);
        }
        widgets::panel().show(ui, |ui| self.mic_eq(ui));
    }

    fn noise_gate(&mut self, ui: &mut Ui, gate: f32) {
        let s = self.s();
        widgets::row_title(ui, s.noise_gate, Some(format!("{:.0}%", gate * 100.0)), true);
        widgets::desc(ui, s.noise_gate_desc);
        ui.add_space(6.0);
        let mut v = gate;
        if widgets::slider(ui, &mut v, true).changed() {
            self.apply(|st| st.set_noise_gate(v));
        }
        if self.editor.is_metering() {
            ui.add_space(2.0);
            widgets::level_meter(ui, s.input_level, self.meters.level, self.meters.peak, eq::METER_FLOOR_DB);
        }
    }

    fn sidetone(&mut self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        let linked = d.linked == Some(true) && d.sidetone.is_some();
        let pct = self
            .view
            .sidetone_drag
            .filter(|(_, t)| t.elapsed() < SIDETONE_HOLD)
            .map(|(v, _)| v)
            .or(d.sidetone.map(sidetone_ui))
            .unwrap_or(0.0);
        widgets::row_title(ui, s.sidetone, linked.then(|| format!("{:.0}%", pct * 100.0)), linked);
        widgets::desc(ui, if linked { s.sidetone_desc } else { s.sidetone_unavailable });
        ui.add_space(6.0);
        let mut v = pct;
        let r = widgets::slider(ui, &mut v, linked);
        if r.changed() {
            self.view.sidetone_drag = Some((v, Instant::now()));
            let _ = self.tx.send(Cmd::SetSidetone(sidetone_raw(v)));
        }
        if r.dragged()
            && let Some((_, t)) = &mut self.view.sidetone_drag
        {
            *t = Instant::now();
        }
        if self.editor.is_metering() {
            // Same footprint as the level meter in the noise-gate panel, so
            // both panels have the same height.
            ui.add_space(2.0);
            ui.allocate_exact_size(vec2(ui.available_width(), widgets::LEVEL_ROW_H), Sense::hover());
        }
    }

    fn noise_reduction(&mut self, ui: &mut Ui, level: u16) {
        let s = self.s();
        let title = |ui: &mut Ui| {
            ui.vertical(|ui| {
                widgets::row_title(ui, s.noise_reduction, None, true);
                widgets::desc(ui, s.noise_reduction_desc);
            });
        };
        widgets::split_row(ui, title, |ui| {
            for (l, label) in [(2, s.nr_strong), (1, s.nr_medium), (0, s.nr_off)] {
                if widgets::chip(ui, label, level == l).clicked() && level != l {
                    let _ = self.tx.send(Cmd::SetNoiseReduction(l));
                }
            }
        });
    }

    fn mic_eq(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::heading_row(ui, s.mic_eq, |ui| {
            if widgets::ghost(ui, s.reset).clicked() {
                self.apply(|st| st.set_mic_eq(&[0.0; BANDS]));
            }
            if widgets::ghost(ui, s.import).on_hover_text(s.eq_import_tip).clicked() {
                self.import_eq(EqTarget::Mic);
            }
        });
        ui.add_space(6.0);
        let mut g = self.editor.settings().map(eposctl::apo::Settings::mic_eq).unwrap_or_default();
        let bands = self.meters.bands;
        let levels = self.editor.is_metering().then_some(&bands[..]);
        if eq::editor(ui, "mic", &mut g, 230.0, levels) {
            self.apply(|st| st.set_mic_eq(&g));
        }
        self.eq_notice_line(ui, EqTarget::Mic);
        widgets::desc(ui, if levels.is_some() { s.mic_eq_hint } else { s.eq_hint });
    }
}
