//! Sound page: presets, the speaker EQ, surround and reverb.

use eframe::egui::{RichText, Ui, vec2};
use eposctl::apo::layout::BANDS;

use super::super::{App, EqTarget};
use crate::catalog::{self, PresetId};
use crate::config::{CustomPreset, Sound};
use crate::theme::DANGER;
use crate::{eq, widgets};

/// What the preset bar asked for this frame.
#[derive(Default)]
struct PresetEdits {
    apply: Option<PresetId>,
    overwrite: Option<usize>,
    delete: Option<usize>,
}

impl App {
    pub(super) fn page_sound(&mut self, ui: &mut Ui) {
        let Some(sound) = self.editor.sound() else {
            self.apo_missing(ui);
            // Without the EPOS driver, Equalizer APO can still do the EQ.
            if self.view.eqapo_installed {
                ui.add_space(12.0);
                self.peq_panel(ui);
            }
            return;
        };
        self.preset_bar(ui, &sound);
        ui.add_space(16.0);
        self.speaker_eq_panel(ui);
        ui.add_space(12.0);
        if self.view.peq_open && !self.editor.is_comparing() {
            self.peq_panel(ui);
            ui.add_space(12.0);
        }
        self.surround_panel(ui, &sound);
        ui.add_space(12.0);
        self.mix_panel(ui);
    }

    // ---- presets ----

