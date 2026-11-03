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

/// Last applied values, restored on start if enabled.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Last {
    pub speaker_eq: Option<[f32; BANDS]>,
    pub mic_eq: Option<[f32; BANDS]>,
    pub reverb: Option<f32>,
    pub noise_gate: Option<f32>,
    pub surround: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub lang: Lang,
    pub close_to_tray: bool,
    pub restore_on_start: bool,
    pub custom_presets: Vec<CustomPreset>,
    pub last: Last,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lang: Lang::Zh,
            close_to_tray: true,
            restore_on_start: true,
            custom_presets: Vec::new(),
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
