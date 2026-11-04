//! eposctl GUI: tray app with sound, microphone, device and settings pages.

#![windows_subsystem = "windows"]

mod battery;
mod chatmix;
mod config;
mod devswitch;
mod dialog;
mod eq;
mod foreground;
mod hotkeys;
mod icons;
mod notify;
mod peq;
mod strings;
mod theme;
mod tray;
mod widgets;
mod worker;

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use eposctl::apo::{ApoChannel, Settings, layout::BANDS};
use eposctl::presets::{self, Preset};

use config::{AppRule, Config, CustomPreset, Last, PeqOutput, Sound};
use hotkeys::{Action, Combo};
use icons::Icon;
use strings::{Lang, Strings, t};
use theme::{ACCENT, DANGER, FAINT, PANEL, RAISED, SECONDARY, TEXT, WARN};
use worker::{Cmd, DevState};

/// Everything an undo step restores.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    speaker_eq: [f32; BANDS],
    mic_eq: [f32; BANDS],
    reverb: f32,
    noise_gate: f32,
    surround: bool,
}

impl Snapshot {
    fn of(s: &Settings) -> Self {
        Self {
            speaker_eq: s.speaker_eq(),
            mic_eq: s.mic_eq(),
            reverb: s.reverb(),
            noise_gate: s.noise_gate(),
            surround: s.surround(),
        }
    }

    fn write(&self, s: &mut Settings) {
        s.set_speaker_eq(&self.speaker_eq);
        s.set_mic_eq(&self.mic_eq);
        s.set_reverb(self.reverb);
        s.set_noise_gate(self.noise_gate);
        s.set_surround(self.surround);
    }
}

/// Edits closer together than this (or within one drag) form one undo step.
const UNDO_GROUP: Duration = Duration::from_millis(600);
const UNDO_DEPTH: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Sound,
    Mic,
    Device,
    Settings,
}

struct App {
    cfg: Config,
    page: Page,

    apo: Option<ApoChannel>,
    next_apo_try: Instant,
    next_sync: Instant,
    s: Option<Settings>,

    dev: worker::Shared,
    tx: Sender<Cmd>,

    flags: Arc<tray::Flags>,
    tray: Option<tray::Tray>,

    /// Value being set from the UI, shown until the headset confirms it.
    sidetone_drag: Option<(f32, Instant)>,
    naming: Option<String>,
    autostart: bool,
    cfg_dirty_at: Option<Instant>,
    quitting: bool,
    /// Frames left during which the window is kept hidden (`--minimized`).
    /// eframe shows the window after its first paint, so it is hidden again here.
    hide_frames: u8,
    /// Lowest battery threshold already notified; cleared once the level recovers.
    battery_warned: Option<u8>,

    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The current undo step stays open until this instant.
    group_until: Option<Instant>,
    /// A/B reference: the sound when a preset was last picked or the driver was found.
    ab_ref: Option<Sound>,
    /// While comparing: the current sound, put back when the comparison ends.
    comparing: Option<Sound>,

    fg: foreground::Shared,
    /// Foreground program the per-app rules were last evaluated for.
    auto_app: Option<String>,
    /// Preset those rules picked (`None`: leave the sound alone).
    auto_target: Option<String>,
    /// Program name typed into the "add rule" field.
    auto_input: String,

    hotkeys: Option<hotkeys::Hotkeys>,
    /// Hotkey being recorded; all hotkeys are unregistered meanwhile so the
    /// current ones can be pressed again.
    recording: Option<Action>,
    /// Result of the last preset import/export: (message, succeeded).
    io_msg: Option<(String, bool)>,
    /// Result of the last EQ file import: (microphone EQ, message, ok).
    eq_msg: Option<(bool, String, bool)>,
    /// The parametric EQ panel is open.
    peq_open: bool,
    /// Equalizer APO is installed (checked when the panel opens).
    eqapo: bool,
    /// Result of the last parametric EQ file load or Equalizer APO write: (message, ok).
    peq_msg: Option<(String, bool)>,

    /// Headset link state seen last, to act on power on/off.
    last_linked: Option<bool>,
    devices: devswitch::Devices,
    battery: battery::Tracker,
    mixer: chatmix::Mixer,
    /// Program name typed into the voice-apps field.
    mix_input: String,

    /// Original meter flag while we have switched it on for the meters.
    monitors: Option<bool>,
    /// Recording stream held open while metering: the APO only meters the mic
    /// while some program records from it.
    recorder: Option<eposctl::audio::KeepRecording>,
    meters: Meters,
    /// We hid the window (tray). Tracked here because the viewport's own
    /// visibility info does not reliably update after `Visible(false)`.
    hidden: bool,
}

/// Smoothed microphone meters for display: fast attack, steady fall, peak hold.
struct Meters {
    bands: [f32; BANDS],
    level: f32,
    peak: f32,
    peak_at: Instant,
}

impl Meters {
    const FALL_DB_PER_S: f32 = 30.0;
    const PEAK_HOLD: Duration = Duration::from_millis(1500);

    fn new() -> Self {
        let f = eq::METER_FLOOR_DB;
        Self { bands: [f; BANDS], level: f, peak: f, peak_at: Instant::now() }
    }

    fn update(&mut self, raw: &[f32], dt: f32) {
        let f = eq::METER_FLOOR_DB;
        let follow = |d: &mut f32, t: f32| {
            *d = if t >= *d { t } else { (*d - Self::FALL_DB_PER_S * dt).max(t) };
        };
        for (d, &r) in self.bands.iter_mut().zip(raw) {
            follow(d, r.clamp(f, 0.0));
        }
        // Overall level: power sum of the bands.
        let p: f32 = raw.iter().take(BANDS).map(|&r| 10f32.powf(r.clamp(f, 0.0) / 10.0)).sum();
        let total = (10.0 * p.log10()).clamp(f, 0.0);
        follow(&mut self.level, total);
        if total >= self.peak || self.peak_at.elapsed() > Self::PEAK_HOLD {
            self.peak = total;
            self.peak_at = Instant::now();
        }
    }
}

fn factory_label(s: &Strings, name: &str) -> &'static str {
    match name {
        "music" => s.preset_music,
        "esport" => s.preset_esport,
        "movie" => s.preset_movie,
        "flat" => s.preset_flat,
        "valorant" => s.preset_valorant,
        "delta-force" => s.preset_delta_force,
        "cod" => s.preset_cod,
        "pubg" => s.preset_pubg,
        _ => s.preset_custom,
    }
}

/// Factory presets in the order the official app shows them, then the game presets.
fn factory() -> impl Iterator<Item = &'static Preset> {
    ["music", "esport", "movie", "flat"].into_iter().filter_map(presets::find).chain(presets::GAMES)
}

fn is_game(p: &Preset) -> bool {
    presets::GAMES.iter().any(|g| g.name == p.name)
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.05)
}

fn sound_of(s: &Settings) -> Sound {
    Sound { speaker_eq: s.speaker_eq(), reverb: s.reverb(), surround: s.surround() }
}

/// Normalise what the user typed into a lower-case executable file name.
fn exe_name(input: &str) -> Option<String> {
    let name = input.trim().trim_matches('"').rsplit(['\\', '/']).next()?.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    Some(if name.contains('.') { name } else { format!("{name}.exe") })
}

/// Drop-down of presets. `none` adds a first entry meaning "no preset".
fn preset_combo(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    value: &mut Option<String>,
    choices: &[(String, String)],
    none: Option<&str>,
    missing: &str,
) -> bool {
    let before = value.clone();
    let text = match value.as_deref() {
        None => none.unwrap_or_default().to_owned(),
        Some(v) => choices.iter().find(|(k, _)| k == v).map_or_else(|| missing.to_owned(), |(_, l)| l.clone()),
    };
    egui::ComboBox::from_id_salt(id).selected_text(text).width(150.0).show_ui(ui, |ui| {
        if let Some(n) = none {
            ui.selectable_value(value, None, n);
        }
        for (k, l) in choices {
            ui.selectable_value(value, Some(k.clone()), l);
        }
    });
    *value != before
}

fn write_sound(s: &mut Settings, sound: &Sound) {
    s.set_speaker_eq(&sound.speaker_eq);
    s.set_surround(sound.surround);
    s.set_reverb(sound.reverb);
}

