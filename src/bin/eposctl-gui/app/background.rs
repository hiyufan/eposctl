//! Work done every frame, shown or not: tray and hotkey requests, the driver
//! connection, per-program presets, battery, metering, saving and closing.

use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportCommand};

use super::App;
use super::pages::Page;
use crate::autoswitch::Next;
use crate::catalog::PresetId;
use crate::devswitch;
use crate::hotkeys::{self, Action, Combo};
use crate::notify;

impl App {
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
        if self.flags.quit_requested() && !self.quitting {
            return self.quit(ctx);
        }
        if self.tray.is_some() {
            self.window.keep_hidden_at_start(ctx);
        }
        self.tray_requests(ctx);
        self.hotkey_requests(ctx);
        self.mixer.set(&self.cfg.chat_mix);

        self.connect();
        self.editor.sync(ctx.dragged_id().is_some());
        self.auto_switch(ctx);
        self.headset_status();
        self.meter(ctx);

        if self.autosave.due() {
            self.cfg.save();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            self.close(ctx);
        }
        // Keep polling the driver and config at a relaxed pace.
        ctx.request_repaint_after(Duration::from_millis(1000));
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        self.shut_down();
        self.tray = None; // remove the icon now rather than leaving a ghost
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// The window's close button: to the tray if enabled, else exit.
    fn close(&mut self, ctx: &egui::Context) {
        if self.cfg.close_to_tray && self.tray.is_some() {
            self.window.hide(ctx);
            self.editor.stop_compare();
            self.editor.set_metering(false);
        } else {
            self.cfg.save();
        }
    }

    fn tray_requests(&mut self, ctx: &egui::Context) {
        if self.flags.take_show() {
            self.window.show(ctx);
        }
        if self.flags.take_toggle_surround() {
            self.toggle_surround();
        }
        if let Some(id) = self.flags.take_preset().as_deref().and_then(PresetId::from_key) {
            self.connect();
            self.apply_preset(id);
        }
    }

    fn hotkey_requests(&mut self, ctx: &egui::Context) {
        if self.view.recording.is_some() && !ctx.input(|i| i.focused) {
            self.view.recording = None;
        }
        self.register_hotkeys();
        let fired = self.hotkeys.as_ref().map(super::super::hotkeys::Hotkeys::take_fired).unwrap_or_default();
        for a in fired {
            match a {
                Action::NextPreset => self.step_preset(1),
                Action::PrevPreset => self.step_preset(-1),
                Action::ToggleSurround => self.toggle_surround(),
                Action::MixVoice => self.nudge_mix(0.2),
                Action::MixGame => self.nudge_mix(-0.2),
            }
        }
    }

    /// Register the configured hotkeys; none while disabled or recording one.
    fn register_hotkeys(&mut self) {
        let hk = &mut self.cfg.hotkeys;
        let wanted = if hk.enabled && self.view.recording.is_none() {
            Action::ALL.map(|a| hk.slot(a.index()).as_deref().and_then(Combo::parse))
        } else {
            [None; hotkeys::N]
        };
        if let Some(h) = &mut self.hotkeys {
            h.set(wanted);
        }
    }

    fn auto_switch(&mut self, ctx: &egui::Context) {
        if !self.cfg.auto.enabled || self.editor.device().is_none() {
            return;
        }
        match self.auto.poll(&self.cfg.auto) {
            Next::Nothing => {}
            Next::WaitFor(wait) => ctx.request_repaint_after(wait),
            Next::Apply(rule) => {
                if let Some(id) = PresetId::from_rule(&rule, &self.cfg.custom_presets) {
                    self.apply_preset(id);
                }
            }
        }
    }

    /// Follow the headset's link and battery: default devices, the runtime
    /// estimate, low-battery warnings and the tray icon.
    fn headset_status(&mut self) {
        let d = self.dev.lock().unwrap().clone();
        let linked = d.linked == Some(true);
        let battery = d.battery.filter(|_| linked);

        if let Some(event) = self.follower.observe(d.linked)
            && self.cfg.device_switch.enabled
        {
            devswitch::apply(event, &self.cfg.device_switch);
        }
        if let Some(rate) = self.battery.observe(battery, Instant::now()) {
            self.cfg.battery_rate = Some(rate);
            self.mark_dirty();
        }
        let name = d.product.clone().unwrap_or_else(|| self.s().headset.to_owned());
        if let Some(b) = battery
            && self.low_battery.check(b)
        {
            self.warn_low_battery(&name, b);
        }
        let tip = self.tray_tooltip(name, linked, battery);
        let surround = self.editor.settings().map(eposctl::apo::Settings::surround);
        if let Some(t) = &mut self.tray {
            t.update(battery, linked, tip);
            t.set_surround(surround);
        }
    }

    fn warn_low_battery(&self, name: &str, percent: u8) {
        if self.cfg.battery_alerts {
            let s = self.s();
            let body = s.low_battery_body.replace("{name}", name).replace("{pct}", &percent.to_string());
            let _ = notify::toast(s.low_battery_title, &body);
        }
    }

    fn tray_tooltip(&self, name: String, linked: bool, battery: Option<u8>) -> String {
        let s = self.s();
        match (linked, battery) {
            (true, Some(b)) => match self.battery_left(b) {
                Some(left) => format!("{name}\n{} {b}% ({left})", s.tray_battery),
                None => format!("{name}\n{} {b}%", s.tray_battery),
            },
            (true, None) => name,
            _ => format!("{name}\n{}", s.tray_no_headset),
        }
    }

    /// Mic meters run while the Mic page is on screen.
    fn meter(&mut self, ctx: &egui::Context) {
        let wanted = self.view.page == Page::Mic && self.window.is_shown(ctx) && !self.quitting;
        self.editor.set_metering(wanted);
        if !wanted || !self.editor.is_metering() {
            return;
        }
        if let Some(raw) = self.editor.meters() {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            self.meters.update(&raw, dt);
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}
