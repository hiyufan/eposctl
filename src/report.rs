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
use crate::win::{Hive, reg_string};

/// Vendor IDs used by EPOS and by Sennheiser gaming products before the split.
pub const VENDOR_IDS: [u16; 2] = [0x1395, 0x1377];

fn windows_version() -> String {
    let get = |name: &str| {
        reg_string(Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", name)
            .unwrap_or_else(|| "?".into())
    };
    format!("build {} ({})", get("CurrentBuild"), get("DisplayVersion"))
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
pub fn build(opts: &Options, prompt: impl FnMut(&str)) -> String {
    // Writing to a String cannot fail, so the `fmt::Result`s below are ignored.
    let mut r = String::new();
    let _ = writeln!(r, "eposctl {} device report", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(r, "Windows {}", windows_version());
    apo_channels(&mut r);
    let devs: Vec<HidInfo> = hid::enumerate().into_iter().filter(|d| VENDOR_IDS.contains(&d.vid)).collect();
    hid_interfaces(&mut r, &devs);
    if opts.query {
        queries(&mut r);
    }
    vendor_traffic(&mut r, opts, &devs, prompt);
    r
}

/// Every APO settings channel and the non-zero rows of its settings block.
fn apo_channels(r: &mut String) {
    let _ = writeln!(r, "\n[APO settings channels]");
    let found = match apo::discover() {
        Ok(found) => found,
        Err(e) => {
            let _ = writeln!(r, "error: {e}");
            return;
        }
    };
    if found.is_empty() {
        let _ = writeln!(r, "none");
    }
    for (id, _) in found {
        let _ = writeln!(r, "{id}");
        match ApoChannel::open(Some(id)).and_then(|c| c.read()) {
            Ok(s) => hex_rows(r, s.raw()),
            Err(e) => {
                let _ = writeln!(r, "  (cannot read: {e})");
            }
        }
    }
}

fn hex_rows(r: &mut String, bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(16).enumerate().filter(|(_, c)| c.iter().any(|&b| b != 0)) {
        let _ = writeln!(r, "  {:04x}  {}", i * 16, hex(chunk));
    }
}

fn hid_interfaces(r: &mut String, devs: &[HidInfo]) {
    let _ = writeln!(r, "\n[HID interfaces]");
    if devs.is_empty() {
        let _ = writeln!(r, "none");
    }
    for d in devs {
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
}

/// Read-only queries (no serial number).
fn queries(r: &mut String) {
    let _ = writeln!(r, "\n[Queries]");
    let h = match crate::headset::Headset::open() {
        Ok(h) => h,
        Err(e) => {
            let _ = writeln!(r, "({e})");
            return;
        }
    };
    let mut show = |label: &str, v: std::io::Result<String>| {
        let _ = match v {
            Ok(v) => writeln!(r, "{label:<9} {v}"),
            Err(e) => writeln!(r, "{label:<9} ({e})"),
        };
    };
    show("product", h.product_name());
    show("firmware", h.firmware());
    show("battery", h.battery().map(|b| format!("{b}%")));
    show("sidetone", h.sidetone().map(|s| s.to_string()));
}

/// A report read during listening: when, from which collection, the bytes.
type Heard = (Duration, String, Vec<u8>);

/// Passive listening on the vendor collections, one thread each.
fn vendor_traffic(r: &mut String, opts: &Options, devs: &[HidInfo], mut prompt: impl FnMut(&str)) {
    let vendor: Vec<&HidInfo> = devs.iter().filter(|d| d.usage_page >= 0xFF00).collect();
    let _ = writeln!(r, "\n[Vendor HID traffic, {} s]", opts.listen.as_secs());
    if vendor.is_empty() {
        let _ = writeln!(r, "no vendor collections");
        return;
    }
    prompt(&format!(
        "Listening for {} s. Meanwhile: switch the headset off and on, press its buttons (ANC, mute), turn the volume wheel.",
        opts.listen.as_secs()
    ));
    let (tx, rx) = mpsc::channel::<Heard>();
    let start = Instant::now();
    let end = start + opts.listen;
    let mut opened = 0;
    for d in vendor {
        let tag = format!("{:04x}:{:04x}/{:04x}", d.vid, d.pid, d.usage_page);
        match HidDevice::open_read(d) {
            Ok(dev) => {
                opened += 1;
                let tx = tx.clone();
                std::thread::spawn(move || listen(&dev, &tag, start, end, &tx));
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
}

/// Forward every report from `dev` until `end` or a read error.
fn listen(dev: &HidDevice, tag: &str, start: Instant, end: Instant, tx: &mpsc::Sender<Heard>) {
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let Ok(read) = dev.read(left.as_millis().clamp(1, 200) as u32) else { break };
        if let Some(rep) = read {
            let _ = tx.send((start.elapsed(), tag.to_owned(), rep));
        }
    }
}