fn matches(cur: &Sound, eq: &[f32; BANDS], reverb: f32, surround: bool) -> bool {
    close(&cur.speaker_eq, eq) && cur.surround == surround && (!surround || (cur.reverb - reverb).abs() < 0.01)
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, show_event: isize) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);
        let cfg = Config::load();
        let ctx = cc.egui_ctx.clone();
        let (dev, tx) = worker::spawn(move || ctx.request_repaint());
        let flags = Arc::new(tray::Flags::default());
        watch_show_event(show_event, flags.clone(), cc.egui_ctx.clone());
        let mut app = Self {
            page: Page::Sound,
            apo: None,
            next_apo_try: Instant::now(),
            next_sync: Instant::now(),
            s: None,
            dev,
            tx,
            tray: None,
            flags: flags.clone(),
            sidetone_drag: None,
            naming: None,
            autostart: config::autostart::is_enabled(),
            cfg_dirty_at: None,
            quitting: false,
            hide_frames: if std::env::args().any(|a| a == "--minimized") { 3 } else { 0 },
            battery_warned: None,
            undo: Vec::new(),
            redo: Vec::new(),
            group_until: None,
            ab_ref: None,
            comparing: None,
            fg: foreground::spawn(cc.egui_ctx.clone()),
            auto_app: None,
            auto_target: None,
            auto_input: String::new(),
            hotkeys: hotkeys::Hotkeys::spawn(cc.egui_ctx.clone()),
            recording: None,
            io_msg: None,
            eq_msg: None,
            peq_open: false,
            eqapo: eposctl::eqapo::config_dir().is_some(),
            peq_msg: None,
            last_linked: None,
            devices: devswitch::Devices::new(cc.egui_ctx.clone()),
            battery: battery::Tracker::default(),
            mixer: chatmix::Mixer::spawn(),
            mix_input: String::new(),
            monitors: None,
            recorder: None,
            meters: Meters::new(),
            hidden: false,
            cfg,
        };
        // Debug builds: `EPOSCTL_PAGE=mic|device|settings|peq` opens on that page (for screenshots).
        #[cfg(debug_assertions)]
        if let Ok(p) = std::env::var("EPOSCTL_PAGE") {
            app.page = match p.as_str() {
                "mic" => Page::Mic,
                "device" => Page::Device,
                "settings" => Page::Settings,
                _ => Page::Sound,
            };
            if p == "peq" {
                app.peq_open = true;
                app.eqapo = eposctl::eqapo::config_dir().is_some();
            }
        }
        // Debug builds: `EPOSCTL_TEST_TOAST=1` shows a sample low-battery notification.
        #[cfg(debug_assertions)]
        if std::env::var_os("EPOSCTL_TEST_TOAST").is_some() {
            let s = app.s();
            let body = s.low_battery_body.replace("{name}", "EPOS H3PRO").replace("{pct}", "20");
            if let Err(e) = notify::toast(s.low_battery_title, &body) {
                let _ = std::fs::write(std::env::temp_dir().join("eposctl-toast-error.txt"), e.to_string());
            }
        }
        app.tray = tray::Tray::new(&cc.egui_ctx, flags, app.s(), &app.tray_presets());
        app
    }

    fn s(&self) -> &'static Strings {
        t(self.cfg.lang)
    }

    fn tray_presets(&self) -> Vec<(String, String)> {
        let s = self.s();
        let mut v: Vec<_> = factory().map(|p| (p.name.to_owned(), factory_label(s, p.name).to_owned())).collect();
        v.extend(self.cfg.custom_presets.iter().enumerate().map(|(i, p)| (format!("custom{i}"), p.name.clone())));
        v
    }

    fn mark_dirty(&mut self) {
        self.cfg_dirty_at = Some(Instant::now());
    }

    // ---- APO channel ----

    fn ensure_apo(&mut self) {
        if self.apo.is_some() || Instant::now() < self.next_apo_try {
            return;
        }
        self.next_apo_try = Instant::now() + Duration::from_secs(2);
        let Ok(ch) = ApoChannel::open(None) else { return };
        if self.cfg.restore_on_start {
            let last = self.cfg.last.clone();
            let _ = ch.modify(|s| {
                if let Some(v) = last.speaker_eq {
                    s.set_speaker_eq(&v);
                }
                if let Some(v) = last.mic_eq {
                    s.set_mic_eq(&v);
                }
                if let Some(v) = last.reverb {
                    s.set_reverb(v);
                }
                if let Some(v) = last.noise_gate {
                    s.set_noise_gate(v);
                }
                if let Some(v) = last.surround {
                    s.set_surround(v);
                }
            });
        }
        self.s = ch.read().ok();
        self.ab_ref = self.s.as_ref().map(sound_of);
        self.comparing = None;
        self.apo = Some(ch);
    }

    /// Re-read the block so changes from elsewhere (CLI, official app) show up.
    fn sync(&mut self, ctx: &egui::Context) {
        if Instant::now() < self.next_sync || ctx.dragged_id().is_some() {
            return;
        }
        self.next_sync = Instant::now() + Duration::from_millis(1000);
        if let Some(ch) = &self.apo {
            match ch.read() {
                Ok(s) => self.s = Some(s),
                Err(_) => self.drop_apo(),
            }
        }
    }

    fn drop_apo(&mut self) {
        self.apo = None;
        self.s = None;
        self.monitors = None;
        self.recorder = None;
    }

    /// The APO only updates its level meters while `mic_meters_enabled` is set.
    /// Switch it on while the Mic page is on screen and put back whatever was
    /// there before. (The official app also switched on the mic EQ and the gate
    /// here, so they only acted while its mic page was open.)
    fn set_monitoring(&mut self, on: bool) {
        let Some(ch) = &self.apo else { return };
        match (on, self.monitors) {
            (true, None) => {
                if let Ok(cur) = ch.read() {
                    let original = cur.mic_meters();
                    if ch.modify(|s| s.set_mic_meters(true)).is_ok() {
                        self.monitors = Some(original);
                        self.recorder = eposctl::audio::KeepRecording::epos();
                    }
                }
            }
            (false, Some(original)) => {
                self.recorder = None;
                let _ = ch.modify(|s| s.set_mic_meters(original));
                self.monitors = None;
            }
            _ => {}
        }
    }

    /// A user edit: ends any A/B comparison, records an undo step, remembers the result.
    fn apply(&mut self, f: impl FnOnce(&mut Settings)) {
        self.stop_compare();
        let before = self.s.as_ref().map(Snapshot::of);
        if self.write(f) {
            if let (Some(b), Some(now)) = (before, self.s.as_ref().map(Snapshot::of))
                && b != now
            {
                self.record(b);
            }
            self.remember();
        }
    }

    /// Write to the driver without touching undo history or the saved settings.
    fn write(&mut self, f: impl FnOnce(&mut Settings)) -> bool {
        let Some(ch) = &self.apo else { return false };
        match ch.modify(f) {
            Ok(s) => {
                self.s = Some(s);
                true
            }
            Err(_) => {
                self.drop_apo();
                false
            }
        }
    }

    fn record(&mut self, before: Snapshot) {
        let now = Instant::now();
        if !self.group_until.is_some_and(|t| now < t) {
            self.undo.push(before);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.group_until = Some(now + UNDO_GROUP);
        self.redo.clear();
    }

    fn undo(&mut self) {
        self.stop_compare();
        let Some(cur) = self.s.as_ref().map(Snapshot::of) else { return };
        let Some(prev) = self.undo.pop() else { return };
        if self.write(|s| prev.write(s)) {
            self.redo.push(cur);
            self.group_until = None;
            self.remember();
        }
    }

    fn redo(&mut self) {
        self.stop_compare();
        let Some(cur) = self.s.as_ref().map(Snapshot::of) else { return };
        let Some(next) = self.redo.pop() else { return };
        if self.write(|s| next.write(s)) {
            self.undo.push(cur);
            self.group_until = None;
            self.remember();
        }
    }

    fn start_compare(&mut self) {
        let Some(reference) = self.ab_ref.clone() else { return };
        let Some(cur) = self.s.as_ref().map(sound_of) else { return };
        if self.write(|s| write_sound(s, &reference)) {
            self.comparing = Some(cur);
        }
    }

    fn stop_compare(&mut self) {
        if let Some(cur) = self.comparing.take() {
            self.write(|s| write_sound(s, &cur));
        }
    }

    /// Save the current driver state as the "last" settings restored on start.
    fn remember(&mut self) {
        let Some(s) = &self.s else { return };
        self.cfg.last = Last {
            speaker_eq: Some(s.speaker_eq()),
            mic_eq: Some(s.mic_eq()),
            reverb: Some(s.reverb()),
            noise_gate: Some(s.noise_gate()),
            surround: Some(s.surround()),
        };
        self.mark_dirty();
    }

    fn apply_preset_id(&mut self, id: &str) {
        // Picking a preset is its own undo step and a fresh start for A/B.
        self.group_until = None;
        if let Some(p) = presets::find(id) {
            self.apply(|s| p.apply(s));
        } else if let Some(c) = id.strip_prefix("custom").and_then(|i| i.parse::<usize>().ok())
            && let Some(p) = self.cfg.custom_presets.get(c).cloned()
        {
            self.apply(|s| write_sound(s, &p.sound));
        } else {
            return;
        }
        self.ab_ref = self.s.as_ref().map(sound_of);
        self.group_until = None;
    }

    fn toggle_surround(&mut self) {
        self.ensure_apo();
        if let Some(on) = self.s.as_ref().map(|s| !s.surround()) {
            self.apply(|s| s.set_surround(on));
        }
    }

    /// Apply the next (`dir` = 1) or previous (-1) preset in the order shown on
    /// the Sound page. From a sound that matches no preset, start at either end.
    fn step_preset(&mut self, dir: isize) {
        self.ensure_apo();
        let Some(cur) = self.s.as_ref().map(sound_of) else { return };
        let ids: Vec<String> = self.tray_presets().into_iter().map(|(id, _)| id).collect();
        if ids.is_empty() {
            return;
        }
        let at = factory()
            .map(|p| matches(&cur, &p.speaker_eq, p.reverb, p.surround))
            .chain(self.cfg.custom_presets.iter().map(|p| matches(&cur, &p.sound.speaker_eq, p.sound.reverb, p.sound.surround)))
            .position(|m| m);
        let n = ids.len() as isize;
        let i = match at {
            Some(i) => (i as isize + dir).rem_euclid(n),
            None if dir > 0 => 0,
            None => n - 1,
        };
        self.apply_preset_id(&ids[i as usize]);
    }

    // ---- global hotkeys ----

    fn sync_hotkeys(&mut self) {
        let hk = &mut self.cfg.hotkeys;
        let wanted = if hk.enabled && self.recording.is_none() {
            Action::ALL.map(|a| hk.slot(a.index()).as_deref().and_then(Combo::parse))
        } else {
            [None; hotkeys::N]
        };
        if let Some(h) = &mut self.hotkeys {
            h.set(wanted);
        }
    }

    /// While recording, the next key press becomes the hotkey. Runs before
    /// anything else reads keys so the press is not also handled as a shortcut.
    fn record_hotkey(&mut self, ui: &mut Ui) {
        let Some(a) = self.recording else { return };
        let press = ui.input_mut(|i| {
            let mut got = None;
            i.events.retain(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    got = got.or(Some((*key, *modifiers)));
                    false
                }
                egui::Event::Key { .. } | egui::Event::Text(_) => false,
                _ => true,
            });
            got
        });
        let Some((key, m)) = press else { return };
        let slot = self.cfg.hotkeys.slot(a.index());
        match key {
            egui::Key::Escape => {}
            egui::Key::Backspace => *slot = None,
            _ => match Combo::from_press(key, m) {
                Some(c) => *slot = Some(c.label()),
                None => return, // keep waiting for a valid combination
            },
        }
        self.recording = None;
        self.mark_dirty();
    }

    fn hotkeys_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut on = self.cfg.hotkeys.enabled;
            if Self::setting_row(ui, s.hotkeys_title, s.hotkeys_desc, &mut on, true) {
                self.cfg.hotkeys.enabled = on;
                self.recording = None;
                self.mark_dirty();
            }
            if !self.cfg.hotkeys.enabled {
                return;
            }
            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let rows = [
                (Action::NextPreset, s.hk_next),
                (Action::PrevPreset, s.hk_prev),
                (Action::ToggleSurround, s.hk_surround),
                (Action::MixVoice, s.hk_mix_voice),
                (Action::MixGame, s.hk_mix_game),
            ];
            for (a, label) in rows {
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.label(RichText::new(label).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let rec = self.recording == Some(a);
                        let text = if rec {
                            s.hk_press.to_owned()
                        } else {
                            self.cfg.hotkeys.slot(a.index()).clone().unwrap_or_else(|| s.hk_unset.to_owned())
                        };
                        if widgets::ghost_toggle(ui, &text, true, rec).clicked() {
                            self.recording = if rec { None } else { Some(a) };
                        }
                        if !rec && self.hotkeys.as_ref().is_some_and(|h| h.failed(a)) {
                            ui.label(RichText::new(s.hk_in_use).font(theme::font(12.5)).color(DANGER));
                        }
                    });
                });
            }
            if self.recording.is_some() {
                ui.add_space(4.0);
                widgets::desc(ui, s.hk_record_hint);
            }
        });
    }

    /// "about 6 h left", from the measured drain rate; `None` while charging or unknown.
    fn battery_left(&self, percent: u8) -> Option<String> {
        if self.battery.charging() {
            return None;
        }
        let h = battery::hours_left(percent, self.cfg.battery_rate?)?;
        let s = self.s();
        let t = if h >= 2.0 {
            s.hours.replace("{n}", &format!("{}", h.round() as u32))
        } else if h >= 1.0 {
            s.hours.replace("{n}", &format!("{:.1}", h))
        } else {
            s.minutes.replace("{n}", &format!("{}", ((h * 60.0 / 10.0).round() as u32 * 10).max(10)))
        };
        Some(s.battery_left.replace("{t}", &t))
    }

    // ---- default device follows the headset ----

    fn follow_headset(&mut self, linked: Option<bool>) {
        let prev = std::mem::replace(&mut self.last_linked, linked);
        if prev == linked || !self.cfg.device_switch.enabled {
            return;
        }
        // Off: also on the first reading, so a headset that is already off at start
        // is handled. On: only after we saw it off, so starting eposctl never
        // overrides a device picked by hand.
        let event = match (prev, linked) {
            (_, Some(false)) => devswitch::Event::Off,
            (Some(false), Some(true)) => devswitch::Event::On,
            _ => return,
        };
        devswitch::apply(event, &self.cfg.device_switch);
    }

    fn devswitch_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut on = self.cfg.device_switch.enabled;
            if Self::setting_row(ui, s.devsw_title, s.devsw_desc, &mut on, true) {
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
                widgets::spinner(ui);
                return;
            };
            // Pick sensible devices the first time: the first non-EPOS one of each kind.
            let ds = &mut self.cfg.device_switch;
            if ds.output.is_none() && ds.input.is_none() {
                ds.output = list.output.iter().find(|e| !eposctl::audio::is_epos(&e.name)).map(|e| e.id.clone());
                ds.input = list.input.iter().find(|e| !eposctl::audio::is_epos(&e.name)).map(|e| e.id.clone());
                self.mark_dirty();
            }
            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let headset: Vec<&str> = list.output.iter().chain(&list.input).filter(|e| eposctl::audio::is_epos(&e.name)).map(|e| e.name.as_str()).collect();
            if headset.is_empty() {
                ui.label(RichText::new(s.devsw_no_headset).font(theme::font(12.5)).color(WARN));
            } else {
                widgets::desc(ui, &s.devsw_headset.replace("{name}", &headset.join(" · ")));
            }
            ui.add_space(4.0);
            let mut dirty = false;
            for (title, eps, which) in [(s.devsw_output, &list.output, 0), (s.devsw_input, &list.input, 1)] {
                let choices: Vec<(String, String)> =
                    eps.iter().filter(|e| !eposctl::audio::is_epos(&e.name)).map(|e| (e.id.clone(), e.name.clone())).collect();
                let slot = if which == 0 { &mut self.cfg.device_switch.output } else { &mut self.cfg.device_switch.input };
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.label(RichText::new(title).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        dirty |= preset_combo(ui, ("devsw", which), slot, &choices, Some(s.devsw_none), s.devsw_missing);
                    });
                });
            }
            if dirty {
                self.mark_dirty();
            }
        });
    }

    // ---- preset files ----

    fn presets_io_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 170.0);
                    widgets::heading(ui, s.presets_io_title);
                    widgets::desc(ui, s.presets_io_desc);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let any = !self.cfg.custom_presets.is_empty();
                    if widgets::ghost_toggle(ui, s.export, any, false).clicked() {
                        self.export_presets();
                    }
                    if widgets::ghost(ui, s.import).clicked() {
                        self.import_presets();
                    }
                });
            });
            if let Some((msg, ok)) = &self.io_msg {
                ui.add_space(6.0);
                ui.label(RichText::new(msg).font(theme::font(12.5)).color(if *ok { ACCENT } else { DANGER }));
            }
        });
    }

    fn export_presets(&mut self) {
        let s = self.s();
        let Some(path) = dialog::json_file(true, "eposctl-presets.json") else { return };
        let n = self.cfg.custom_presets.len();
        self.io_msg = Some(match std::fs::write(&path, config::export_presets(&self.cfg.custom_presets)) {
            Ok(()) => (s.exported.replace("{n}", &n.to_string()), true),
            Err(e) => (s.io_failed.replace("{err}", &e.to_string()), false),
        });
    }

    fn import_presets(&mut self) {
        let s = self.s();
        let Some(path) = dialog::json_file(false, "") else { return };
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| config::parse_presets(&t));
        self.io_msg = Some(match parsed {
            Ok(list) => match config::merge_presets(&mut self.cfg.custom_presets, list) {
                0 => (s.import_nothing_new.to_owned(), true),
                n => {
                    self.after_presets_changed();
                    (s.imported.replace("{n}", &n.to_string()), true)
                }
            },
            Err(e) => (s.io_failed.replace("{err}", &e), false),
        });
    }

    /// Load an Equalizer APO / AutoEQ file into the speaker or microphone EQ.
    fn import_eq(&mut self, mic: bool) {
        let s = self.s();
        let Some(path) = dialog::eq_file() else { return };
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| eposctl::eqfile::parse(&t));
        let (msg, ok) = match parsed {
            Ok(imp) => {
                let g = imp.gains;
                self.apply(|st| if mic { st.set_mic_eq(&g) } else { st.set_speaker_eq(&g) });
                let mut m = s.eq_imported.replace("{n}", &imp.used.to_string());
                if imp.shift != 0.0 {
                    m += &s.eq_shifted.replace("{db}", &format!("{:+.1}", imp.shift));
                }
                if imp.clipped {
                    m += s.eq_clipped;
                }
                (m, true)
            }
            Err(e) => (s.io_failed.replace("{err}", &e), false),
        };
        self.eq_msg = Some((mic, msg, ok));
    }

    fn peq_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        let mut cfg = self.cfg.peq.clone();
        // Without the EPOS driver the only output is Equalizer APO. Taken before
        // `old_output`, so opening the page alone writes nothing.
        let has_epos = self.s.is_some();
        if !has_epos {
            cfg.output = PeqOutput::EqApo;
        }
        let old_output = cfg.output;
        let mut changed = peq::Changed::default();
        let current = self.s.as_ref().map(|x| x.speaker_eq()).unwrap_or_default();
        widgets::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(ui, s.peq_title);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::ghost(ui, s.peq_load).on_hover_text(s.peq_load_tip).clicked()
                        && let Some(path) = dialog::eq_file()
                    {
                        let loaded = std::fs::read_to_string(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|t| eposctl::peq::Peq::from_apo_text(&t));
                        match loaded {
                            Ok(p) => {
                                cfg.peq = p;
                                cfg.peq.bands.truncate(peq::MAX_BANDS);
                                changed.curve = true;
                                changed.settled = true;
                                self.peq_msg = None;
                            }
                            Err(e) => self.peq_msg = Some((s.io_failed.replace("{err}", &e), false)),
                        }
                    }
                    ui.add_space(8.0);
                    let r = widgets::ghost_toggle(ui, s.peq_out_apo, self.eqapo, cfg.output == PeqOutput::EqApo);
                    let r = if self.eqapo { r } else { r.on_disabled_hover_text(s.peq_apo_missing) };
                    if r.clicked() && self.eqapo {
                        cfg.output = PeqOutput::EqApo;
                    }
                    if widgets::ghost_toggle(ui, s.peq_out_epos, has_epos, cfg.output == PeqOutput::Epos).clicked() && has_epos {
                        cfg.output = PeqOutput::Epos;
                    }
                });
            });
            widgets::desc(ui, s.peq_desc);
            ui.add_space(6.0);
            let fit = cfg.peq.to_epos();
            let fitted = (cfg.output == PeqOutput::Epos).then_some((&fit.gains, fit.shift));
            let c = peq::plot(ui, &mut cfg.peq, fitted, 220.0);
            changed.curve |= c.curve;
            changed.settled |= c.settled;
            if cfg.peq.bands.is_empty() {
                widgets::desc(ui, s.peq_empty);
            } else if cfg.output == PeqOutput::Epos {
                let (db, hz) = peq::worst_gap(&cfg.peq, &fit.gains, fit.shift);
                let hz = if hz >= 1000.0 { format!("{:.1} kHz", hz / 1000.0) } else { format!("{hz:.0} Hz") };
                widgets::desc(ui, &s.peq_fit_note.replace("{db}", &format!("{db:.1}")).replace("{hz}", &hz));
                // Saved bands are not applied until edited; offer it when the headset differs.
                let applied = current.iter().zip(&fit.gains).all(|(a, b)| (a - b).abs() < 0.05);
                if !applied && !changed.curve {
                    ui.add_space(4.0);
                    if widgets::primary(ui, s.peq_apply).clicked() {
                        changed.curve = true;
                    }
                }
            } else {
                widgets::desc(ui, s.peq_apo_note);
            }
            ui.add_space(8.0);
            let c = peq::table(ui, s, &mut cfg.peq);
            changed.curve |= c.curve;
            changed.settled |= c.settled;
            if cfg.output == PeqOutput::EqApo {
                ui.add_space(4.0);
                if widgets::ghost(ui, s.peq_apo_off).clicked() {
                    match epos_render_word().map(|w| eposctl::eqapo::write(None, &w)) {
                        Some(Ok(_)) => self.peq_msg = None,
                        Some(Err(e)) => self.peq_msg = Some((s.io_failed.replace("{err}", &e.to_string()), false)),
                        None => {}
                    }
                    cfg.output = PeqOutput::Epos;
                }
            }
            if let Some((m, ok)) = &self.peq_msg {
                ui.label(RichText::new(m).font(theme::font(12.5)).color(if *ok { ACCENT } else { DANGER }));
            }
        });

        let switched = cfg.output != old_output;
        if changed.curve || changed.settled || switched {
            self.cfg.peq = cfg.clone();
            self.mark_dirty();
        }
        match cfg.output {
            PeqOutput::Epos if changed.curve || switched => {
                let g = cfg.peq.to_epos().gains;
                self.apply(|st| st.set_speaker_eq(&g));
            }
            PeqOutput::EqApo if changed.settled || switched => {
                let res = epos_render_word()
                    .ok_or_else(|| "no EPOS playback device".to_string())
                    .and_then(|w| eposctl::eqapo::write(Some(&cfg.peq), &w).map_err(|e| e.to_string()));
                match res {
                    Ok(_) => {
                        self.peq_msg = None;
                        // Otherwise the headset's own EQ would apply on top.
                        self.apply(|st| st.set_speaker_eq(&[0.0; BANDS]));
                    }
                    Err(e) => self.peq_msg = Some((s.io_failed.replace("{err}", &e), false)),
                }
            }
            _ => {}
        }
    }

    fn eq_msg_line(&self, ui: &mut Ui, mic: bool) {
        if let Some((m, msg, ok)) = &self.eq_msg
            && *m == mic
        {
            ui.label(RichText::new(msg).font(theme::font(12.5)).color(if *ok { ACCENT } else { DANGER }));
            ui.add_space(4.0);
        }
    }

    // ---- per-program presets ----

    /// Presets a rule can point at, as (reference, label). Saved presets are
    /// referenced by name so deleting one does not shift the others.
    fn preset_choices(&self) -> Vec<(String, String)> {
        let s = self.s();
        let mut v: Vec<(String, String)> =
            factory().map(|p| (p.name.to_owned(), factory_label(s, p.name).to_owned())).collect();
        for p in &self.cfg.custom_presets {
            let key = format!("custom:{}", p.name);
            if !v.iter().any(|(k, _)| *k == key) {
                v.push((key, p.name.clone()));
            }
        }
        v
    }

    fn apply_preset_ref(&mut self, r: &str) {
        match r.strip_prefix("custom:") {
            Some(name) => {
                if let Some(i) = self.cfg.custom_presets.iter().position(|p| p.name == name) {
                    self.apply_preset_id(&format!("custom{i}"));
                }
            }
            None => self.apply_preset_id(r),
        }
    }

    /// Apply the preset for the program in front, once it has stayed there a moment.
    /// Only a change of *target preset* applies anything, so hand-made changes
    /// survive switching between programs that share a preset.
    fn auto_switch(&mut self, ctx: &egui::Context) {
        if !self.cfg.auto.enabled || self.apo.is_none() {
            return;
        }
        let Some((exe, since)) = self.fg.lock().unwrap().current.clone() else { return };
        if self.auto_app.as_deref() == Some(exe.as_str()) {
            return;
        }
        let wait = foreground::SETTLE.saturating_sub(since.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        let auto = &self.cfg.auto;
        let target = auto.rules.iter().find(|r| r.exe == exe).map(|r| r.preset.clone()).or_else(|| auto.default.clone());
        self.auto_app = Some(exe);
        if target != self.auto_target {
            self.auto_target = target.clone();
            if let Some(r) = target {
                self.apply_preset_ref(&r);
            }
        }
    }

    fn auto_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        let choices = self.preset_choices();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut on = self.cfg.auto.enabled;
            if Self::setting_row(ui, s.auto_title, s.auto_desc, &mut on, true) {
                self.cfg.auto.enabled = on;
                // Take effect from the next switch rather than changing the sound right now.
                self.auto_app = self.fg.lock().unwrap().current.as_ref().map(|(e, _)| e.clone());
                self.auto_target = None;
                self.mark_dirty();
            }
            if !self.cfg.auto.enabled {
                return;
            }
            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(4.0);

            let row = |ui: &mut Ui, label: &str, right: &mut dyn FnMut(&mut Ui)| {
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.label(RichText::new(label).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| right(ui));
                });
            };

            let mut default = self.cfg.auto.default.clone();
            let mut default_changed = false;
            row(ui, s.auto_other, &mut |ui| {
                default_changed =
                    preset_combo(ui, "auto-default", &mut default, &choices, Some(s.auto_keep), s.missing_preset);
            });
            if default_changed {
                self.cfg.auto.default = default;
                self.mark_dirty();
            }

            let rules = self.cfg.auto.rules.clone();
            let (mut remove, mut set) = (None, None);
            for (i, rule) in rules.iter().enumerate() {
                row(ui, &rule.exe, &mut |ui| {
                    if widgets::ghost(ui, s.delete).clicked() {
                        remove = Some(i);
                    }
                    let mut v = Some(rule.preset.clone());
                    if preset_combo(ui, ("auto-rule", i), &mut v, &choices, None, s.missing_preset)
                        && let Some(v) = v
                    {
                        set = Some((i, v));
                    }
                });
            }
            if rules.is_empty() {
                ui.add_space(4.0);
                widgets::desc(ui, s.auto_empty);
            }

            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(10.0);
            let mut add: Option<String> = None;
            let recent: Vec<String> = self
                .fg
                .lock()
                .unwrap()
                .recent
                .iter()
                .filter(|e| !rules.iter().any(|r| r.exe == **e))
                .cloned()
                .collect();
            if !recent.is_empty() {
                widgets::desc(ui, s.auto_recent);
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for e in &recent {
                        if widgets::ghost(ui, &format!("+  {e}")).clicked() {
                            add = Some(e.clone());
                        }
                    }
                });
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                let te = ui.add(
                    egui::TextEdit::singleline(&mut self.auto_input)
                        .hint_text(s.auto_exe_hint)
                        .desired_width(240.0)
                        .margin(Margin::symmetric(10, 7)),
                );
                let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (widgets::primary(ui, s.add).clicked() || enter)
                    && let Some(exe) = exe_name(&self.auto_input)
                {
                    add = Some(exe);
                    self.auto_input.clear();
                }
            });

            let rules = &mut self.cfg.auto.rules;
            let mut dirty = false;
            if let Some((i, v)) = set {
                rules[i].preset = v;
                dirty = true;
            }
            if let Some(i) = remove {
                rules.remove(i);
                dirty = true;
            }
            if let Some(exe) = add
                && !rules.iter().any(|r| r.exe == exe)
            {
                let preset = choices.first().map(|c| c.0.clone()).unwrap_or_default();
                rules.push(AppRule { exe, preset });
                dirty = true;
            }
            if dirty {
                // Re-evaluate on the next switch with the new rules.
                self.auto_target = None;
                self.mark_dirty();
            }
        });
    }

    // ---- chrome ----

    fn nav(&mut self, ui: &mut Ui) {
        let s = self.s();
        ui.add_space(18.0);
        // Brand mark: the same ring as the tray icon.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let c = r.center();
        ui.painter().circle_stroke(c, 13.0, Stroke::new(3.0, ACCENT));
        ui.painter().circle_filled(c, 4.0, ACCENT);
        ui.add_space(22.0);

        for (page, icon, label) in [
            (Page::Sound, Icon::Sound, s.nav_sound),
            (Page::Mic, Icon::Mic, s.nav_mic),
            (Page::Device, Icon::Device, s.nav_device),
            (Page::Settings, Icon::Settings, s.nav_settings),
        ] {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
            if resp.clicked() {
                self.page = page;
                self.naming = None;
                self.recording = None;
                self.stop_compare();
            }
            let sel = self.page == page;
            let color = if sel {
                ACCENT
            } else if resp.hovered() {
                TEXT
            } else {
                SECONDARY
            };
            let p = ui.painter();
            if sel {
                p.rect_filled(rect.shrink2(vec2(10.0, 3.0)), CornerRadius::same(10), RAISED);
                p.rect_filled(
                    egui::Rect::from_min_size(rect.left_top() + vec2(0.0, 18.0), vec2(3.0, rect.height() - 36.0)),
                    CornerRadius::same(2),
                    ACCENT,
                );
            }
            icons::paint(p, egui::Rect::from_center_size(rect.center() - vec2(0.0, 9.0), vec2(22.0, 22.0)), icon, color);
            p.text(rect.center() + vec2(0.0, 15.0), egui::Align2::CENTER_CENTER, label, theme::font(12.0), color);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.add_space(4.0);
        }
    }

    fn header(&self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        ui.horizontal(|ui| {
            let name = d.product.clone().unwrap_or_else(|| s.headset.to_owned());
            ui.label(RichText::new(name).font(theme::font_strong(24.0)).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                match (d.dongle, d.linked, d.battery) {
                    (false, _, _) => {
                        ui.label(RichText::new(s.dongle_missing).color(SECONDARY));
                        widgets::dot(ui, FAINT);
                    }
                    (true, Some(true), Some(b)) => {
                        ui.label(RichText::new(format!("{b}%")).font(theme::font_strong(15.0)).color(TEXT));
                        widgets::battery(ui, b);
                        ui.add_space(10.0);
                        ui.label(RichText::new(s.linked).color(SECONDARY));
                        widgets::dot(ui, ACCENT);
                    }
                    (true, Some(true), None) => {
                        ui.label(RichText::new(s.linked).color(SECONDARY));
                        widgets::dot(ui, ACCENT);
                    }
                    (true, _, _) => {
                        ui.label(RichText::new(s.not_linked).color(SECONDARY));
                        widgets::dot(ui, FAINT);
                    }
                }
            });
        });
    }

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
                let w = 14.0 + 8.0 + ui.painter().layout_no_wrap(s.apo_retrying.into(), theme::font(13.0), FAINT).size().x;
                ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                widgets::spinner(ui);
                ui.label(RichText::new(s.apo_retrying).font(theme::font(13.0)).color(FAINT));
            });
        });
    }

    // ---- pages ----

    fn page_sound(&mut self, ui: &mut Ui) {
        let s = self.s();
        let Some(cur) = self.s.clone() else {
            self.apo_missing(ui);
            // Without the EPOS driver, Equalizer APO can still do the EQ.
            if self.eqapo {
                ui.add_space(12.0);
                self.peq_panel(ui);
            }
            return;
        };
        let sound = sound_of(&cur);

        // Presets.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let mut chosen = None;
            let mut first_game = true;
            for p in factory() {
                let game = is_game(p);
                if game && std::mem::take(&mut first_game) {
                    ui.add_space(6.0);
                }
                let on = matches(&sound, &p.speaker_eq, p.reverb, p.surround);
                let mut r = widgets::chip(ui, factory_label(s, p.name), on);
                if game {
                    r = r.on_hover_text(s.preset_game_tip);
                }
                if r.clicked() {
                    chosen = Some(p.name.to_owned());
                }
            }
            let mut delete = None;
            let mut overwrite = None;
            for (i, p) in self.cfg.custom_presets.iter().enumerate() {
                let on = matches(&sound, &p.sound.speaker_eq, p.sound.reverb, p.sound.surround);
                let r = widgets::chip(ui, &p.name, on);
                if r.clicked() {
                    chosen = Some(format!("custom{i}"));
                }
                r.context_menu(|ui| {
                    if ui.button(s.overwrite).clicked() {
                        overwrite = Some(i);
                        ui.close();
                    }
                    if ui.button(RichText::new(s.delete).color(DANGER)).clicked() {
                        delete = Some(i);
                        ui.close();
                    }
                });
            }
            ui.add_space(4.0);
            match &mut self.naming {
                None => {
                    if widgets::ghost(ui, &format!("+  {}", s.save_preset)).clicked() {
                        self.naming = Some(String::new());
                    }
                }
                Some(name) => {
                    let te = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text(s.preset_name_hint)
                            .desired_width(150.0)
                            .margin(Margin::symmetric(10, 7)),
                    );
                    if !te.has_focus() && name.is_empty() && !te.lost_focus() {
                        te.request_focus();
                    }
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    let save = widgets::primary(ui, s.save).clicked() || enter;
                    let cancel = widgets::ghost(ui, s.cancel).clicked() || esc;
                    let trimmed = name.trim().to_owned();
                    if save && !trimmed.is_empty() {
                        self.cfg.custom_presets.push(CustomPreset { name: trimmed, sound: sound.clone() });
                        self.naming = None;
                        self.after_presets_changed();
                    } else if cancel {
                        self.naming = None;
                    }
                }
            }
            if let Some(i) = overwrite {
                self.cfg.custom_presets[i].sound = sound.clone();
                self.mark_dirty();
            }
            if let Some(i) = delete {
                self.cfg.custom_presets.remove(i);
                self.after_presets_changed();
            }
            if let Some(id) = chosen {
                self.apply_preset_id(&id);
            }
        });
        ui.add_space(16.0);

        // Equalizer.
        widgets::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(ui, s.equalizer);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let comparing = self.comparing.is_some();
                    if widgets::ghost_toggle(ui, s.reset, !comparing, false).clicked() {
                        self.apply(|st| st.set_speaker_eq(&[0.0; BANDS]));
                    }
                    if widgets::ghost_toggle(ui, s.import, !comparing, false).on_hover_text(s.eq_import_tip).clicked() {
                        self.import_eq(false);
                    }
                    if widgets::ghost_toggle(ui, s.peq_button, !comparing, self.peq_open).clicked() {
                        self.peq_open = !self.peq_open;
                        if self.peq_open {
                            self.eqapo = eposctl::eqapo::config_dir().is_some();
                        }
                    }
                    let can_undo = !self.undo.is_empty() && !comparing;
                    if widgets::ghost_toggle(ui, s.undo, can_undo, false).on_hover_text(s.undo_tip).clicked() {
                        self.undo();
                    }
                    let differs = self
                        .ab_ref
                        .as_ref()
                        .is_some_and(|r| !matches(&sound, &r.speaker_eq, r.reverb, r.surround));
                    let label = if comparing { s.comparing } else { s.compare };
                    let r = widgets::ghost_toggle(ui, label, comparing || differs, comparing)
                        .on_hover_text(s.compare_tip);
                    if r.clicked() {
                        if comparing {
                            self.stop_compare();
                        } else {
                            self.start_compare();
                        }
                    }
                });
            });
            ui.add_space(6.0);
            let editable = self.comparing.is_none();
            ui.add_enabled_ui(editable, |ui| {
                let mut g = self.s.as_ref().map(|x| x.speaker_eq()).unwrap_or_default();
                if eq::editor(ui, "speaker", &mut g, 230.0, None) && editable {
                    self.apply(|st| st.set_speaker_eq(&g));
                }
            });
            self.eq_msg_line(ui, false);
            widgets::desc(ui, s.eq_hint);
        });
        ui.add_space(12.0);
        if self.peq_open && self.comparing.is_none() {
            self.peq_panel(ui);
            ui.add_space(12.0);
        }

        // Surround, with reverb beneath it because it only works with surround on.
        let editable = self.comparing.is_none();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !editable {
                ui.disable();
            }
            let mut on = sound.surround;
            if Self::setting_row(ui, s.surround, s.surround_desc, &mut on, true) {
                self.apply(|st| st.set_surround(on));
            }
            ui.add_space(10.0);
            widgets::divider(ui);
            ui.add_space(10.0);
            let enabled = sound.surround;
            let value = if enabled { format!("{:.0}%", sound.reverb * 100.0) } else { s.reverb_off_hint.to_owned() };
            widgets::row_title(ui, s.reverb, Some(value), enabled);
            let mut v = sound.reverb;
            if widgets::slider(ui, &mut v, enabled).changed() {
                self.apply(|st| st.set_reverb(v));
            }
        });
        ui.add_space(12.0);
        self.mix_panel(ui);
    }

    fn nudge_mix(&mut self, by: f32) {
        let m = &mut self.cfg.chat_mix;
        if m.enabled {
            m.balance = ((m.balance + by) * 10.0).round().clamp(-10.0, 10.0) / 10.0;
            self.mark_dirty();
        }
    }

    fn mix_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut on = self.cfg.chat_mix.enabled;
            if Self::setting_row(ui, s.mix_title, s.mix_desc, &mut on, true) {
                self.cfg.chat_mix.enabled = on;
                self.mark_dirty();
            }
            if !self.cfg.chat_mix.enabled {
                return;
            }
            ui.add_space(10.0);
            let b = self.cfg.chat_mix.balance;
            let value = if b.abs() < 0.005 {
                s.mix_center.to_owned()
            } else {
                let side = if b > 0.0 { s.mix_game } else { s.mix_voice };
                s.mix_lowered.replace("{side}", side).replace("{pct}", &format!("{:.0}", b.abs() * 100.0))
            };
            widgets::row_title(ui, "", Some(value), true);
            let mut changed = None;
            ui.horizontal(|ui| {
                ui.label(RichText::new(s.mix_game).color(SECONDARY));
                let label_w = 48.0;
                ui.allocate_ui(vec2((ui.available_width() - label_w).max(80.0), 26.0), |ui| {
                    let mut v = (b + 1.0) / 2.0;
                    let r = widgets::slider(ui, &mut v, true);
                    if r.double_clicked() {
                        changed = Some(0.0);
                    } else if r.changed() {
                        // Snap to the centre near the middle so "both full" is easy to hit.
                        let nb = v * 2.0 - 1.0;
                        changed = Some(if nb.abs() < 0.04 { 0.0 } else { nb });
                    }
                });
                ui.label(RichText::new(s.mix_voice).color(SECONDARY));
            });
            if let Some(nb) = changed {
                self.cfg.chat_mix.balance = nb;
                self.mark_dirty();
            }

            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(8.0);
            widgets::desc(ui, s.mix_apps);
            ui.add_space(6.0);
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, app) in self.cfg.chat_mix.voice_apps.iter().enumerate() {
                    if widgets::ghost(ui, &format!("{app}  ×")).clicked() {
                        remove = Some(i);
                    }
                }
            });
            let mut add = None;
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let te = ui.add(
                    egui::TextEdit::singleline(&mut self.mix_input)
                        .hint_text(s.mix_app_hint)
                        .desired_width(240.0)
                        .margin(Margin::symmetric(10, 7)),
                );
                let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (widgets::primary(ui, s.add).clicked() || enter)
                    && let Some(exe) = exe_name(&self.mix_input)
                {
                    add = Some(exe);
                    self.mix_input.clear();
                }
            });
            let apps = &mut self.cfg.chat_mix.voice_apps;
            let mut dirty = false;
            if let Some(i) = remove {
                apps.remove(i);
                dirty = true;
            }
            if let Some(exe) = add
                && !apps.contains(&exe)
            {
                apps.push(exe);
                dirty = true;
            }
            if dirty {
                self.mark_dirty();
            }
        });
    }

    fn after_presets_changed(&mut self) {
        self.mark_dirty();
        let (s, presets) = (self.s(), self.tray_presets());
        if let Some(t) = &mut self.tray {
            t.set_menu(s, &presets);
        }
    }

    fn page_mic(&mut self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        let Some(cur) = self.s.clone() else { return self.apo_missing(ui) };

        ui.columns(2, |cols| {
            let (a, b) = cols.split_at_mut(1);
            widgets::panel().show(&mut a[0], |ui| {
                let gate = cur.noise_gate();
                widgets::row_title(ui, s.noise_gate, Some(format!("{:.0}%", gate * 100.0)), true);
                widgets::desc(ui, s.noise_gate_desc);
                ui.add_space(6.0);
                let mut v = gate;
                if widgets::slider(ui, &mut v, true).changed() {
                    self.apply(|st| st.set_noise_gate(v));
                }
                if self.monitors.is_some() {
                    ui.add_space(2.0);
                    widgets::level_meter(ui, s.input_level, self.meters.level, self.meters.peak, eq::METER_FLOOR_DB);
                }
            });
            widgets::panel().show(&mut b[0], |ui| {
                let linked = d.linked == Some(true) && d.sidetone.is_some();
                let pct = self
                    .sidetone_drag
                    .filter(|(_, t)| t.elapsed() < Duration::from_millis(1500))
                    .map(|(v, _)| v)
                    .or(d.sidetone.map(|raw| (255.0 - raw.min(255) as f32) / 255.0))
                    .unwrap_or(0.0);
                let value = linked.then(|| format!("{:.0}%", pct * 100.0));
                widgets::row_title(ui, s.sidetone, value, linked);
                widgets::desc(ui, if linked { s.sidetone_desc } else { s.sidetone_unavailable });
                ui.add_space(6.0);
                let mut v = pct;
                let r = widgets::slider(ui, &mut v, linked);
                if r.changed() {
                    self.sidetone_drag = Some((v, Instant::now()));
                    let raw = (255.0 * (1.0 - v)).round() as u16;
                    let _ = self.tx.send(Cmd::SetSidetone(raw));
                }
                if r.dragged()
                    && let Some((_, t)) = &mut self.sidetone_drag
                {
                    *t = Instant::now();
                }
                if self.monitors.is_some() {
                    // Same footprint as the level meter in the noise-gate panel,
                    // so both panels have the same height.
                    ui.add_space(2.0);
                    ui.allocate_exact_size(vec2(ui.available_width(), widgets::LEVEL_ROW_H), Sense::hover());
                }
            });
        });
        ui.add_space(12.0);

        // Hardware noise reduction (HID 0A/11); shown only when the headset reports a level.
        if let Some(level) = d.noise_reduction.filter(|_| d.linked == Some(true)) {
            widgets::panel().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        widgets::row_title(ui, s.noise_reduction, None, true);
                        widgets::desc(ui, s.noise_reduction_desc);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for (l, label) in [(2, s.nr_strong), (1, s.nr_medium), (0, s.nr_off)] {
                            if widgets::chip(ui, label, level == l).clicked() && level != l {
                                let _ = self.tx.send(Cmd::SetNoiseReduction(l));
                            }
                        }
                    });
                });
            });
            ui.add_space(12.0);
        }

        widgets::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(ui, s.mic_eq);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::ghost(ui, s.reset).clicked() {
                        self.apply(|st| st.set_mic_eq(&[0.0; BANDS]));
                    }
                    if widgets::ghost(ui, s.import).on_hover_text(s.eq_import_tip).clicked() {
                        self.import_eq(true);
                    }
                });
            });
            ui.add_space(6.0);
            let mut g = self.s.as_ref().map(|x| x.mic_eq()).unwrap_or_default();
            let bands = self.meters.bands;
            let levels = self.monitors.is_some().then_some(&bands[..]);
            if eq::editor(ui, "mic", &mut g, 230.0, levels) {
                self.apply(|st| st.set_mic_eq(&g));
            }
            self.eq_msg_line(ui, true);
            widgets::desc(ui, if levels.is_some() { s.mic_eq_hint } else { s.eq_hint });
        });
    }

    fn page_device(&mut self, ui: &mut Ui, d: &DevState) {
        let s = self.s();
        let dash = s.unknown;
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let link = match (d.dongle, d.linked) {
                (false, _) => (s.dongle_missing, SECONDARY),
                (true, Some(true)) => (s.linked, ACCENT),
                _ => (s.not_linked, SECONDARY),
            };
            widgets::info_row(ui, s.link, link.0, link.1);
            widgets::divider(ui);
            widgets::info_row(ui, s.product, d.product.as_deref().unwrap_or(dash), TEXT);
            widgets::divider(ui);
            widgets::info_row(ui, s.serial, d.serial.as_deref().unwrap_or(dash), TEXT);
            widgets::divider(ui);
            widgets::info_row(ui, s.firmware, d.firmware.as_deref().unwrap_or(dash), TEXT);
            widgets::divider(ui);
            let bat = match d.battery {
                Some(b) => match self.battery_left(b) {
                    Some(left) => format!("{left}  ·  {b}%"),
                    None => format!("{b}%"),
                },
                None => dash.to_owned(),
            };
            let bat_color = match d.battery {
                Some(0..=10) => DANGER,
                Some(11..=20) => WARN,
                _ => TEXT,
            };
            widgets::info_row(ui, s.battery, &bat, bat_color);
            widgets::divider(ui);
            let (anc, anc_c) = match d.anc {
                Some(true) => (s.anc_on, ACCENT),
                Some(false) => (s.anc_off, TEXT),
                None => (s.anc_unknown, FAINT),
            };
            widgets::info_row(ui, s.anc, anc, anc_c);
            widgets::divider(ui);
            let apo = self.apo.as_ref().map(|c| c.device.to_string());
            widgets::info_row(
                ui,
                s.apo_channel,
                apo.as_deref().unwrap_or(s.apo_missing_short),
                if apo.is_some() { TEXT } else { SECONDARY },
            );
        });
        ui.add_space(12.0);
        if widgets::ghost(ui, s.refresh).clicked() {
            let _ = self.tx.send(Cmd::Refresh);
        }
    }

    fn setting_row(ui: &mut Ui, title: &str, desc: &str, on: &mut bool, strong: bool) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 70.0);
                if strong {
                    widgets::heading(ui, title);
                } else {
                    ui.label(RichText::new(title).color(TEXT));
                }
                widgets::desc(ui, desc);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                changed = widgets::switch(ui, on, true).changed();
            });
        });
        changed
    }

    fn page_settings(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(s.language).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Right-to-left: English first so 中文 ends up on the left.
                    for (lang, label) in [(Lang::En, "English"), (Lang::Zh, "中文")] {
                        if widgets::chip(ui, label, self.cfg.lang == lang).clicked() && self.cfg.lang != lang {
                            self.cfg.lang = lang;
                            self.after_presets_changed();
                        }
                    }
                });
            });
            ui.add_space(4.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let mut v = self.cfg.close_to_tray;
            if Self::setting_row(ui, s.close_to_tray, s.close_to_tray_desc, &mut v, false) {
                self.cfg.close_to_tray = v;
                self.mark_dirty();
            }
            ui.add_space(4.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let mut v = self.autostart;
            if Self::setting_row(ui, s.autostart, s.autostart_desc, &mut v, false) && config::autostart::set(v).is_ok() {
                self.autostart = v;
            }
            ui.add_space(4.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let mut v = self.cfg.restore_on_start;
            if Self::setting_row(ui, s.restore, s.restore_desc, &mut v, false) {
                self.cfg.restore_on_start = v;
                self.mark_dirty();
            }
            ui.add_space(4.0);
            widgets::divider(ui);
            ui.add_space(4.0);
            let mut v = self.cfg.battery_alerts;
            if Self::setting_row(ui, s.battery_alerts, s.battery_alerts_desc, &mut v, false) {
                self.cfg.battery_alerts = v;
                self.mark_dirty();
            }
        });
        ui.add_space(12.0);
        self.auto_panel(ui);
        ui.add_space(12.0);
        self.hotkeys_panel(ui);
        ui.add_space(12.0);
        self.devswitch_panel(ui);
        ui.add_space(12.0);
        self.presets_io_panel(ui);
        ui.add_space(12.0);
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::heading(ui, s.about);
                ui.label(RichText::new(format!("eposctl {}", env!("CARGO_PKG_VERSION"))).color(SECONDARY));
            });
            ui.add_space(2.0);
            ui.label(RichText::new(s.about_body).color(SECONDARY));
        });
    }

    /// Notify once per threshold as the battery drains; re-arm after charging.
    fn check_battery(&mut self, name: &str, percent: u8) {
        const THRESHOLDS: [u8; 3] = [5, 10, 20];
        if percent > THRESHOLDS[2] + 5 {
            self.battery_warned = None;
            return;
        }
        let Some(level) = THRESHOLDS.into_iter().find(|&t| percent <= t) else { return };
        if self.battery_warned.is_some_and(|w| level >= w) {
            return;
        }
        self.battery_warned = Some(level);
        if self.cfg.battery_alerts {
            let s = self.s();
            let body = s.low_battery_body.replace("{name}", name).replace("{pct}", &percent.to_string());
            let _ = notify::toast(s.low_battery_title, &body);
        }
    }

    fn update_tray(&mut self) {
        let s = self.s();
        let d = self.dev.lock().unwrap().clone();
        let linked = d.linked == Some(true);
        let name = d.product.clone().unwrap_or_else(|| s.headset.to_owned());
        let tip = match (linked, d.battery) {
            (true, Some(b)) => match self.battery_left(b) {
                Some(left) => format!("{name}\n{} {b}% ({left})", s.tray_battery),
                None => format!("{name}\n{} {b}%", s.tray_battery),
            },
            (true, None) => name,
            _ => format!("{name}\n{}", s.tray_no_headset),
        };
        self.follow_headset(d.linked);
        if let Some(rate) = self.battery.observe(if linked { d.battery } else { None }, Instant::now()) {
            self.cfg.battery_rate = Some(rate);
            self.mark_dirty();
        }
        if linked && let Some(b) = d.battery {
            let name = d.product.clone().unwrap_or_else(|| s.headset.to_owned());
            self.check_battery(&name, b);
        }
        let surround = self.s.as_ref().map(|s| s.surround());
        if let Some(t) = &mut self.tray {
            t.update(if linked { d.battery } else { None }, linked, tip);
            t.set_surround(surround);
        }
    }
}

