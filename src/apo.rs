//! Direct access to the EPOS APO settings channel.
//!
//! The EPOS audio processing object (`EAPO.dll`, loaded by `audiodg.exe` on EPOS
//! endpoints) receives its DSP settings through three named kernel objects:
//!
//! ```text
//! Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_memory   section (settings block)
//! Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_mutex    mutex guarding writes
//! Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_event    change notification
//! ```
//!
//! The original EPOS Gaming Suite service writes the block and signals the event.
//! We do the same, which removes the need for that service. See `docs/protocol.md`
//! for how each offset was established.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Memory::{
    FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW,
    UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{
    OpenEventW, OpenMutexW, ReleaseMutex, SetEvent, WaitForSingleObject,
};

use crate::objdir;

/// Fixed prefix of every EPOS APO kernel object name.
pub const OBJECT_GUID: &str = "CF4B411F-BE2B-4D84-8106-EC27CA0F8F05";

const SYNCHRONIZE: u32 = 0x0010_0000;
const MUTEX_MODIFY_STATE: u32 = 0x0001;
const EVENT_MODIFY_STATE: u32 = 0x0002;
const LOCK_TIMEOUT_MS: u32 = 2000;

/// Byte offsets inside the settings block (`epos::ipc::apo_settings::Data`).
///
/// "observed" = confirmed by diffing the block while changing the setting in the
/// official UI; "inferred" = consistent with observations but not isolated yet.
pub mod layout {
    /// u32, value 2 on every capture (observed, meaning unknown — likely a version).
    pub const HEADER: usize = 0x00;
    /// u8 bool, virtual 3D surround (observed). `EAPO.dll` names it `direct_sound_enabled`.
    pub const SURROUND_ENABLED: usize = 0x40;
    /// u8 bool, `speaker_eq_enabled` (name from `EAPO.dll`; always 1).
    pub const SPEAKER_EQ_ENABLED: usize = 0x41;
    /// u8 bool, `mic_eq_enabled`. The mic EQ gains have no effect while it is 0
    /// (measured: +6 dB on every band changed nothing with it off, +10 dB with it on).
    pub const MIC_EQ_ENABLED: usize = 0x42;
    /// u8 bool, `noise_gate_enabled`. The gate level has no effect while it is 0
    /// (measured: gate at 100 % let room noise through with it off, muted it with it on).
    pub const NOISE_GATE_ENABLED: usize = 0x44;
    /// u8 bool, `mic_meters_enabled`. The APO updates the meters at `METERS` only
    /// while it is set and a program is recording from the microphone (measured).
    pub const MIC_METERS_ENABLED: usize = 0x46;
    /// u8 bool, `reverb_enabled` (name from `EAPO.dll`); the official UI toggles it
    /// together with surround (observed).
    pub const REVERB_ENABLED: usize = 0x45;
    /// Fields named by the settings (de)serialiser in `EAPO.dll`
    /// (`0x1802e6650` / `0x1802e6c20`), with their width in bytes. Effects measured
    /// on the H3PRO are noted on the constants above and in docs/protocol.md.
    pub const NAMED: [(&str, usize, usize); 15] = [
        ("direct_sound_enabled", 0x40, 1),
        ("speaker_eq_enabled", 0x41, 1),
        ("mic_eq_enabled", 0x42, 1),
        ("mic_nr_enabled", 0x43, 1),
        ("noise_gate_enabled", 0x44, 1),
        ("reverb_enabled", 0x45, 1),
        ("mic_meters_enabled", 0x46, 1),
        ("mic_hpf_enabled", 0x47, 1),
        ("mic_eq_preset", 0x9C, 4),
        ("mic_nr_preset", 0xA0, 4),
        ("hrtf_preset", 0xA8, 4),
        ("mic_speech_compressor_enabled", 0xB1, 1),
        ("mic_speech_compressor_preset", 0xB4, 4),
        ("reverb_level", 0x94, 4),
        ("noise_gate_level", 0x98, 4),
    ];
    /// f32 x 9, speaker EQ gains in dB, -6..=6 (observed).
    pub const SPEAKER_EQ: usize = 0x4C;
    /// f32 x 9, microphone EQ gains in dB (observed for bands 1-8).
    pub const MIC_EQ: usize = 0x70;
    /// f32, reverb amount 0..=1 (observed; official UI writes slider/255).
    pub const REVERB_LEVEL: usize = 0x94;
    /// f32, noise gate amount 0..=1 (observed; official UI writes slider/255).
    pub const NOISE_GATE_LEVEL: usize = 0x98;
    /// f32 x 15, level meters in dB written back by the APO (observed, -120 = silence).
    pub const METERS: usize = 0x440;
    pub const METER_COUNT: usize = 15;
    pub const BANDS: usize = 9;
    /// Bytes we mirror.
    pub const LEN: usize = METERS + METER_COUNT * 4;
    /// We never write at or beyond this offset (the APO owns the meters).
    pub const WRITABLE_END: usize = METERS;
}

