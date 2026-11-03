//! eposctl GUI: tray app with sound, microphone, device and settings pages.

#![windows_subsystem = "windows"]

mod config;
mod eq;
mod icons;
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

use config::{Config, CustomPreset, Last, Sound};
use icons::Icon;
use strings::{Lang, Strings, t};
use theme::{ACCENT, DANGER, FAINT, PANEL, RAISED, SECONDARY, TEXT, WARN};
use worker::{Cmd, DevState};

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
}

fn factory_label(s: &Strings, name: &str) -> &'static str {
    match name {
        "music" => s.preset_music,
        "esport" => s.preset_esport,
        "movie" => s.preset_movie,
        "flat" => s.preset_flat,
        _ => s.preset_custom,
    }
}

/// Factory presets in the order the official app shows them.
fn factory() -> impl Iterator<Item = &'static Preset> {
    ["music", "esport", "movie", "flat"].into_iter().filter_map(presets::find)
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.05)
}

fn sound_of(s: &Settings) -> Sound {
    Sound { speaker_eq: s.speaker_eq(), reverb: s.reverb(), surround: s.surround() }
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
            cfg,
        };
        // Debug builds: `EPOSCTL_PAGE=mic|device|settings` opens on that page (for screenshots).
        #[cfg(debug_assertions)]
        if let Ok(p) = std::env::var("EPOSCTL_PAGE") {
            app.page = match p.as_str() {
                "mic" => Page::Mic,
                "device" => Page::Device,
                "settings" => Page::Settings,
                _ => Page::Sound,
            };
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
    }

    fn apply(&mut self, f: impl FnOnce(&mut Settings)) {
        let Some(ch) = &self.apo else { return };
        match ch.modify(f) {
            Ok(s) => {
                self.cfg.last = Last {
                    speaker_eq: Some(s.speaker_eq()),
                    mic_eq: Some(s.mic_eq()),
                    reverb: Some(s.reverb()),
                    noise_gate: Some(s.noise_gate()),
                    surround: Some(s.surround()),
                };
                self.s = Some(s);
                self.mark_dirty();
            }
            Err(_) => self.drop_apo(),
        }
    }

    fn apply_preset_id(&mut self, id: &str) {
        if let Some(p) = presets::find(id) {
            self.apply(|s| p.apply(s));
        } else if let Some(c) = id.strip_prefix("custom").and_then(|i| i.parse::<usize>().ok())
            && let Some(p) = self.cfg.custom_presets.get(c).cloned() {
                self.apply(|s| {
                    s.set_speaker_eq(&p.sound.speaker_eq);
                    s.set_surround(p.sound.surround);
                    s.set_reverb(p.sound.reverb);
                });
            }
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
        let Some(cur) = self.s.clone() else { return self.apo_missing(ui) };
        let sound = sound_of(&cur);

        // Presets.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let mut chosen = None;
            for p in factory() {
                let on = matches(&sound, &p.speaker_eq, p.reverb, p.surround);
                if widgets::chip(ui, factory_label(s, p.name), on).clicked() {
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
                    if widgets::ghost(ui, s.reset).clicked() {
                        self.apply(|st| st.set_speaker_eq(&[0.0; BANDS]));
                    }
                });
            });
            ui.add_space(6.0);
            let mut g = self.s.as_ref().map(|x| x.speaker_eq()).unwrap_or_default();
            if eq::editor(ui, "speaker", &mut g, s.low, s.high, 230.0) {
                self.apply(|st| st.set_speaker_eq(&g));
            }
            widgets::desc(ui, s.eq_hint);
        });
        ui.add_space(12.0);

        // Surround, with reverb beneath it because it only works with surround on.
        widgets::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
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
    }

    fn after_presets_changed(&mut self) {
        self.mark_dirty();
        if let Some(t) = &self.tray {
            t.set_menu(self.s(), &self.tray_presets());
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
                    && let Some((_, t)) = &mut self.sidetone_drag {
                        *t = Instant::now();
                    }
            });
        });
        ui.add_space(12.0);

        widgets::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::heading(ui, s.mic_eq);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::ghost(ui, s.reset).clicked() {
                        self.apply(|st| st.set_mic_eq(&[0.0; BANDS]));
                    }
                });
            });
            ui.add_space(6.0);
            let mut g = self.s.as_ref().map(|x| x.mic_eq()).unwrap_or_default();
            if eq::editor(ui, "mic", &mut g, s.low, s.high, 230.0) {
                self.apply(|st| st.set_mic_eq(&g));
            }
            widgets::desc(ui, s.eq_hint);
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
            let bat = d.battery.map(|b| format!("{b}%")).unwrap_or_else(|| dash.to_owned());
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
        });
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

    fn update_tray(&mut self) {
        let s = self.s();
        let d = self.dev.lock().unwrap().clone();
        let linked = d.linked == Some(true);
        let name = d.product.clone().unwrap_or_else(|| s.headset.to_owned());
        let tip = match (linked, d.battery) {
            (true, Some(b)) => format!("{name}\n{} {b}%", s.tray_battery),
            (true, None) => name,
            _ => format!("{name}\n{}", s.tray_no_headset),
        };
        if let Some(t) = &mut self.tray {
            t.update(if linked { d.battery } else { None }, linked, tip);
        }
    }
}

impl eframe::App for App {
    /// Runs even while the window is hidden, whenever something wakes the UI.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.flags.quit_requested() && !self.quitting {
            self.quitting = true;
            self.cfg.save();
            self.tray = None; // remove the icon now rather than leaving a ghost
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.hide_frames > 0 && self.tray.is_some() {
            self.hide_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
        if self.flags.take_show() {
            self.hide_frames = 0;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if let Some(id) = self.flags.take_preset() {
            self.ensure_apo();
            self.apply_preset_id(&id);
        }

        self.ensure_apo();
        self.sync(ctx);
        self.update_tray();

        if self.cfg_dirty_at.is_some_and(|t| t.elapsed() > Duration::from_millis(800)) {
            self.cfg_dirty_at = None;
            self.cfg.save();
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.cfg.close_to_tray && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                self.cfg.save();
            }
        }
        // Keep polling the driver and config at a relaxed pace.
        ctx.request_repaint_after(Duration::from_millis(1000));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let d = self.dev.lock().unwrap().clone();

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
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
        self.cfg.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(0x17, 0x18, 0x1E).to_normalized_gamma_f32()
    }
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