impl eframe::App for App {
    /// Runs even while the window is hidden, whenever something wakes the UI.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.flags.quit_requested() && !self.quitting {
            self.quitting = true;
            self.stop_compare();
            self.mixer.shutdown();
            self.set_monitoring(false);
            self.cfg.save();
            self.tray = None; // remove the icon now rather than leaving a ghost
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.hide_frames > 0 && self.tray.is_some() {
            self.hide_frames -= 1;
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
        if self.flags.take_show() {
            self.hide_frames = 0;
            self.hidden = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.flags.take_toggle_surround() {
            self.toggle_surround();
        }
        if let Some(id) = self.flags.take_preset() {
            self.ensure_apo();
            self.apply_preset_id(&id);
        }
        if self.recording.is_some() && !ctx.input(|i| i.focused) {
            self.recording = None;
        }
        self.sync_hotkeys();
        self.mixer.set(&self.cfg.chat_mix);
        for a in self.hotkeys.as_ref().map(|h| h.take_fired()).unwrap_or_default() {
            match a {
                Action::NextPreset => self.step_preset(1),
                Action::PrevPreset => self.step_preset(-1),
                Action::ToggleSurround => self.toggle_surround(),
                Action::MixVoice => self.nudge_mix(0.2),
                Action::MixGame => self.nudge_mix(-0.2),
            }
        }

        self.ensure_apo();
        self.sync(ctx);
        self.auto_switch(ctx);
        self.update_tray();

        let shown = !self.hidden && !ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let metering = self.page == Page::Mic && shown && !self.quitting;
        self.set_monitoring(metering);
        if metering && self.monitors.is_some() {
            if let Some(m) = self.apo.as_ref().and_then(|c| c.read().ok()).map(|s| s.meters()) {
                let dt = ctx.input(|i| i.stable_dt).min(0.1);
                self.meters.update(&m[..BANDS], dt);
            }
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        if self.cfg_dirty_at.is_some_and(|t| t.elapsed() > Duration::from_millis(800)) {
            self.cfg_dirty_at = None;
            self.cfg.save();
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.cfg.close_to_tray && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.stop_compare();
                self.hidden = true;
                self.set_monitoring(false);
            } else {
                self.cfg.save();
            }
        }
        // Keep polling the driver and config at a relaxed pace.
        ctx.request_repaint_after(Duration::from_millis(1000));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let d = self.dev.lock().unwrap().clone();
        #[cfg(debug_assertions)]
        debug_screenshot(ui.ctx());

        // A drag (EQ point, slider) is one undo step however long it lasts.
        if ui.ctx().dragged_id().is_some() && self.group_until.is_some() {
            self.group_until = Some(Instant::now() + UNDO_GROUP);
        }
        self.record_hotkey(ui);
        if !ui.ctx().egui_wants_keyboard_input() {
            use egui::{Key, Modifiers};
            let (redo_shift, undo, redo) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                    i.consume_key(Modifiers::COMMAND, Key::Z),
                    i.consume_key(Modifiers::COMMAND, Key::Y),
                )
            });
            if undo {
                self.undo();
            } else if redo || redo_shift {
                self.redo();
            }
        }

        egui::Panel::left("nav")
            .exact_size(88.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(PANEL))
            .show(ui, |ui| self.nav(ui));

        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 32, right: 32, top: 26, bottom: 20 }))
            .show(ui, |ui| {
                self.header(ui, &d);
                ui.add_space(16.0);
                let area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                // Debug builds: `EPOSCTL_SCROLL=<px>` scrolls the page (for screenshots).
                #[cfg(debug_assertions)]
                let area = match std::env::var("EPOSCTL_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
                    Some(y) => area.vertical_scroll_offset(y),
                    None => area,
                };
                area.show(ui, |ui| {
                    match self.page {
                        Page::Sound => self.page_sound(ui),
                        Page::Mic => self.page_mic(ui, &d),
                        Page::Device => self.page_device(ui, &d),
                        Page::Settings => self.page_settings(ui),
                    }
                    ui.add_space(8.0);
                });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_compare();
        self.mixer.shutdown();
        self.set_monitoring(false);
        self.cfg.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(0x17, 0x18, 0x1E).to_normalized_gamma_f32()
    }
}