pub const EQ_MIN_DB: f32 = -6.0;
pub const EQ_MAX_DB: f32 = 6.0;

/// A copy of the settings block.
#[derive(Clone, PartialEq)]
pub struct Settings {
    bytes: [u8; layout::LEN],
}

impl Settings {
    pub fn from_bytes(bytes: [u8; layout::LEN]) -> Self {
        Self { bytes }
    }

    pub fn raw(&self) -> &[u8] {
        &self.bytes
    }

    fn u8_at(&self, off: usize) -> u8 {
        self.bytes[off]
    }

    fn f32_at(&self, off: usize) -> f32 {
        f32::from_le_bytes(self.bytes[off..off + 4].try_into().unwrap())
    }

    fn set_f32(&mut self, off: usize, v: f32) {
        self.bytes[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn bands(&self, off: usize) -> [f32; layout::BANDS] {
        std::array::from_fn(|i| self.f32_at(off + i * 4))
    }

    fn set_bands(&mut self, off: usize, gains: &[f32; layout::BANDS]) {
        for (i, g) in gains.iter().enumerate() {
            self.set_f32(off + i * 4, g.clamp(EQ_MIN_DB, EQ_MAX_DB));
        }
    }

    pub fn header(&self) -> u32 {
        u32::from_le_bytes(self.bytes[layout::HEADER..layout::HEADER + 4].try_into().unwrap())
    }

    pub fn speaker_eq(&self) -> [f32; layout::BANDS] {
        self.bands(layout::SPEAKER_EQ)
    }

    pub fn set_speaker_eq(&mut self, gains: &[f32; layout::BANDS]) {
        self.set_bands(layout::SPEAKER_EQ, gains);
    }

    pub fn mic_eq(&self) -> [f32; layout::BANDS] {
        self.bands(layout::MIC_EQ)
    }

    /// Also switches the mic EQ on when any band is non-zero (and off when flat),
    /// since the APO ignores the gains while `mic_eq_enabled` is 0.
    pub fn set_mic_eq(&mut self, gains: &[f32; layout::BANDS]) {
        self.set_bands(layout::MIC_EQ, gains);
        self.bytes[layout::MIC_EQ_ENABLED] = gains.iter().any(|&g| g != 0.0) as u8;
    }

    pub fn speaker_eq_enabled(&self) -> bool {
        self.u8_at(layout::SPEAKER_EQ_ENABLED) != 0
    }

    /// Reverb amount, 0.0..=1.0.
    pub fn reverb(&self) -> f32 {
        self.f32_at(layout::REVERB_LEVEL)
    }

    pub fn set_reverb(&mut self, v: f32) {
        self.set_f32(layout::REVERB_LEVEL, v.clamp(0.0, 1.0));
    }

    /// Noise gate amount, 0.0..=1.0.
    pub fn noise_gate(&self) -> f32 {
        self.f32_at(layout::NOISE_GATE_LEVEL)
    }

    /// Also switches the gate on when the amount is above 0 (and off at 0), since
    /// the APO ignores the amount while `noise_gate_enabled` is 0.
    pub fn set_noise_gate(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.set_f32(layout::NOISE_GATE_LEVEL, v);
        self.bytes[layout::NOISE_GATE_ENABLED] = (v > 0.0) as u8;
    }

    pub fn surround(&self) -> bool {
        self.u8_at(layout::SURROUND_ENABLED) != 0
    }

    /// Mirrors the official UI, which flips the surround and reverb-enable bytes together.
    pub fn set_surround(&mut self, on: bool) {
        self.bytes[layout::SURROUND_ENABLED] = on as u8;
        self.bytes[layout::REVERB_ENABLED] = on as u8;
    }

    pub fn reverb_enabled(&self) -> bool {
        self.u8_at(layout::REVERB_ENABLED) != 0
    }

    pub fn mic_eq_enabled(&self) -> bool {
        self.u8_at(layout::MIC_EQ_ENABLED) != 0
    }

    pub fn noise_gate_enabled(&self) -> bool {
        self.u8_at(layout::NOISE_GATE_ENABLED) != 0
    }

    pub fn mic_meters(&self) -> bool {
        self.u8_at(layout::MIC_METERS_ENABLED) != 0
    }

    /// The APO only updates the level meters while this is set (and a program is
    /// recording). It does not affect the sound.
    pub fn set_mic_meters(&mut self, on: bool) {
        self.bytes[layout::MIC_METERS_ENABLED] = on as u8;
    }

    /// Read one field of [`layout::NAMED`]: integers as stored, `*_level` fields
    /// as their f32 value.
    pub fn named(&self, name: &str) -> Option<f64> {
        let &(_, off, width) = layout::NAMED.iter().find(|f| f.0 == name)?;
        Some(if name.ends_with("_level") {
            self.f32_at(off) as f64
        } else {
            let mut b = [0u8; 4];
            b[..width].copy_from_slice(&self.bytes[off..off + width]);
            u32::from_le_bytes(b) as f64
        })
    }

    /// Write one field of [`layout::NAMED`] (for experiments; see `examples/miclab.rs`).
    pub fn set_named(&mut self, name: &str, value: f64) -> Result<(), String> {
        let &(_, off, width) =
            layout::NAMED.iter().find(|f| f.0 == name).ok_or_else(|| format!("unknown field {name}"))?;
        if name.ends_with("_level") {
            self.set_f32(off, (value as f32).clamp(0.0, 1.0));
        } else {
            let v = value as u32;
            if width == 1 && v > 0xFF {
                return Err(format!("{name} is one byte"));
            }
            self.bytes[off..off + width].copy_from_slice(&v.to_le_bytes()[..width]);
        }
        Ok(())
    }

    /// The integer fields of [`layout::NAMED`] (flags and presets), as stored.
    pub fn named_ints(&self) -> Vec<(&'static str, u32)> {
        layout::NAMED
            .iter()
            .filter(|&&(name, _, _)| !name.ends_with("_level"))
            .map(|&(name, off, width)| {
                let mut b = [0u8; 4];
                b[..width].copy_from_slice(&self.bytes[off..off + width]);
                (name, u32::from_le_bytes(b))
            })
            .collect()
    }

    pub fn meters(&self) -> [f32; layout::METER_COUNT] {
        std::array::from_fn(|i| self.f32_at(layout::METERS + i * 4))
    }
}

/// A device that currently exposes an APO settings channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId {
    pub vid: u16,
    pub pid: u16,
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vid, self.pid)
    }
}

