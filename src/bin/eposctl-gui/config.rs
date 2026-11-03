//! Persistent settings (`%APPDATA%\eposctl\config.json`) and the autostart entry.

use std::path::PathBuf;

use eposctl::apo::layout::BANDS;
use serde::{Deserialize, Serialize};

use crate::strings::Lang;

/// A sound setup: everything a preset can carry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    pub speaker_eq: [f32; BANDS],
    pub reverb: f32,
    pub surround: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomPreset {
    pub name: String,
    pub sound: Sound,
}

// ---- preset files ----

/// What `export_presets` writes. Import also accepts a bare list or a single preset.
#[derive(Serialize)]
struct PresetFile<'a> {
    eposctl_presets: u32,
    presets: &'a [CustomPreset],
}

pub fn export_presets(presets: &[CustomPreset]) -> String {
    serde_json::to_string_pretty(&PresetFile { eposctl_presets: 1, presets }).unwrap_or_default()
}

/// Parse a preset file, clamping values into the ranges the driver accepts.
pub fn parse_presets(json: &str) -> Result<Vec<CustomPreset>, String> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Any {
        File { presets: Vec<CustomPreset> },
        List(Vec<CustomPreset>),
        One(CustomPreset),
    }
    let any: Any = serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let list = match any {
        Any::File { presets } | Any::List(presets) => presets,
        Any::One(p) => vec![p],
    };
    let clean = |x: f32, lo: f32, hi: f32| if x.is_finite() { x.clamp(lo, hi) } else { 0.0 };
    Ok(list
        .into_iter()
        .filter_map(|mut p| {
            p.name = p.name.trim().chars().take(40).collect();
            for g in &mut p.sound.speaker_eq {
                *g = clean(*g, crate::eq::MIN_DB, crate::eq::MAX_DB);
            }
            p.sound.reverb = clean(p.sound.reverb, 0.0, 1.0);
            (!p.name.is_empty()).then_some(p)
        })
        .collect())
}

/// Append imported presets and return how many were added. Exact duplicates
/// are skipped; a name already used for a different sound gets a number.
pub fn merge_presets(into: &mut Vec<CustomPreset>, new: Vec<CustomPreset>) -> usize {
    let mut added = 0;
    for mut p in new {
        if into.iter().any(|q| q.name == p.name && q.sound == p.sound) {
            continue;
        }
        if into.iter().any(|q| q.name == p.name) {
            let base = p.name.clone();
            let mut n = 2;
            while into.iter().any(|q| q.name == format!("{base} ({n})")) {
                n += 1;
            }
            p.name = format!("{base} ({n})");
        }
        into.push(p);
        added += 1;
    }
    added
}

/// Last applied values, restored on start if enabled.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Last {
    pub speaker_eq: Option<[f32; BANDS]>,
    pub mic_eq: Option<[f32; BANDS]>,
    pub reverb: Option<f32>,
    pub noise_gate: Option<f32>,
    pub surround: Option<bool>,
}

/// A program and the preset to use while it is in the foreground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppRule {
    /// Lower-case executable name, e.g. `valorant-win64-shipping.exe`.
    pub exe: String,
    /// A factory preset name (`music`) or `custom:<name>` for a saved preset.
    pub preset: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoPresets {
    pub enabled: bool,
    /// Preset for programs without a rule; `None` leaves the sound as it is.
    pub default: Option<String>,
    pub rules: Vec<AppRule>,
}

/// Game/voice balance over the per-program volumes in the Windows mixer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatMix {
    pub enabled: bool,
    /// -1 = only game, 0 = both at full volume, 1 = only voice.
    pub balance: f32,
    /// Lower-case executable names of voice chat programs.
    pub voice_apps: Vec<String>,
}

impl Default for ChatMix {
    fn default() -> Self {
        let apps = [
            "discord.exe", "kook.exe", "yy.exe", "qq.exe", "weixin.exe", "wechat.exe", "teamspeak.exe",
            "ts3client_win64.exe", "mumble.exe", "skype.exe", "zoom.exe", "ms-teams.exe",
        ];
        Self { enabled: false, balance: 0.0, voice_apps: apps.map(String::from).to_vec() }
    }
}

/// Switch the Windows default devices when the headset is turned off and on.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceSwitch {
    pub enabled: bool,
    /// Endpoint ids to use while the headset is off; `None` leaves that direction alone.
    pub output: Option<String>,
    pub input: Option<String>,
}

