//! Settings page: general options, then one panel per feature, then About.

use eframe::egui::{RichText, Ui};

use super::super::App;
use crate::autostart;
use crate::strings::Lang;
use crate::theme::{SECONDARY, TEXT};
use crate::widgets;

impl App {
    pub(super) fn page_settings(&mut self, ui: &mut Ui) {
        widgets::card(ui, |ui| self.general(ui));
        ui.add_space(12.0);
        self.per_app_panel(ui);
        ui.add_space(12.0);
        self.shortcuts_panel(ui);
        ui.add_space(12.0);
        self.device_switch_panel(ui);
        ui.add_space(12.0);
        self.preset_files_panel(ui);
        ui.add_space(12.0);
        self.official_panel(ui);
        widgets::card(ui, |ui| self.about(ui));
    }

    fn general(&mut self, ui: &mut Ui) {
        let s = self.s();
        let label = |ui: &mut Ui| {
            ui.label(RichText::new(s.language).color(TEXT));
        };
        widgets::split_row(ui, label, |ui| {
            // Right-to-left: English first so 中文 ends up on the left.
            for (lang, label) in [(Lang::En, "English"), (Lang::Zh, "中文")] {
                if widgets::chip(ui, label, self.cfg.lang == lang).clicked() && self.cfg.lang != lang {
                    self.cfg.lang = lang;
                    self.presets_changed();
                }
            }
        });
        widgets::spaced_divider(ui, 4.0, 4.0);

        let mut v = self.cfg.close_to_tray;
        if widgets::setting_row(ui, s.close_to_tray, s.close_to_tray_desc, &mut v, false) {
            self.cfg.close_to_tray = v;
            self.mark_dirty();
        }
        widgets::spaced_divider(ui, 4.0, 4.0);

        let mut v = self.autostart;
        if widgets::setting_row(ui, s.autostart, s.autostart_desc, &mut v, false) && autostart::set(v).is_ok() {
            self.autostart = v;
        }
        widgets::spaced_divider(ui, 4.0, 4.0);

        let mut v = self.cfg.restore_on_start;
        if widgets::setting_row(ui, s.restore, s.restore_desc, &mut v, false) {
            self.cfg.restore_on_start = v;
            self.mark_dirty();
        }
        widgets::spaced_divider(ui, 4.0, 4.0);

        let mut v = self.cfg.battery_alerts;
        if widgets::setting_row(ui, s.battery_alerts, s.battery_alerts_desc, &mut v, false) {
            self.cfg.battery_alerts = v;
            self.mark_dirty();
        }
    }

    fn about(&self, ui: &mut Ui) {
        let s = self.s();
        ui.horizontal(|ui| {
            widgets::heading(ui, s.about);
            ui.label(RichText::new(format!("eposctl {}", env!("CARGO_PKG_VERSION"))).color(SECONDARY));
        });
        ui.add_space(2.0);
        ui.label(RichText::new(s.about_body).color(SECONDARY));
    }
}