impl std::str::FromStr for DeviceId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (v, p) = s.split_once(':').ok_or("expected VID:PID in hex, e.g. 1395:0303")?;
        let parse = |x: &str| u16::from_str_radix(x, 16).map_err(|e| format!("{x}: {e}"));
        Ok(DeviceId { vid: parse(v)?, pid: parse(p)? })
    }
}

/// Find every device whose APO settings section currently exists.
pub fn discover() -> io::Result<Vec<(DeviceId, String)>> {
    let mut found = Vec::new();
    for entry in objdir::list(r"\BaseNamedObjects")? {
        if entry.type_name != "Section" {
            continue;
        }
        let Some(rest) = strip_prefix_ci(&entry.name, OBJECT_GUID) else { continue };
        let Some(ids) = rest.strip_prefix('_').and_then(|r| r.strip_suffix("_memory")) else {
            continue;
        };
        let Some((v, p)) = ids.split_once('_') else { continue };
        if let (Ok(vid), Ok(pid)) = (u16::from_str_radix(v, 16), u16::from_str_radix(p, 16)) {
            // Keep the exact spelling: kernel object names are case-sensitive.
            let base = entry.name.trim_end_matches("_memory").to_string();
            found.push((DeviceId { vid, pid }, base));
        }
    }
    Ok(found)
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// An open APO settings channel for one device.
pub struct ApoChannel {
    pub device: DeviceId,
    _mapping: OwnedHandle,
    view: *mut u8,
    mutex: OwnedHandle,
    event: OwnedHandle,
}

// The view is process-wide shared memory; access is serialised by the named mutex.
unsafe impl Send for ApoChannel {}

struct MutexGuard<'a>(&'a OwnedHandle);

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0.0) };
    }
}