/// Global hotkeys, stored like `Ctrl+Alt+PageDown`; `None` = not set.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub enabled: bool,
    pub next_preset: Option<String>,
    pub prev_preset: Option<String>,
    pub toggle_surround: Option<String>,
    pub mix_voice: Option<String>,
    pub mix_game: Option<String>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            enabled: false,
            next_preset: Some("Ctrl+Alt+PageDown".into()),
            prev_preset: Some("Ctrl+Alt+PageUp".into()),
            toggle_surround: Some("Ctrl+Alt+Home".into()),
            mix_voice: None,
            mix_game: None,
        }
    }
}

impl Hotkeys {
    /// Slot `i`, in the order of `hotkeys::Action::ALL`.
    pub fn slot(&mut self, i: usize) -> &mut Option<String> {
        match i {
            0 => &mut self.next_preset,
            1 => &mut self.prev_preset,
            2 => &mut self.toggle_surround,
            3 => &mut self.mix_voice,
            _ => &mut self.mix_game,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub lang: Lang,
    pub close_to_tray: bool,
    pub restore_on_start: bool,
    pub battery_alerts: bool,
    /// Last measured battery drain in percent per hour, for the runtime estimate.
    pub battery_rate: Option<f32>,
    pub custom_presets: Vec<CustomPreset>,
    pub auto: AutoPresets,
    pub hotkeys: Hotkeys,
    pub device_switch: DeviceSwitch,
    pub chat_mix: ChatMix,
    pub last: Last,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lang: Lang::Zh,
            close_to_tray: true,
            restore_on_start: true,
            battery_alerts: true,
            battery_rate: None,
            custom_presets: Vec::new(),
            auto: AutoPresets::default(),
            hotkeys: Hotkeys::default(),
            device_switch: DeviceSwitch::default(),
            chat_mix: ChatMix::default(),
            last: Last::default(),
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("eposctl").join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            // Write then rename so a crash never leaves a half-written file.
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
}

pub mod autostart {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "eposctl";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn command() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        Some(format!("\"{}\" --minimized", exe.display()))
    }

    pub fn is_enabled() -> bool {
        let (key, val) = (wide(RUN_KEY), wide(VALUE));
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if rc != 0 {
            return false;
        }
        let len = (size as usize / 2).saturating_sub(1);
        let current = String::from_utf16_lossy(&buf[..len]);
        // Only count it as ours if it points at this executable.
        command().is_some_and(|c| c.eq_ignore_ascii_case(&current))
    }

    pub fn set(enabled: bool) -> std::io::Result<()> {
        let (key, val) = (wide(RUN_KEY), wide(VALUE));
        let rc = if enabled {
            let cmd = wide(&command().ok_or_else(|| std::io::Error::other("cannot locate executable"))?);
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    val.as_ptr(),
                    REG_SZ,
                    cmd.as_ptr().cast(),
                    (cmd.len() * 2) as u32,
                )
            }
        } else {
            let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr()) };
            if rc == 2 { 0 } else { rc } // ERROR_FILE_NOT_FOUND: already off
        };
        if rc == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(rc as i32)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str, g: f32) -> CustomPreset {
        CustomPreset { name: name.into(), sound: Sound { speaker_eq: [g; BANDS], reverb: 0.5, surround: true } }
    }

    #[test]
    fn presets_round_trip_and_merge() {
        let mine = vec![preset("Game", 1.0), preset("Voice", -2.0)];
        let json = export_presets(&mine);
        let back = parse_presets(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].sound, mine[0].sound);

        let mut have = mine.clone();
        // Same name and sound: skipped. Same name, other sound: numbered.
        let added = merge_presets(&mut have, vec![preset("Game", 1.0), preset("Voice", 3.0), preset("New", 0.0)]);
        assert_eq!(added, 2);
        let names: Vec<_> = have.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Game", "Voice", "Voice (2)", "New"]);
    }

    #[test]
    fn import_accepts_list_and_single_and_clamps() {
        let one = r#"{"name":"  Loud ","sound":{"speaker_eq":[99,0,0,0,0,0,0,0,-99],"reverb":7,"surround":false}}"#;
        let p = parse_presets(one).unwrap();
        assert_eq!(p[0].name, "Loud");
        assert_eq!(p[0].sound.speaker_eq[0], crate::eq::MAX_DB);
        assert_eq!(p[0].sound.speaker_eq[8], crate::eq::MIN_DB);
        assert_eq!(p[0].sound.reverb, 1.0);
        assert_eq!(parse_presets(&format!("[{one},{one}]")).unwrap().len(), 2);
        assert!(parse_presets("{\"presets\": 3}").is_err());
        assert!(parse_presets(r#"[{"name":"","sound":{"speaker_eq":[0,0,0,0,0,0,0,0,0],"reverb":0,"surround":false}}]"#)
            .unwrap()
            .is_empty());
    }
}
