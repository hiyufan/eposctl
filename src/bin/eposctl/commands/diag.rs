//! Research and diagnostics: `monitor`, `raw` and `report`.

use std::io;
use std::time::Duration;

use eposctl::capricorn::{Frame, Message};
use eposctl::headset::Headset;
use eposctl::hid::{HidDevice, find_vendor_collections};
use eposctl::report;

/// `report`: a read-only device report for a GitHub issue.
pub fn report(seconds: u64, query: bool) -> io::Result<()> {
    let opts = report::Options { listen: Duration::from_secs(seconds), query };
    print!("{}", report::build(&opts, |m| eprintln!("{m}")));
    Ok(())
}

// ---- monitor ----

/// `monitor`: passively print what the dongle sends, until Ctrl+C.
pub fn monitor(raw: bool) -> io::Result<()> {
    let infos = find_vendor_collections();
    let info =
        infos.first().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no EPOS vendor HID collection found"))?;
    let dev = HidDevice::open_read(info)?;
    eprintln!("listening on {:04x}:{:04x} (Ctrl+C to stop)", info.vid, info.pid);
    loop {
        if let Some(report) = dev.read(1000)? {
            print_report(&report, raw);
        }
    }
}

fn print_report(report: &[u8], raw: bool) {
    let ts = local_time();
    let f = match Frame::parse(report) {
        Ok(f) => f,
        Err(e) => return println!("{ts} unparsed ({e:?}): {:02x?}", &report[..report.len().min(24)]),
    };
    let msg = f.decode();
    if raw || msg == Message::Unknown {
        println!("{ts} kind={:#06x} node={} words={:04x?}", f.kind, f.node, f.words());
    }
    if msg != Message::Unknown {
        println!("{ts} {msg:?}");
    }
}

/// Local wall-clock time as HH:MM:SS.mmm without pulling in a date crate.
fn local_time() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: SYSTEMTIME is plain data; GetLocalTime fills it in.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

// ---- raw ----

/// Read-only requests for `raw --probe`: the requests whose responses the
/// official service hands to a getter-style parser (see docs/protocol.md). The
/// serial number is left out.
const PROBE: &[(u16, u16, &str)] = &[
    (0x09, 0x01, "product name"),
    (0x01, 0x03, "firmware"),
    (0x12, 0x03, "battery"),
    (0x0A, 0x0C, "sidetone"),
    (0x0A, 0x12, "noise reduction level"),
    (0x0A, 0x17, "latency level"),
    (0x01, 0x14, "PID number"),
    (0x01, 0x09, "sub-devices connected"),
    (0x01, 0x20, "unknown; read side of 01/1F"),
    (0x06, 0x14, "Bluetooth address"),
    (0x06, 0x11, "number of link keys"),
    (0x06, 0x27, "Bluetooth name"),
];

/// Never sent: puts the headset into firmware update mode (`parseEnterDfu`).
const FORBIDDEN: &[(u16, u16)] = &[(0x01, 0x06)];

/// Requests the official service sends without parsing a value from the reply,
/// or that touch firmware update or pairing: they may be actions, so they need
/// `--write` like setters do.
const ACTIONS: &[(u16, u16)] = &[(0x01, 0x0A), (0x01, 0x0D), (0x06, 0x09), (0x06, 0x13), (0x06, 0x1C), (0x08, 0xF0)];

/// One request to send: group, command, appended values and a note to print.
struct Request {
    group: u16,
    command: u16,
    values: Vec<u16>,
    note: &'static str,
}

/// `raw`: send one request (or the probe list) and print every frame back.
pub fn raw(req: Option<(u16, u16)>, values: Vec<u16>, write: bool, listen: u64) -> io::Result<()> {
    check_allowed(req, !values.is_empty(), write)?;
    let requests = match req {
        Some((group, command)) => vec![Request { group, command, values, note: "" }],
        None => {
            PROBE.iter().map(|&(group, command, note)| Request { group, command, values: Vec::new(), note }).collect()
        }
    };
    let h = Headset::open()?;
    let window = Duration::from_millis(listen);
    for r in requests {
        send(&h, r, window)?;
    }
    Ok(())
}

/// Refuse requests that could change the headset unless `--write` was given.
fn check_allowed(req: Option<(u16, u16)>, has_values: bool, write: bool) -> io::Result<()> {
    let refusal = match req {
        Some(r) if FORBIDDEN.contains(&r) => {
            Some("refusing to send this request: it puts the headset into firmware update mode")
        }
        Some(r) if ACTIONS.contains(&r) && !write => {
            Some("this request may perform an action (firmware update or pairing related); add --write to send it")
        }
        _ if has_values && !write => Some(
            "values turn the request into a setter, which may change a setting stored on the headset; add --write to send it",
        ),
        _ => None,
    };
    refusal.map_or(Ok(()), |m| Err(io::Error::other(m)))
}

fn send(h: &Headset, r: Request, window: Duration) -> io::Result<()> {
    let Request { group: g, command: c, values, note } = r;
    let args: String = values.iter().map(|w| format!(" {w}")).collect();
    let note = if note.is_empty() { String::new() } else { format!("  ({note})") };
    println!("> {g:02X}/{c:02X}{args}{note}");
    let words: Vec<u16> = [g, c].into_iter().chain(values).collect();
    let frames = h.exchange(&words, window)?;
    if frames.is_empty() {
        println!("  no response");
    }
    for f in frames {
        // `<` marks the response that echoes the request's group and command.
        let mark = if f.group() == Some(g) && f.command() == Some(c) { '<' } else { ' ' };
        println!("{mark} kind={:#06x} node={} {}", f.kind, f.node, report::describe(&f));
    }
    Ok(())
}