/// Debug builds: `EPOSCTL_SHOT=file.png` saves a screenshot of the window once
/// it has settled, then exits. Used for the README images.
/// `EPOSCTL_SHOT_DELAY` (seconds, default 3) and `EPOSCTL_SHOT_HEIGHT` (logical
/// points from the top, default the whole window) are optional.
#[cfg(debug_assertions)]
fn debug_screenshot(ctx: &egui::Context) {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    static START: OnceLock<Instant> = OnceLock::new();
    static ASKED: AtomicBool = AtomicBool::new(false);
    let Some(path) = std::env::var_os("EPOSCTL_SHOT") else { return };
    let start = *START.get_or_init(Instant::now);
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let delay = env("EPOSCTL_SHOT_DELAY", 3.0);
    let height = env("EPOSCTL_SHOT_HEIGHT", f32::INFINITY);
    if start.elapsed().as_secs_f32() > delay && !ASKED.swap(true, Ordering::Relaxed) {
        let ppp = ctx.pixels_per_point();
        let screen = ctx.content_rect();
        let crop = egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, screen.max.y.min(height)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
            let img = img.region(&crop, Some(ppp));
            let file = std::fs::File::create(&path).expect("screenshot file");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width() as u32, img.height() as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            enc.write_header().and_then(|mut w| w.write_image_data(&bytes)).expect("write png");
            std::process::exit(0);
        })));
    }
    ctx.request_repaint();
}

