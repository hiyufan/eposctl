//! Import and export of saved presets.

use eframe::egui::Ui;

use super::super::App;
use crate::widgets;

impl App {
    pub(super) fn preset_files_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::card(ui, |ui| {
            let title = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 170.0);
                    widgets::heading(ui, s.presets_io_title);
                    widgets::desc(ui, s.presets_io_desc);
                });
            };
            widgets::split_row(ui, title, |ui| {
                let any = !self.cfg.custom_presets.is_empty();
                if widgets::ghost_toggle(ui, s.export, any, false).clicked() {
                    self.export_presets();
                }
                if widgets::ghost(ui, s.import).clicked() {
                    self.import_presets();
                }
            });
            if let Some(n) = &self.view.preset_files_notice {
                ui.add_space(6.0);
                n.show(ui);
            }
        });
    }
}
