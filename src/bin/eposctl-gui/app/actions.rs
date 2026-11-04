//! User actions that more than one place triggers (page, tray, hotkey), and
//! the file imports and exports.

use eframe::egui::{self, Key, Modifiers, Ui};
use eposctl::apo::layout::BANDS;

use super::{App, EqTarget, read_with};
use crate::battery;
use crate::catalog::{self, PresetId};
use crate::config;
use crate::dialog;
use crate::hotkeys::Combo;
use crate::widgets::Notice;

impl App {
    // ---- presets ----

    pub(super) fn apply_preset(&mut self, id: PresetId) {
        let Some(sound) = id.sound(&self.cfg.custom_presets) else { return };
        if self.editor.apply_preset(&sound) {
            self.remember();
        }
    }

    /// Apply the next (`dir` = 1) or previous (-1) preset in the order shown on
    /// the Sound page.
    pub(super) fn step_preset(&mut self, dir: isize) {
        self.connect();
        let Some(current) = self.editor.sound() else { return };
        if let Some(id) = catalog::step(&current, &self.cfg.custom_presets, dir) {
            self.apply_preset(id);
        }
    }

    pub(super) fn toggle_surround(&mut self) {
        self.connect();
        if let Some(on) = self.editor.settings().map(|s| !s.surround()) {
            self.apply(|s| s.set_surround(on));
        }
    }

    /// The saved presets changed: save, and rebuild the tray menu.
    pub(super) fn presets_changed(&mut self) {
        self.mark_dirty();
        let (s, presets) = (self.s(), self.tray_presets());
        if let Some(t) = &mut self.tray {
            t.set_menu(s, &presets);
        }
    }

    // ---- undo ----

    pub(super) fn undo(&mut self) {
        if self.editor.undo() {
            self.remember();
        }
    }

    pub(super) fn redo(&mut self) {
        if self.editor.redo() {
            self.remember();
        }
    }

    /// Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z, unless a text field has the keyboard.
    pub(super) fn undo_shortcuts(&mut self, ui: &mut Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (redo_shift, undo, redo) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::Y),
            )
        });
        if undo {
            self.undo();
        } else if redo || redo_shift {
            self.redo();
        }
    }

    // ---- hotkeys ----

    /// While recording, the next key press becomes the hotkey. Runs before
    /// anything else reads keys so the press is not also handled as a shortcut.
    pub(super) fn record_hotkey(&mut self, ui: &mut Ui) {
        let Some(a) = self.view.recording else { return };
        let Some((key, m)) = ui.input_mut(take_key_press) else { return };
        let slot = self.cfg.hotkeys.slot(a.index());
        match key {
            Key::Escape => {}
            Key::Backspace => *slot = None,
            _ => match Combo::from_press(key, m) {
                Some(c) => *slot = Some(c.label()),
                None => return, // keep waiting for a valid combination
            },
        }
        self.view.recording = None;
        self.mark_dirty();
    }

    // ---- chat mix ----

    pub(super) fn nudge_mix(&mut self, by: f32) {
        let m = &mut self.cfg.chat_mix;
        if m.enabled {
            m.balance = ((m.balance + by) * 10.0).round().clamp(-10.0, 10.0) / 10.0;
            self.mark_dirty();
        }
    }

    // ---- battery ----

    /// "about 6 h left", from the measured drain rate; `None` while charging or unknown.
    pub(super) fn battery_left(&self, percent: u8) -> Option<String> {
        if self.battery.charging() {
            return None;
        }
        let h = battery::hours_left(percent, self.cfg.battery_rate?)?;
        let s = self.s();
        let t = if h >= 2.0 {
            s.hours.replace("{n}", &format!("{}", h.round() as u32))
        } else if h >= 1.0 {
            s.hours.replace("{n}", &format!("{h:.1}"))
        } else {
            s.minutes.replace("{n}", &format!("{}", ((h * 60.0 / 10.0).round() as u32 * 10).max(10)))
        };
        Some(s.battery_left.replace("{t}", &t))
    }

    // ---- files ----

    pub(super) fn export_presets(&mut self) {
        let s = self.s();
        let Some(path) = dialog::json_file(true, "eposctl-presets.json") else { return };
        let n = self.cfg.custom_presets.len();
        let notice = match std::fs::write(&path, config::export_presets(&self.cfg.custom_presets)) {
            Ok(()) => Notice::ok(s.exported.replace("{n}", &n.to_string())),
            Err(e) => Notice::error(s.io_failed.replace("{err}", &e.to_string())),
        };
        self.view.preset_files_notice = Some(notice);
    }

    pub(super) fn import_presets(&mut self) {
        let s = self.s();
        let Some(path) = dialog::json_file(false, "") else { return };
        let notice = match read_with(&path, config::parse_presets) {
            Err(e) => Notice::error(s.io_failed.replace("{err}", &e)),
            Ok(list) => match config::merge_presets(&mut self.cfg.custom_presets, list) {
                0 => Notice::ok(s.import_nothing_new),
                n => {
                    self.presets_changed();
                    Notice::ok(s.imported.replace("{n}", &n.to_string()))
                }
            },
        };
        self.view.preset_files_notice = Some(notice);
    }

    /// Load an Equalizer APO / AutoEQ file into the speaker or microphone EQ.
    pub(super) fn import_eq(&mut self, target: EqTarget) {
        let s = self.s();
        let Some(path) = dialog::eq_file() else { return };
        let notice = match read_with(&path, eposctl::eqfile::parse) {
            Err(e) => Notice::error(s.io_failed.replace("{err}", &e)),
            Ok(imp) => {
                let g = imp.gains;
                self.set_eq(target, &g);
                let mut m = s.eq_imported.replace("{n}", &imp.used.to_string());
                if imp.shift != 0.0 {
                    m += &s.eq_shifted.replace("{db}", &format!("{:+.1}", imp.shift));
                }
                if imp.clipped {
                    m += s.eq_clipped;
                }
                Notice::ok(m)
            }
        };
        self.view.eq_notice = Some((target, notice));
    }

    pub(super) fn set_eq(&mut self, target: EqTarget, gains: &[f32; BANDS]) {
        match target {
            EqTarget::Speaker => self.apply(|s| s.set_speaker_eq(gains)),
            EqTarget::Mic => self.apply(|s| s.set_mic_eq(gains)),
        }
    }
}

/// Remove all key and text events from this frame and return the first key press.
fn take_key_press(i: &mut egui::InputState) -> Option<(Key, Modifiers)> {
    let mut got = None;
    i.events.retain(|e| match e {
        egui::Event::Key { key, pressed: true, modifiers, .. } => {
            got = got.or(Some((*key, *modifiers)));
            false
        }
        egui::Event::Key { .. } | egui::Event::Text(_) => false,
        _ => true,
    });
    got
}
