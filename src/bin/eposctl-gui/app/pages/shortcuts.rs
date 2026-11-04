//! Global hotkeys panel.

use eframe::egui::{RichText, Ui};

use super::super::App;
use crate::hotkeys::Action;
use crate::theme::{self, DANGER};
use crate::widgets;

impl App {
    pub(super) fn shortcuts_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::card(ui, |ui| {
            let mut on = self.cfg.hotkeys.enabled;
            if widgets::setting_row(ui, s.hotkeys_title, s.hotkeys_desc, &mut on, true) {
                self.cfg.hotkeys.enabled = on;
                self.view.recording = None;
                self.mark_dirty();
            }
            if !self.cfg.hotkeys.enabled {
                return;
            }
            widgets::spaced_divider(ui, 8.0, 4.0);
            let rows = [
                (Action::NextPreset, s.hk_next),
                (Action::PrevPreset, s.hk_prev),
                (Action::ToggleSurround, s.hk_surround),
                (Action::MixVoice, s.hk_mix_voice),
                (Action::MixGame, s.hk_mix_game),
            ];
            for (a, label) in rows {
                widgets::labeled_row(ui, label, |ui| self.hotkey_button(ui, a));
            }
            if self.view.recording.is_some() {
                ui.add_space(4.0);
                widgets::desc(ui, s.hk_record_hint);
            }
        });
    }

    /// The combination as a button that starts (or cancels) recording, with a
    /// warning if another program holds it.
    fn hotkey_button(&mut self, ui: &mut Ui, a: Action) {
        let s = self.s();
        let recording = self.view.recording == Some(a);
        let text = if recording {
            s.hk_press.to_owned()
        } else {
            self.cfg.hotkeys.slot(a.index()).clone().unwrap_or_else(|| s.hk_unset.to_owned())
        };
        if widgets::ghost_toggle(ui, &text, true, recording).clicked() {
            self.view.recording = if recording { None } else { Some(a) };
        }
        if !recording && self.hotkeys.as_ref().is_some_and(|h| h.failed(a)) {
            ui.label(RichText::new(s.hk_in_use).font(theme::font(12.5)).color(DANGER));
        }
    }
}
