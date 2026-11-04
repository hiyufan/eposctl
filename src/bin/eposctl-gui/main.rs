//! eposctl GUI: tray app with sound, microphone, device and settings pages.
//!
//! Layout (see docs/architecture.md):
//! - `app`: the `App` that wires everything together, and its pages;
//! - models without UI: `editor` (driver settings, undo, A/B), `catalog`
//!   (presets), `config`, `history`, `autoswitch`, `battery`, `meters`;
//! - background services: `worker` (HID), `foreground`, `hotkeys`, `chatmix`,
//!   `devswitch`, `tray`;
//! - drawing: `widgets`, `eq`, `peq`, `icons`, `theme`;
//! - Windows plumbing: `instance`, `window`, `autostart`, `dialog`, `notify`.

#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod autoswitch;
mod battery;
mod catalog;
mod chatmix;
mod config;
#[cfg(debug_assertions)]
mod debug;
mod devswitch;
mod dialog;
mod editor;
mod eq;
mod foreground;
mod history;
mod hotkeys;
mod icons;
mod instance;
mod meters;
mod notify;
mod peq;
mod strings;
mod theme;
mod tray;
mod widgets;
mod window;
mod worker;

use eframe::egui;

fn main() -> eframe::Result {
    let Some(show_event) = instance::claim() else { return Ok(()) };
    let icon = egui::IconData { rgba: icons::bitmap(64, None, true), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("eposctl")
            .with_inner_size([1000.0, 780.0])
            .with_min_inner_size([820.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("eposctl", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, show_event)))))
}
