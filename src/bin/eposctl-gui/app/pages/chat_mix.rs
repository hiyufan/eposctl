//! Game/voice balance panel (software ChatMix over the Windows mixer).

use eframe::egui::{RichText, Ui, vec2};

use super::super::App;
use super::exe_input;
use crate::theme::SECONDARY;
use crate::widgets;

/// Near the middle the slider snaps to the centre so "both full" is easy to hit.
const SNAP: f32 = 0.04;

impl App {
    pub(super) fn mix_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::card(ui, |ui| {
            let mut on = self.cfg.chat_mix.enabled;
            if widgets::setting_row(ui, s.mix_title, s.mix_desc, &mut on, true) {
                self.cfg.chat_mix.enabled = on;
                self.mark_dirty();
            }
            if !self.cfg.chat_mix.enabled {
                return;
            }
            ui.add_space(10.0);
            self.mix_balance(ui);
            widgets::spaced_divider(ui, 8.0, 8.0);
            self.voice_apps(ui);
        });
    }

    fn mix_balance(&mut self, ui: &mut Ui) {
        let s = self.s();
        let b = self.cfg.chat_mix.balance;
        let value = if b.abs() < 0.005 {
            s.mix_center.to_owned()
        } else {
            let side = if b > 0.0 { s.mix_game } else { s.mix_voice };
            s.mix_lowered.replace("{side}", side).replace("{pct}", &format!("{:.0}", b.abs() * 100.0))
        };
        widgets::row_title(ui, "", Some(value), true);
        let changed = ui
            .horizontal(|ui| {
                ui.label(RichText::new(s.mix_game).color(SECONDARY));
                let label_w = 48.0;
                let size = vec2((ui.available_width() - label_w).max(80.0), 26.0);
                let changed = ui.allocate_ui(size, |ui| balance_slider(ui, b)).inner;
                ui.label(RichText::new(s.mix_voice).color(SECONDARY));
                changed
            })
            .inner;
        if let Some(nb) = changed {
            self.cfg.chat_mix.balance = nb;
            self.mark_dirty();
        }
    }

    /// The voice chat programs, removable, and a field to add one.
    fn voice_apps(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::desc(ui, s.mix_apps);
        ui.add_space(6.0);
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, app) in self.cfg.chat_mix.voice_apps.iter().enumerate() {
                if widgets::ghost(ui, &format!("{app}  ×")).clicked() {
                    remove = Some(i);
                }
            }
        });
        ui.add_space(6.0);
        let add = exe_input(ui, &mut self.view.voice_input, s.mix_app_hint, s.add);

        let apps = &mut self.cfg.chat_mix.voice_apps;
        let mut dirty = false;
        if let Some(i) = remove {
            apps.remove(i);
            dirty = true;
        }
        if let Some(exe) = add
            && !apps.contains(&exe)
        {
            apps.push(exe);
            dirty = true;
        }
        if dirty {
            self.mark_dirty();
        }
    }
}

/// The balance as a 0..1 slider; the new balance (-1..1) when moved.
fn balance_slider(ui: &mut Ui, balance: f32) -> Option<f32> {
    let mut v = (balance + 1.0) / 2.0;
    let r = widgets::slider(ui, &mut v, true);
    if r.double_clicked() {
        return Some(0.0);
    }
    let nb = v * 2.0 - 1.0;
    r.changed().then_some(if nb.abs() < SNAP { 0.0 } else { nb })
}
