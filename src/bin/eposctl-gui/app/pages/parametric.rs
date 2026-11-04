//! Parametric EQ panel: any number of bands, fitted to the headset's nine or
//! written exactly to Equalizer APO.

use eframe::egui::Ui;
use eposctl::apo::layout::BANDS;
use eposctl::audio::{self, Flow};
use eposctl::eqapo;
use eposctl::eqfile::Imported;
use eposctl::peq::Peq;

use super::super::{App, read_with};
use crate::config::{PeqConfig, PeqOutput};
use crate::dialog;
use crate::peq::{self, Changed};
use crate::widgets::{self, Notice};

/// The `Device:` word for Equalizer APO that selects the EPOS playback device.
fn epos_render_word() -> Option<String> {
    let ep = audio::epos_endpoint(Flow::Output).ok()??;
    eqapo::device_word(&ep.id)
}

fn format_hz(hz: f64) -> String {
    if hz >= 1000.0 { format!("{:.1} kHz", hz / 1000.0) } else { format!("{hz:.0} Hz") }
}

impl App {
    /// Edits a copy of the saved bands, then commits what changed.
    pub(super) fn peq_panel(&mut self, ui: &mut Ui) {
        let mut cfg = self.cfg.peq.clone();
        // Without the EPOS driver the only output is Equalizer APO. Set before
        // `before`, so opening the page alone writes nothing.
        if self.editor.settings().is_none() {
            cfg.output = PeqOutput::EqApo;
        }
        let before = cfg.output;
        let changed = widgets::panel().show(ui, |ui| self.peq_contents(ui, &mut cfg)).inner;
        self.peq_commit(cfg, before, changed);
    }

    fn peq_contents(&mut self, ui: &mut Ui, cfg: &mut PeqConfig) -> Changed {
        let s = self.s();
        let mut changed = widgets::heading_row(ui, s.peq_title, |ui| self.peq_actions(ui, cfg));
        widgets::desc(ui, s.peq_desc);
        ui.add_space(6.0);
        let fit = cfg.peq.to_epos();
        let fitted = (cfg.output == PeqOutput::Epos).then_some((&fit.gains, fit.shift));
        changed.merge(peq::plot(ui, &mut cfg.peq, fitted, 220.0));
        self.peq_fit_note(ui, cfg, &fit, &mut changed);
        ui.add_space(8.0);
        changed.merge(peq::table(ui, s, &mut cfg.peq));
        if cfg.output == PeqOutput::EqApo {
            ui.add_space(4.0);
            if widgets::ghost(ui, s.peq_apo_off).clicked() {
                self.remove_eqapo_bands();
                cfg.output = PeqOutput::Epos;
            }
        }
        if let Some(n) = &self.view.peq_notice {
            n.show(ui);
        }
        changed
    }

    /// Right to left: load a file, then the output choice.
    fn peq_actions(&mut self, ui: &mut Ui, cfg: &mut PeqConfig) -> Changed {
        let s = self.s();
        let mut changed = Changed::default();
        if widgets::ghost(ui, s.peq_load).on_hover_text(s.peq_load_tip).clicked()
            && let Some(path) = dialog::eq_file()
        {
            match read_with(&path, Peq::from_apo_text) {
                Ok(p) => {
                    cfg.peq = p;
                    cfg.peq.bands.truncate(peq::MAX_BANDS);
                    changed = Changed::DONE;
                    self.view.peq_notice = None;
                }
                Err(e) => self.view.peq_notice = Some(Notice::error(s.io_failed.replace("{err}", &e))),
            }
        }
        ui.add_space(8.0);
        let has_apo = self.view.eqapo_installed;
        let r = widgets::ghost_toggle(ui, s.peq_out_apo, has_apo, cfg.output == PeqOutput::EqApo);
        let r = if has_apo { r } else { r.on_disabled_hover_text(s.peq_apo_missing) };
        if r.clicked() && has_apo {
            cfg.output = PeqOutput::EqApo;
        }
        let has_epos = self.editor.settings().is_some();
        if widgets::ghost_toggle(ui, s.peq_out_epos, has_epos, cfg.output == PeqOutput::Epos).clicked() && has_epos {
            cfg.output = PeqOutput::Epos;
        }
        changed
    }

    /// How close the 9-band fit gets, and an "apply" button when the headset
    /// does not have it yet (saved bands are not applied until edited).
    fn peq_fit_note(&self, ui: &mut Ui, cfg: &PeqConfig, fit: &Imported, changed: &mut Changed) {
        let s = self.s();
        if cfg.peq.bands.is_empty() {
            return widgets::desc(ui, s.peq_empty);
        }
        if cfg.output == PeqOutput::EqApo {
            return widgets::desc(ui, s.peq_apo_note);
        }
        let (db, hz) = cfg.peq.worst_gap(fit);
        widgets::desc(ui, &s.peq_fit_note.replace("{db}", &format!("{db:.1}")).replace("{hz}", &format_hz(hz)));
        let current = self.editor.settings().map(|x| x.speaker_eq()).unwrap_or_default();
        let applied = current.iter().zip(&fit.gains).all(|(a, b)| (a - b).abs() < 0.05);
        if !applied && !changed.curve {
            ui.add_space(4.0);
            changed.curve |= widgets::primary(ui, s.peq_apply).clicked();
        }
    }

    /// Save the edited bands and send them to the chosen output.
    fn peq_commit(&mut self, cfg: PeqConfig, before: PeqOutput, changed: Changed) {
        let switched = cfg.output != before;
        if changed.any() || switched {
            self.cfg.peq = cfg.clone();
            self.mark_dirty();
        }
        match cfg.output {
            PeqOutput::Epos if changed.curve || switched => {
                let g = cfg.peq.to_epos().gains;
                self.apply(|st| st.set_speaker_eq(&g));
            }
            PeqOutput::EqApo if changed.settled || switched => self.write_eqapo_bands(&cfg.peq),
            _ => {}
        }
    }

    fn write_eqapo_bands(&mut self, peq: &Peq) {
        let res = epos_render_word()
            .ok_or_else(|| "no EPOS playback device".to_string())
            .and_then(|w| eqapo::write(Some(peq), &w).map_err(|e| e.to_string()));
        match res {
            Ok(_) => {
                self.view.peq_notice = None;
                // Otherwise the headset's own EQ would apply on top.
                self.apply(|st| st.set_speaker_eq(&[0.0; BANDS]));
            }
            Err(e) => self.view.peq_notice = Some(Notice::error(self.s().io_failed.replace("{err}", &e))),
        }
    }

    fn remove_eqapo_bands(&mut self) {
        let Some(word) = epos_render_word() else { return };
        self.view.peq_notice = match eqapo::write(None, &word) {
            Ok(_) => None,
            Err(e) => Some(Notice::error(self.s().io_failed.replace("{err}", &e.to_string()))),
        };
    }
}
