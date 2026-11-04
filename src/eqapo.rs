//! Output to Equalizer APO, for parametric EQs with more detail than the
//! headset's nine bands can show.
//!
//! eposctl writes its bands to `eposctl.txt` in Equalizer APO's config folder,
//! scoped with a `Device:` line to one playback device, and adds
//! `Include: eposctl.txt` to `config.txt` once. Equalizer APO itself has to be
//! installed on that device by the user (its Configurator); eposctl never
//! installs or registers anything.
//!
//! `Device:` patterns are words that must all occur in "device name, connection
//! name, GUID" (Equalizer APO's configuration reference), so the endpoint GUID
//! alone selects exactly one device.

use std::io;
use std::path::PathBuf;

use crate::peq::Peq;
use crate::win::{Hive, reg_string};

/// The file eposctl owns inside Equalizer APO's config folder.
pub const FILE: &str = "eposctl.txt";
const INCLUDE: &str = "Include: eposctl.txt";

/// Equalizer APO's config folder (`HKLM\SOFTWARE\EqualizerAPO\ConfigPath`), if
/// it is installed.
pub fn config_dir() -> Option<PathBuf> {
    let dir = reg_string(Hive::LocalMachine, r"SOFTWARE\EqualizerAPO", "ConfigPath")?;
    let dir = PathBuf::from(dir);
    dir.join("config.txt").is_file().then_some(dir)
}

/// The GUID word of a Windows endpoint id (`{0.0.0.00000000}.{guid}`), for a
/// `Device:` line.
pub fn device_word(endpoint_id: &str) -> Option<String> {
    let guid = endpoint_id.rsplit('.').next()?.trim_matches(|c| c == '{' || c == '}');
    (guid.len() == 36 && guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| guid.to_ascii_lowercase())
}

/// The text of `eposctl.txt`: `peq` on the device `device` only, or nothing.
pub fn file_text(peq: Option<&Peq>, device: &str) -> String {
    let mut s = String::from("# Written by eposctl; it is overwritten when eposctl changes the EQ.\r\n");
    if let Some(p) = peq {
        s.push_str(&format!("Device: {device}\r\n"));
        for line in p.to_apo_text().lines() {
            s.push_str(line);
            s.push_str("\r\n");
        }
        s.push_str("Device: all\r\n");
    }
    s
}

/// Write `peq` (or clear it with `None`) for the device `device` and make sure
/// `config.txt` includes it. Returns the path written.
pub fn write(peq: Option<&Peq>, device: &str) -> io::Result<PathBuf> {
    let dir = config_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Equalizer APO is not installed"))?;
    let path = dir.join(FILE);
    std::fs::write(&path, file_text(peq, device))?;
    let config = dir.join("config.txt");
    let text = std::fs::read_to_string(&config)?;
    if !text.lines().any(|l| l.trim().eq_ignore_ascii_case(INCLUDE)) {
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\r\n" };
        std::fs::write(&config, format!("{text}{sep}# Added by eposctl\r\n{INCLUDE}\r\n"))?;
    }
    Ok(path)
}

/// The bands eposctl last wrote, if any.
pub fn read() -> Option<Peq> {
    let text = std::fs::read_to_string(config_dir()?.join(FILE)).ok()?;
    Peq::from_apo_text(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peq::Band;

    #[test]
    fn device_word_takes_the_endpoint_guid() {
        assert_eq!(
            device_word("{0.0.0.00000000}.{6B5E2C1A-0D4F-4E8B-9C3A-1F2E3D4C5B6A}").as_deref(),
            Some("6b5e2c1a-0d4f-4e8b-9c3a-1f2e3d4c5b6a")
        );
        assert_eq!(device_word("speakers"), None);
    }

    #[test]
    fn file_is_scoped_to_the_device() {
        let peq = Peq { bands: vec![Band::peak(100.0, 3.0, 1.41)] };
        let t = file_text(Some(&peq), "abc");
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[1], "Device: abc");
        assert!(lines[2].starts_with("Preamp: -3"));
        assert!(lines[3].starts_with("Filter 1: ON PK Fc 100.0 Hz Gain 3.0 dB Q 1.410"));
        assert_eq!(*lines.last().unwrap(), "Device: all");
        assert_eq!(file_text(None, "abc").lines().count(), 1);
    }
}
