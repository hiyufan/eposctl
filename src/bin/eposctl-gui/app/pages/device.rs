//! Device page: what the headset and the driver report.

use eframe::egui::{Color32, Ui};

use super::super::App;
use crate::theme::{ACCENT, DANGER, FAINT, SECONDARY, TEXT, WARN};
use crate::widgets;
use crate::worker::{Cmd, DevState};

impl App {
    pub(super) fn page_device(&mut self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        let rows = self.device_rows(d);
        widgets::card(ui, |ui| {
            for (i, (label, value, color)) in rows.iter().enumerate() {
                if i > 0 {
                    widgets::divider(ui);
                }
                widgets::info_row(ui, label, value, *color);
            }
        });
        ui.add_space(12.0);
        if widgets::ghost(ui, s.refresh).clicked() {
            let _ = self.tx.send(Cmd::Refresh);
        }
    }

    /// (label, value, value colour) for each line.
    fn device_rows(&self, d: &DevState) -> Vec<(&'static str, String, Color32)> {
        let s = self.s();
        let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(|| s.unknown.to_owned());
        let link = match (d.dongle, d.linked) {
            (false, _) => (s.dongle_missing, SECONDARY),
            (true, Some(true)) => (s.linked, ACCENT),
            _ => (s.not_linked, SECONDARY),
        };
        let (anc, anc_color) = match d.anc {
            Some(true) => (s.anc_on, ACCENT),
            Some(false) => (s.anc_off, TEXT),
            None => (s.anc_unknown, FAINT),
        };
        let apo = match self.editor.device() {
            Some(dev) => (dev.to_string(), TEXT),
            None => (s.apo_missing_short.to_owned(), SECONDARY),
        };
        vec![
            (s.link, link.0.to_owned(), link.1),
            (s.product, or_dash(&d.product), TEXT),
            (s.serial, or_dash(&d.serial), TEXT),
            (s.firmware, or_dash(&d.firmware), TEXT),
            (s.battery, self.battery_text(d.battery), battery_color(d.battery)),
            (s.anc, anc.to_owned(), anc_color),
            (s.apo_channel, apo.0, apo.1),
        ]
    }

    fn battery_text(&self, battery: Option<u8>) -> String {
        let Some(b) = battery else { return self.s().unknown.to_owned() };
        match self.battery_left(b) {
            Some(left) => format!("{left}  ·  {b}%"),
            None => format!("{b}%"),
        }
    }
}

fn battery_color(battery: Option<u8>) -> Color32 {
    match battery {
        Some(0..=10) => DANGER,
        Some(11..=20) => WARN,
        _ => TEXT,
    }
}