/// The `Device:` word for Equalizer APO that selects the EPOS playback device.
fn epos_render_word() -> Option<String> {
    use eposctl::audio::{Flow, endpoints, is_epos};
    let ep = endpoints(Flow::Output).ok()?.into_iter().find(|e| is_epos(&e.name))?;
    eposctl::eqapo::device_word(&ep.id)
}

/// Named event shared by all instances. A second launch signals it and exits;
/// the running instance shows its window.
const SHOW_EVENT: &str = r"Local\eposctl-gui-show";

/// Returns the event handle if this is the first instance, `None` if another one was signalled.
fn single_instance() -> Option<isize> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};
    let name: Vec<u16> = SHOW_EVENT.encode_utf16().chain(Some(0)).collect();
    let h = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if h.is_null() {
        return Some(0); // can't coordinate; just run
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { SetEvent(h) };
        return None;
    }
    Some(h as isize)
}

fn watch_show_event(h: isize, flags: Arc<tray::Flags>, ctx: egui::Context) {
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    if h == 0 {
        return;
    }
    std::thread::spawn(move || {
        loop {
            if unsafe { WaitForSingleObject(h as _, INFINITE) } != 0 {
                return;
            }
            flags.show.store(true, std::sync::atomic::Ordering::Relaxed);
            ctx.request_repaint();
        }
    });
}

fn main() -> eframe::Result {
    let Some(show_event) = single_instance() else { return Ok(()) };
    let icon = egui::IconData { rgba: icons::bitmap(64, None, true), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("eposctl")
            .with_inner_size([1000.0, 780.0])
            .with_min_inner_size([820.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("eposctl", options, Box::new(move |cc| Ok(Box::new(App::new(cc, show_event)))))
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
