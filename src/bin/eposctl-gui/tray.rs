//! System tray icon and menu. Event handlers only set flags and wake the UI;
//! the UI thread acts on them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icons;
use crate::strings::Strings;

#[derive(Default)]
pub struct Flags {
    pub show: AtomicBool,
    pub quit: AtomicBool,
    pub toggle_surround: AtomicBool,
    pub preset: Mutex<Option<String>>,
}

impl Flags {
    pub fn take_show(&self) -> bool {
        self.show.swap(false, Ordering::Relaxed)
    }
    pub fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::Relaxed)
    }
    pub fn take_toggle_surround(&self) -> bool {
        self.toggle_surround.swap(false, Ordering::Relaxed)
    }
    pub fn take_preset(&self) -> Option<String> {
        self.preset.lock().unwrap().take()
    }
}

pub struct Tray {
    icon: TrayIcon,
    surround: CheckMenuItem,
    /// Surround state shown in the menu; `None` = no APO channel (item disabled).
    surround_state: Option<bool>,
    state: (Option<u8>, bool),
    tooltip: String,
}

fn icon(battery: Option<u8>, connected: bool) -> Option<Icon> {
    Icon::from_rgba(icons::bitmap(32, battery, connected), 32, 32).ok()
}

/// Menu entries: (id, label). Preset ids are prefixed with `preset:`.
fn menu(s: &Strings, presets: &[(String, String)], surround: Option<bool>) -> (Menu, CheckMenuItem) {
    let m = Menu::new();
    let _ = m.append(&MenuItem::with_id("show", s.tray_open, true, None));
    let _ = m.append(&PredefinedMenuItem::separator());
    for (id, label) in presets {
        let _ = m.append(&MenuItem::with_id(format!("preset:{id}"), label, true, None));
    }
    let _ = m.append(&PredefinedMenuItem::separator());
    let sur = CheckMenuItem::with_id("surround", s.surround, surround.is_some(), surround == Some(true), None);
    let _ = m.append(&sur);
    let _ = m.append(&PredefinedMenuItem::separator());
    let _ = m.append(&MenuItem::with_id("quit", s.tray_quit, true, None));
    (m, sur)
}

impl Tray {
    pub fn new(ctx: &egui::Context, flags: Arc<Flags>, s: &Strings, presets: &[(String, String)]) -> Option<Self> {
        let (m, surround) = menu(s, presets, None);
        let icon = TrayIconBuilder::new()
            .with_icon(icon(None, false)?)
            .with_tooltip("eposctl")
            .with_menu(Box::new(m))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;

        let (c, f) = (ctx.clone(), flags.clone());
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            let open = matches!(
                e,
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                    | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
            );
            if open {
                f.show.store(true, Ordering::Relaxed);
                c.request_repaint();
            }
        }));
        let (c, f) = (ctx.clone(), flags);
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let id = e.id().0.as_str();
            match id {
                "show" => f.show.store(true, Ordering::Relaxed),
                "quit" => f.quit.store(true, Ordering::Relaxed),
                "surround" => f.toggle_surround.store(true, Ordering::Relaxed),
                _ => {
                    if let Some(p) = id.strip_prefix("preset:") {
                        *f.preset.lock().unwrap() = Some(p.to_owned());
                    }
                }
            }
            c.request_repaint();
        }));

        Some(Self { icon, surround, surround_state: None, state: (None, false), tooltip: String::new() })
    }

    pub fn set_menu(&mut self, s: &Strings, presets: &[(String, String)]) {
        let (m, sur) = menu(s, presets, self.surround_state);
        self.icon.set_menu(Some(Box::new(m)));
        self.surround = sur;
    }

    /// Keep the menu's surround check in sync with the driver (`None` disables it).
    pub fn set_surround(&mut self, state: Option<bool>) {
        // Re-applied every call: muda flips the check itself when clicked.
        self.surround.set_enabled(state.is_some());
        self.surround.set_checked(state == Some(true));
        self.surround_state = state;
    }

    pub fn update(&mut self, battery: Option<u8>, connected: bool, tooltip: String) {
        if self.state != (battery, connected) {
            self.state = (battery, connected);
            let _ = self.icon.set_icon(icon(battery, connected));
        }
        if self.tooltip != tooltip {
            let _ = self.icon.set_tooltip(Some(&tooltip));
            self.tooltip = tooltip;
        }
    }
}