    fn preset_bar(&mut self, ui: &mut Ui, sound: &Sound) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let mut edits = PresetEdits { apply: self.factory_chips(ui, sound), ..PresetEdits::default() };
            self.custom_chips(ui, sound, &mut edits);
            ui.add_space(4.0);
            self.save_preset_field(ui, sound);
            self.apply_preset_edits(edits, sound);
        });
    }

    /// Factory and game presets; returns the one clicked.
    fn factory_chips(&self, ui: &mut Ui, sound: &Sound) -> Option<PresetId> {
        let s = self.s();
        let mut chosen = None;
        let mut first_game = true;
        for p in catalog::factory() {
            let game = catalog::is_game(p);
            if game && std::mem::take(&mut first_game) {
                ui.add_space(6.0);
            }
            let on = sound.matches(&Sound::of_preset(p));
            let mut r = widgets::chip(ui, catalog::factory_label(s, p.name), on);
            if game {
                r = r.on_hover_text(s.preset_game_tip);
            }
            if r.clicked() {
                chosen = Some(PresetId::Factory(p));
            }
        }
        chosen
    }

    /// Saved presets, with overwrite and delete in the context menu.
    fn custom_chips(&self, ui: &mut Ui, sound: &Sound, edits: &mut PresetEdits) {
        let s = self.s();
        for (i, p) in self.cfg.custom_presets.iter().enumerate() {
            let r = widgets::chip(ui, &p.name, sound.matches(&p.sound));
            if r.clicked() {
                edits.apply = Some(PresetId::Custom(i));
            }
            r.context_menu(|ui| {
                if ui.button(s.overwrite).clicked() {
                    edits.overwrite = Some(i);
                    ui.close();
                }
                if ui.button(RichText::new(s.delete).color(DANGER)).clicked() {
                    edits.delete = Some(i);
                    ui.close();
                }
            });
        }
    }

    /// "+ Save as preset", which turns into a name field.
    fn save_preset_field(&mut self, ui: &mut Ui, sound: &Sound) {
        let s = self.s();
        let Some(name) = &mut self.view.naming else {
            if widgets::ghost(ui, &format!("+  {}", s.save_preset)).clicked() {
                self.view.naming = Some(String::new());
            }
            return;
        };
        let te = ui.add(widgets::text_field(name, s.preset_name_hint, 150.0));
        if !te.has_focus() && name.is_empty() && !te.lost_focus() {
            te.request_focus();
        }
        let enter = te.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(eframe::egui::Key::Escape));
        let save = widgets::primary(ui, s.save).clicked() || enter;
        let cancel = widgets::ghost(ui, s.cancel).clicked() || esc;
        let trimmed = name.trim().to_owned();
        if save && !trimmed.is_empty() {
            self.cfg.custom_presets.push(CustomPreset { name: trimmed, sound: sound.clone() });
            self.view.naming = None;
            self.presets_changed();
        } else if cancel {
            self.view.naming = None;
        }
    }

    fn apply_preset_edits(&mut self, edits: PresetEdits, sound: &Sound) {
        if let Some(i) = edits.overwrite {
            self.cfg.custom_presets[i].sound = sound.clone();
            self.mark_dirty();
        }
        if let Some(i) = edits.delete {
            self.cfg.custom_presets.remove(i);
            self.presets_changed();
        }
        if let Some(id) = edits.apply {
            self.apply_preset(id);
        }
    }

    // ---- equalizer ----

    fn speaker_eq_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            widgets::heading_row(ui, s.equalizer, |ui| self.speaker_eq_actions(ui));
            ui.add_space(6.0);
            let editable = !self.editor.is_comparing();
            ui.add_enabled_ui(editable, |ui| {
                let mut g = self.editor.settings().map(eposctl::apo::Settings::speaker_eq).unwrap_or_default();
                if eq::editor(ui, "speaker", &mut g, 230.0, None) && editable {
                    self.apply(|st| st.set_speaker_eq(&g));
                }
            });
            self.eq_notice_line(ui, EqTarget::Speaker);
            widgets::desc(ui, s.eq_hint);
        });
    }

    /// Right to left: reset, import, parametric, undo, A/B compare.
    fn speaker_eq_actions(&mut self, ui: &mut Ui) {
        let s = self.s();
        let comparing = self.editor.is_comparing();
        if widgets::ghost_toggle(ui, s.reset, !comparing, false).clicked() {
            self.apply(|st| st.set_speaker_eq(&[0.0; BANDS]));
        }
        if widgets::ghost_toggle(ui, s.import, !comparing, false).on_hover_text(s.eq_import_tip).clicked() {
            self.import_eq(EqTarget::Speaker);
        }
        if widgets::ghost_toggle(ui, s.peq_button, !comparing, self.view.peq_open).clicked() {
            self.view.peq_open = !self.view.peq_open;
            if self.view.peq_open {
                self.view.eqapo_installed = eposctl::eqapo::config_dir().is_some();
            }
        }
        if widgets::ghost_toggle(ui, s.undo, self.editor.can_undo(), false).on_hover_text(s.undo_tip).clicked() {
            self.undo();
        }
        let label = if comparing { s.comparing } else { s.compare };
        let enabled = comparing || self.editor.differs_from_reference();
        if widgets::ghost_toggle(ui, label, enabled, comparing).on_hover_text(s.compare_tip).clicked() {
            if comparing {
                self.editor.stop_compare();
            } else {
                self.editor.start_compare();
            }
        }
    }

    /// The last EQ import's result, under the EQ it was for.
    pub(super) fn eq_notice_line(&self, ui: &mut Ui, target: EqTarget) {
        if let Some((t, notice)) = &self.view.eq_notice
            && *t == target
        {
            notice.show(ui);
            ui.add_space(4.0);
        }
    }

    // ---- surround ----

    /// Surround, with reverb beneath it because it only works with surround on.
    fn surround_panel(&mut self, ui: &mut Ui, sound: &Sound) {
        let s = self.s();
        let editable = !self.editor.is_comparing();
        widgets::card(ui, |ui| {
            if !editable {
                ui.disable();
            }
            let mut on = sound.surround;
            if widgets::setting_row(ui, s.surround, s.surround_desc, &mut on, true) {
                self.apply(|st| st.set_surround(on));
            }
            widgets::spaced_divider(ui, 10.0, 10.0);
            let enabled = sound.surround;
            let value = if enabled { format!("{:.0}%", sound.reverb * 100.0) } else { s.reverb_off_hint.to_owned() };
            widgets::row_title(ui, s.reverb, Some(value), enabled);
            let mut v = sound.reverb;
            if widgets::slider(ui, &mut v, enabled).changed() {
                self.apply(|st| st.set_reverb(v));
            }
        });
    }
}
