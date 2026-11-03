//! Switch the Windows default playback/recording devices when the headset is
//! turned off and on. The dongle stays plugged in, so Windows keeps sending
//! sound to a headset that is off; this moves it to the speakers and back.
//!
//! COM work runs on short-lived threads so the UI thread's COM apartment (which
//! the file dialogs rely on) is never touched.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use eposctl::audio::{self, Endpoint, Flow};

use crate::config::DeviceSwitch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Off,
    On,
}

/// Act on a headset power change. For each direction with a chosen device:
/// - off: if the default is an EPOS device, switch to the chosen one;
/// - on: if the default is still the chosen one, switch back to the EPOS device.
pub fn apply(event: Event, cfg: &DeviceSwitch) {
    let cfg = cfg.clone();
    std::thread::spawn(move || {
        for (flow, target) in [(Flow::Output, cfg.output), (Flow::Input, cfg.input)] {
            if let Some(target) = target {
                switch(flow, event, &target);
            }
        }
    });
}

fn switch(flow: Flow, event: Event, target: &str) {
    let (Ok(Some(current)), Ok(all)) = (audio::default_id(flow), audio::endpoints(flow)) else { return };
    let headset = all.iter().find(|e| audio::is_epos(&e.name));
    let current_is_headset = all.iter().any(|e| e.id == current && audio::is_epos(&e.name));
    let to = match event {
        Event::Off if current_is_headset && all.iter().any(|e| e.id == target) => target,
        Event::On if current == target => match headset {
            Some(h) => h.id.as_str(),
            None => return,
        },
        _ => return,
    };
    let _ = audio::set_default(to);
}

#[derive(Clone, Default)]
pub struct Lists {
    pub output: Vec<Endpoint>,
    pub input: Vec<Endpoint>,
}

/// Device lists for the settings page, fetched in the background.
pub struct Devices {
    lists: Arc<Mutex<Option<Lists>>>,
    fetched: Option<Instant>,
    ctx: egui::Context,
}

impl Devices {
    const STALE: Duration = Duration::from_secs(5);

    pub fn new(ctx: egui::Context) -> Self {
        Self { lists: Default::default(), fetched: None, ctx }
    }

    pub fn get(&self) -> Option<Lists> {
        self.lists.lock().unwrap().clone()
    }

    pub fn refresh(&mut self) {
        self.fetched = Some(Instant::now());
        let (lists, ctx) = (self.lists.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let l = Lists {
                output: audio::endpoints(Flow::Output).unwrap_or_default(),
                input: audio::endpoints(Flow::Input).unwrap_or_default(),
            };
            *lists.lock().unwrap() = Some(l);
            ctx.request_repaint();
        });
    }

    /// Re-read while the settings page is open, so a newly plugged device shows up.
    pub fn refresh_if_stale(&mut self) {
        if self.fetched.is_none_or(|t| t.elapsed() > Self::STALE) {
            self.refresh();
        }
        self.ctx.request_repaint_after(Self::STALE);
    }
}
