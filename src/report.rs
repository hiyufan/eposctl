//! `eposctl report`: a read-only summary of what eposctl can see, for bug
//! reports and for adding support for other EPOS models.
//!
//! Nothing is written to the headset or the driver. The serial number and
//! Bluetooth address are never included: HID paths are omitted and those
//! frames are redacted.

use std::fmt::Write as _;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::apo::{self, ApoChannel};
use crate::capricorn::{Frame, Message};
use crate::hid::{self, HidDevice, HidInfo};

/// Vendor IDs used by EPOS and by Sennheiser gaming products before the split.
pub const VENDOR_IDS: [u16; 2] = [0x1395, 0x1377];

fn windows_version() -> String {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let get = |name: &str| -> Option<String> {
        let key: Vec<u16> = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion".encode_utf16().chain(Some(0)).collect();
        let val: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut buf = [0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (rc == 0).then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
    };
    format!(
        "build {} ({})",
        get("CurrentBuild").unwrap_or_else(|| "?".into()),
        get("DisplayVersion").unwrap_or_else(|| "?".into())
    )
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// Responses that identify the unit (serial number, Bluetooth address): their
/// values are never printed.
const PRIVATE: [(u16, u16); 2] = [(0x01, 0x0B), (0x06, 0x14)];

/// One-line description of a frame, with identifying values redacted.
pub fn describe(f: &Frame) -> String {
    let w = f.words();
    if w.len() > 2 && PRIVATE.contains(&(w[0], w[1])) {
        return format!("words=[{:04x}, {:04x}, <redacted>]", w[0], w[1]);
    }
    match f.decode() {
        Message::SerialNumber(_) => "SerialNumber(<redacted>)".into(),
        Message::Unknown => format!("words={:04x?}", f.words()),
        m => format!("{m:?}"),
    }
}

pub struct Options {
    /// How long to listen to vendor HID traffic.
    pub listen: Duration,
    /// Also send read-only requests (product name, firmware, battery, sidetone).
    pub query: bool,
}

/// Build the report. `prompt` receives progress lines meant for the user.
pub fn build(opts: &Options, mut prompt: impl FnMut(&str)) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "eposctl {} device report", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(r, "Windows {}", windows_version());

    // APO channels.
    let _ = writeln!(r, "\n[APO settings channels]");
    match apo::discover() {
        Ok(found) if found.is_empty() => {
            let _ = writeln!(r, "none");
        }
        Ok(found) => {
            for (id, _) in found {
                let _ = writeln!(r, "{id}");
                match ApoChannel::open(Some(id)).and_then(|c| c.read()) {
                    Ok(s) => {
                        for (i, chunk) in s.raw().chunks(16).enumerate() {
                            if chunk.iter().any(|&b| b != 0) {
                                let _ = writeln!(r, "  {:04x}  {}", i * 16, hex(chunk));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(r, "  (cannot read: {e})");
                    }
                }
            }
        }
        Err(e) => {
            let _ = writeln!(r, "error: {e}");
        }
    }

    // HID interfaces.
    let _ = writeln!(r, "\n[HID interfaces]");
    let devs: Vec<HidInfo> = hid::enumerate().into_iter().filter(|d| VENDOR_IDS.contains(&d.vid)).collect();
    if devs.is_empty() {
        let _ = writeln!(r, "none");
    }
    for d in &devs {
        let _ = writeln!(
            r,
            "{:04x}:{:04x}  page {:04x} usage {:04x}  in {:3} out {:3}  {}",
            d.vid,
            d.pid,
            d.usage_page,
            d.usage,
            d.input_len,
            d.output_len,
            d.product.as_deref().unwrap_or("-")
        );
    }

    // Optional read-only queries.
    if opts.query {
        let _ = writeln!(r, "\n[Queries]");
        match crate::headset::Headset::open() {
            Ok(h) => {
                let show = |r: &mut String, label: &str, v: std::io::Result<String>| {
                    let _ = match v {
                        Ok(v) => writeln!(r, "{label:<9} {v}"),
                        Err(e) => writeln!(r, "{label:<9} ({e})"),
                    };
                };
                show(&mut r, "product", h.product_name());
                show(&mut r, "firmware", h.firmware());
                show(&mut r, "battery", h.battery().map(|b| format!("{b}%")));
                show(&mut r, "sidetone", h.sidetone().map(|s| s.to_string()));
            }
            Err(e) => {
                let _ = writeln!(r, "({e})");
            }
        }
    }

    // Passive listening on vendor collections.
    let vendor: Vec<&HidInfo> = devs.iter().filter(|d| d.usage_page >= 0xFF00).collect();
    let _ = writeln!(r, "\n[Vendor HID traffic, {} s]", opts.listen.as_secs());
    if vendor.is_empty() {
        let _ = writeln!(r, "no vendor collections");
        return r;
    }
    prompt(&format!(
        "Listening for {} s. Meanwhile: switch the headset off and on, press its buttons (ANC, mute), turn the volume wheel.",
        opts.listen.as_secs()
    ));
    let (tx, rx) = mpsc::channel::<(Duration, String, Vec<u8>)>();
    let start = Instant::now();
    let mut opened = 0;
    for d in vendor {
        let tag = format!("{:04x}:{:04x}/{:04x}", d.vid, d.pid, d.usage_page);
        match HidDevice::open_read(d) {
            Ok(dev) => {
                opened += 1;
                let (tx, end) = (tx.clone(), start + opts.listen);
                std::thread::spawn(move || {
                    while let Some(left) = end.checked_duration_since(Instant::now()) {
                        match dev.read(left.as_millis().clamp(1, 200) as u32) {
                            Ok(Some(rep)) => {
                                let _ = tx.send((start.elapsed(), tag.clone(), rep));
                            }
                            Ok(None) => {}
                            Err(_) => break,
                        }
                    }
                });
            }
            Err(e) => {
                let _ = writeln!(r, "{tag}: cannot open ({e})");
            }
        }
    }
    drop(tx);
    let mut count = 0;
    for (t, tag, rep) in rx {
        count += 1;
        let line = match Frame::parse(&rep) {
            Ok(f) => format!("kind={:#06x} node={} {}", f.kind, f.node, describe(&f)),
            Err(_) => format!("raw {}", hex(&rep[..rep.len().min(32)])),
        };
        let _ = writeln!(r, "+{:6.2}s {tag} {line}", t.as_secs_f32());
    }
    if count == 0 && opened > 0 {
        let _ = writeln!(r, "no reports received");
    }
    r
}
