//! What the window shows: navigation, header and one module per page or panel.

mod chat_mix;
mod device;
mod device_switch;
mod mic;
mod official;
mod parametric;
mod per_app;
mod preset_files;
mod settings;
mod shortcuts;
mod sound;

use eframe::egui::{self, CornerRadius, Rect, RichText, Sense, Stroke, Ui, vec2};

use super::App;
use crate::icons::{self, Icon};
use crate::theme::{self, ACCENT, FAINT, PANEL, RAISED, SECONDARY, TEXT};
use crate::widgets;
use crate::worker::DevState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Sound,
    Mic,
    Device,
    Settings,
}

impl Page {
    /// For `EPOSCTL_PAGE` in debug builds; unknown names give the Sound page.
    #[cfg(debug_assertions)]
    pub fn from_name(name: &str) -> Self {
        match name {
            "mic" => Self::Mic,
            "device" => Self::Device,
            "settings" => Self::Settings,
            _ => Self::Sound,
        }
    }
}

/// Normalise what the user typed into a lower-case executable file name.
fn exe_name(input: &str) -> Option<String> {
    let name = input.trim().trim_matches('"').rsplit(['\\', '/']).next()?.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    Some(if name.contains('.') { name } else { format!("{name}.exe") })
}

/// A program-name field with an add button: the typed name, normalised, when
/// submitted. The field is cleared then.
fn exe_input(ui: &mut Ui, input: &mut String, hint: &str, button: &str) -> Option<String> {
    let exe = widgets::submit_field(ui, input, hint, button).as_deref().and_then(exe_name)?;
    input.clear();
    Some(exe)
}

impl App {
    /// The current page, scrollable.
    pub(super) fn page(&mut self, ui: &mut Ui, d: &DevState) {
        let area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        #[cfg(debug_assertions)]
        let area = match crate::debug::scroll() {
            Some(y) => area.vertical_scroll_offset(y),
            None => area,
        };
        area.show(ui, |ui| {
            match self.view.page {
                Page::Sound => self.page_sound(ui),
                Page::Mic => self.page_mic(ui, d),
                Page::Device => self.page_device(ui, d),
                Page::Settings => self.page_settings(ui),
            }
            ui.add_space(8.0);
        });
    }

    fn go_to(&mut self, page: Page) {
        self.view.page = page;
        self.view.naming = None;
        self.view.recording = None;
        self.editor.stop_compare();
    }

    /// The navigation rail.
    pub(super) fn nav(&mut self, ui: &mut Ui) {
        let s = self.s();
        ui.add_space(18.0);
        // Brand mark: the same ring as the tray icon.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(3.0, ACCENT));
        ui.painter().circle_filled(r.center(), 4.0, ACCENT);
        ui.add_space(22.0);

        let items = [
            (Page::Sound, Icon::Sound, s.nav_sound),
            (Page::Mic, Icon::Mic, s.nav_mic),
            (Page::Device, Icon::Device, s.nav_device),
            (Page::Settings, Icon::Settings, s.nav_settings),
        ];
        for (page, icon, label) in items {
            if nav_item(ui, icon, label, self.view.page == page) {
                self.go_to(page);
            }
            ui.add_space(4.0);
        }
    }

    /// Product name with link and battery status.
    pub(super) fn header(&self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        let name = d.product.clone().unwrap_or_else(|| s.headset.to_owned());
        let title = |ui: &mut Ui| {
            ui.label(RichText::new(name).font(theme::font_strong(24.0)).color(TEXT));
        };
        widgets::split_row(ui, title, |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (text, dot) = match (d.dongle, d.linked) {
                (false, _) => (s.dongle_missing, FAINT),
                (true, Some(true)) => (s.linked, ACCENT),
                (true, _) => (s.not_linked, FAINT),
            };
            if let (true, Some(true), Some(b)) = (d.dongle, d.linked, d.battery) {
                ui.label(RichText::new(format!("{b}%")).font(theme::font_strong(15.0)).color(TEXT));
                widgets::battery(ui, b);
                ui.add_space(10.0);
            }
            ui.label(RichText::new(text).color(SECONDARY));
            widgets::dot(ui, dot);
        });
    }

    /// Placeholder while the EPOS driver's channel is not found.
    fn apo_missing(&self, ui: &mut Ui) {
        let s = self.s();
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 28.0, PANEL);
            icons::paint(ui.painter(), r.shrink(14.0), Icon::Device, SECONDARY);
            ui.add_space(14.0);
            ui.label(RichText::new(s.apo_missing_title).font(theme::font_strong(18.0)).color(TEXT));
            ui.add_space(4.0);
            ui.set_max_width(420.0);
            ui.label(RichText::new(s.apo_missing_body).color(SECONDARY));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let text_w = ui.painter().layout_no_wrap(s.apo_retrying.into(), theme::font(13.0), FAINT).size().x;
                ui.add_space((ui.available_width() - (14.0 + 8.0 + text_w)).max(0.0) / 2.0);
                widgets::spinner(ui);
                ui.label(RichText::new(s.apo_retrying).font(theme::font(13.0)).color(FAINT));
            });
        });
    }
}

/// One entry of the navigation rail. True when clicked.
fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
    let color = match (selected, resp.hovered()) {
        (true, _) => ACCENT,
        (false, true) => TEXT,
        (false, false) => SECONDARY,
    };
    let p = ui.painter();
    if selected {
        p.rect_filled(rect.shrink2(vec2(10.0, 3.0)), CornerRadius::same(10), RAISED);
        let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 18.0), vec2(3.0, rect.height() - 36.0));
        p.rect_filled(bar, CornerRadius::same(2), ACCENT);
    }
    icons::paint(p, Rect::from_center_size(rect.center() - vec2(0.0, 9.0), vec2(22.0, 22.0)), icon, color);
    p.text(rect.center() + vec2(0.0, 15.0), egui::Align2::CENTER_CENTER, label, theme::font(12.0), color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::exe_name;

    #[test]
    fn exe_names_are_normalised() {
        assert_eq!(exe_name(" Valorant ").as_deref(), Some("valorant.exe"));
        assert_eq!(exe_name(r#""C:\Riot Games\VALORANT\Game.EXE""#).as_deref(), Some("game.exe"));
        assert_eq!(exe_name("D:/Games/cs2.exe").as_deref(), Some("cs2.exe"));
        assert_eq!(exe_name("   "), None);
    }
}