impl ApoChannel {
    /// Open the channel for `device`, or for the first discovered device if `None`.
    pub fn open(device: Option<DeviceId>) -> io::Result<Self> {
        let found = discover()?;
        let (id, base) = match device {
            Some(want) => found.into_iter().find(|(id, _)| *id == want).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no EPOS APO channel for {want}; is the device connected?"),
                )
            })?,
            None => found.into_iter().next().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "no EPOS APO channel found; is an EPOS device connected and its audio driver installed?",
                )
            })?,
        };
        Self::open_named(id, &format!("Global\\{base}"))
    }

    fn open_named(device: DeviceId, base: &str) -> io::Result<Self> {
        let last = || io::Error::last_os_error();
        unsafe {
            let mapping = OwnedHandle(OpenFileMappingW(
                FILE_MAP_READ | FILE_MAP_WRITE,
                0,
                wide(&format!("{base}_memory")).as_ptr(),
            ));
            if mapping.0.is_null() {
                return Err(last());
            }
            let MEMORY_MAPPED_VIEW_ADDRESS { Value: view } =
                MapViewOfFile(mapping.0, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, layout::LEN);
            if view.is_null() {
                return Err(last());
            }
            let view = view as *mut u8;
            let mutex = OwnedHandle(OpenMutexW(
                SYNCHRONIZE | MUTEX_MODIFY_STATE,
                0,
                wide(&format!("{base}_mutex")).as_ptr(),
            ));
            let event = OwnedHandle(OpenEventW(
                SYNCHRONIZE | EVENT_MODIFY_STATE,
                0,
                wide(&format!("{base}_event")).as_ptr(),
            ));
            if mutex.0.is_null() || event.0.is_null() {
                let err = last();
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.cast() });
                return Err(err);
            }
            Ok(Self { device, _mapping: mapping, view, mutex, event })
        }
    }

    fn lock(&self) -> io::Result<MutexGuard<'_>> {
        match unsafe { WaitForSingleObject(self.mutex.0, LOCK_TIMEOUT_MS) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(MutexGuard(&self.mutex)),
            _ => Err(io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for the APO settings mutex")),
        }
    }

    fn copy_out(&self) -> Settings {
        let mut bytes = [0u8; layout::LEN];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = unsafe { ptr::read_volatile(self.view.add(i)) };
        }
        Settings::from_bytes(bytes)
    }

    /// Read a consistent copy of the settings block.
    pub fn read(&self) -> io::Result<Settings> {
        let _g = self.lock()?;
        Ok(self.copy_out())
    }

    /// Read-modify-write under the channel mutex, then notify the APO.
    ///
    /// Only 32-bit words that actually changed are written, and nothing in the
    /// APO-owned meter area. Returns the settings as written.
    pub fn modify(&self, f: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
        let changed;
        let after;
        {
            let _g = self.lock()?;
            let before = self.copy_out();
            let mut next = before.clone();
            f(&mut next);
            let mut any = false;
            for off in (0..layout::WRITABLE_END).step_by(4) {
                let new = &next.bytes[off..off + 4];
                if *new != before.bytes[off..off + 4] {
                    let word = u32::from_le_bytes(new.try_into().unwrap());
                    unsafe { ptr::write_volatile(self.view.add(off) as *mut u32, word) };
                    any = true;
                }
            }
            changed = any;
            after = next;
        }
        if changed {
            unsafe { SetEvent(self.event.0) };
        }
        Ok(after)
    }
}

impl Drop for ApoChannel {
    fn drop(&mut self) {
        unsafe { UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.view.cast() }) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ints_read_widths_and_skip_levels() {
        let mut b = [0u8; layout::LEN];
        b[0x44] = 1;
        b[0x45] = 0xFF; // a one-byte flag must not pick up its neighbours
        b[0xB4..0xB8].copy_from_slice(&2u32.to_le_bytes());
        b[0x98..0x9C].copy_from_slice(&0.5f32.to_le_bytes());
        let s = Settings::from_bytes(b);
        let named = s.named_ints();
        let get = |n: &str| named.iter().find(|(k, _)| *k == n).map(|&(_, v)| v);
        assert_eq!(get("noise_gate_enabled"), Some(1));
        assert_eq!(get("reverb_enabled"), Some(0xFF));
        assert_eq!(get("mic_speech_compressor_preset"), Some(2));
        assert_eq!(get("noise_gate_level"), None);
    }

    #[test]
    fn gate_and_mic_eq_switch_their_enable_flags() {
        let mut s = Settings::from_bytes([0u8; layout::LEN]);
        s.set_noise_gate(0.35);
        assert!(s.noise_gate_enabled());
        s.set_noise_gate(0.0);
        assert!(!s.noise_gate_enabled());
        let mut g = [0.0; layout::BANDS];
        s.set_mic_eq(&g);
        assert!(!s.mic_eq_enabled());
        g[3] = -2.0;
        s.set_mic_eq(&g);
        assert!(s.mic_eq_enabled());
        s.set_mic_meters(true);
        assert!(s.mic_meters() && s.mic_eq_enabled() && !s.noise_gate_enabled());
    }

    #[test]
    fn named_fields_stay_below_the_meters() {
        for (name, off, width) in layout::NAMED {
            assert!(off + width <= layout::WRITABLE_END, "{name}");
        }
    }
}
