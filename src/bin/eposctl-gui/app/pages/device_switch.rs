//! Panel for switching the Windows default devices with the headset's power.

use eframe::egui::{RichText, Ui};
use eposctl::audio::{Endpoint, is_epos};

use super::super::App;
use super::per_app::preset_combo;
use crate::devswitch::Lists;
use crate::theme::{self, WARN};
use crate::widgets;

/// Devices to switch to: everything that is not the headset, as (id, name).
fn others(eps: &[Endpoint]) -> Vec<(String, String)> {
    eps.iter().filter(|e| !is_epos(&e.name)).map(|e| (e.id.clone(), e.name.clone())).collect()
}

impl App {
    pub(super) fn device_switch_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::card(ui, |ui| {
            let mut on = self.cfg.device_switch.enabled;
            if widgets::setting_row(ui, s.devsw_title, s.devsw_desc, &mut on, true) {
                self.cfg.device_switch.enabled = on;
                if on {
                    self.devices.refresh();
                }
                self.mark_dirty();
            }
            if !self.cfg.device_switch.enabled {
                return;
            }
            self.devices.refresh_if_stale();
            let Some(list) = self.devices.get() else {
                ui.add_space(6.0);
                return widgets::spinner(ui);
            };
            self.pick_default_devices(&list);
            widgets::spaced_divider(ui, 8.0, 4.0);
            headset_line(ui, s, &list);
            ui.add_space(4.0);
            self.device_choices(ui, &list);
        });
    }

    /// The first time: the first non-EPOS device of each kind.
    fn pick_default_devices(&mut self, list: &Lists) {
        let ds = &mut self.cfg.device_switch;
        if ds.output.is_none() && ds.input.is_none() {
            ds.output = list.output.iter().find(|e| !is_epos(&e.name)).map(|e| e.id.clone());
            ds.input = list.input.iter().find(|e| !is_epos(&e.name)).map(|e| e.id.clone());
            self.mark_dirty();
        }
    }

    fn device_choices(&mut self, ui: &mut Ui, list: &Lists) {
        let s = self.s();
        let ds = &mut self.cfg.device_switch;
        let rows = [(s.devsw_output, &list.output, &mut ds.output), (s.devsw_input, &list.input, &mut ds.input)];
        let mut dirty = false;
        for (i, (title, eps, slot)) in rows.into_iter().enumerate() {
            let choices = others(eps);
            dirty |= widgets::labeled_row(ui, title, |ui| {
                preset_combo(ui, ("devsw", i), slot, &choices, Some(s.devsw_none), s.devsw_missing)
            });
        }
        if dirty {
            self.mark_dirty();
        }
    }
}

/// Which EPOS devices were found, or a warning if none.
fn headset_line(ui: &mut Ui, s: &crate::strings::Strings, list: &Lists) {
    let names: Vec<&str> =
        list.output.iter().chain(&list.input).filter(|e| is_epos(&e.name)).map(|e| e.name.as_str()).collect();
    if names.is_empty() {
        ui.label(RichText::new(s.devsw_no_headset).font(theme::font(12.5)).color(WARN));
    } else {
        widgets::desc(ui, &s.devsw_headset.replace("{name}", &names.join(" · ")));
    }
}
