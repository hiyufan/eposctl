//! Settings panel for the official EPOS Gaming Suite next to eposctl.

use eframe::egui::{RichText, Ui};

use super::super::App;
use crate::official::Outcome;
use crate::theme::{self, SECONDARY};
use crate::widgets::{self, Notice};

impl App {
    /// Shown only when the Gaming Suite is installed.
    pub(super) fn official_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        if let Some(o) = self.official.take_outcome() {
            self.view.official_notice = Some(match o {
                Outcome::Done { on: false } => Notice::ok(s.suite_done_off),
                Outcome::Done { on: true } => Notice::ok(s.suite_done_on),
                Outcome::Declined => Notice::error(s.suite_declined),
                Outcome::Failed(e) => Notice::error(s.io_failed.replace("{err}", &e)),
            });
        }
        let st = self.official.status();
        if !st.installed {
            return;
        }
        let busy = self.official.busy();
        widgets::card(ui, |ui| {
            let title = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 230.0);
                    widgets::heading(ui, s.suite_title);
                    widgets::desc(ui, s.suite_desc);
                });
            };
            let clicked = widgets::split_row(ui, title, |ui| {
                if busy {
                    widgets::spinner(ui);
                    false
                } else if st.active() {
                    widgets::primary(ui, s.suite_off).clicked()
                } else {
                    widgets::ghost(ui, s.suite_on).clicked()
                }
            });
            if clicked {
                self.view.official_notice = None;
                self.official.switch(!st.active(), ui.ctx());
            }
            ui.add_space(6.0);
            let state = |b: bool, yes: &str, no: &str| if b { yes.to_owned() } else { no.to_owned() };
            let line = s
                .suite_state
                .replace("{svc}", &state(st.service_running, s.suite_running, s.suite_stopped))
                .replace("{svc_boot}", &state(st.service_autostart, s.suite_boot_on, s.suite_boot_off))
                .replace("{app}", &state(st.app_running, s.suite_running, s.suite_not_running))
                .replace("{app_boot}", &state(st.app_autostart, s.suite_boot_on, s.suite_boot_off));
            ui.label(RichText::new(line).font(theme::font(12.5)).color(SECONDARY));
            if busy {
                widgets::desc(ui, s.suite_waiting);
            }
            if let Some(n) = &self.view.official_notice {
                ui.add_space(4.0);
                n.show(ui);
            }
        });
        ui.add_space(12.0);
    }
}
