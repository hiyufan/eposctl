use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use eposctl::apo::{self, ApoChannel, DeviceId, Settings, layout::BANDS};
use eposctl::presets;

#[derive(Parser)]
#[command(version, about = "Control EPOS headsets without the EPOS Gaming Suite")]
struct Cli {
    /// Device as VID:PID in hex (default: first one found), e.g. 1395:0303
    #[arg(long, global = true)]
    device: Option<DeviceId>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List devices that expose an APO settings channel
    List,
    /// Show current DSP settings
    Status {
        /// Also print the raw settings block as hex
        #[arg(long)]
        raw: bool,
    },
    /// Apply a factory or game preset (speaker side)
    Preset {
        #[arg(value_parser = preset_names())]
        name: String,
    },
    /// Set the 9 speaker EQ bands in dB (-6..6), lowest frequency first
    #[command(allow_negative_numbers = true)]
    Eq {
        #[arg(num_args = BANDS, required = true)]
        gains: Vec<f32>,
    },
    /// Set the 9 microphone EQ bands in dB (-6..6), lowest frequency first
    #[command(allow_negative_numbers = true)]
    MicEq {
        #[arg(num_args = BANDS, required = true)]
        gains: Vec<f32>,
    },
    /// Load an Equalizer APO / AutoEQ file (ParametricEQ.txt, GraphicEQ.txt or a CSV)
    /// into the 9 EQ bands
    EqImport {
        file: std::path::PathBuf,
        /// Load into the microphone EQ instead of the speaker EQ
        #[arg(long)]
        mic: bool,
        /// Only print the bands, do not write them
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a parametric EQ with any number of bands (ParametricEQ.txt) to
    /// Equalizer APO for the EPOS playback device, and flatten the headset's own
    /// 9-band EQ. Without FILE: show what is there
    Eqapo {
        file: Option<std::path::PathBuf>,
        /// Remove eposctl's bands from Equalizer APO
        #[arg(long, conflicts_with = "file")]
        off: bool,
    },
    /// Set reverb amount in percent (0-100)
    Reverb { percent: f32 },
    /// Set microphone noise gate amount in percent (0-100)
    Gate { percent: f32 },
    /// Turn virtual 3D surround on or off
    Surround { state: OnOff },
    /// Show headset info read over HID: name, serial, firmware, battery, sidetone
    Info,
    /// Show or set sidetone level
    Sidetone { level: Option<u16> },
    /// Show or set the headset's microphone noise reduction: 0 off, 1 medium, 2 strong
    #[command(alias = "nr")]
    NoiseReduction {
        #[arg(value_parser = clap::value_parser!(u16).range(0..=2))]
        level: Option<u16>,
    },
    /// Print a read-only device report to attach to a GitHub issue (no serial number)
    Report {
        /// Seconds to listen to headset traffic
        #[arg(long, default_value_t = 15)]
        seconds: u64,
        /// Also send read-only queries (product name, firmware, battery, sidetone)
        #[arg(long)]
        query: bool,
    },
    /// Passively print headset events and responses from the dongle (read-only)
    Monitor {
        /// Also print every raw frame
        #[arg(long)]
        raw: bool,
    },
    /// List Windows playback and recording devices (* = default)
    Audio {
        #[command(subcommand)]
        cmd: Option<AudioCmd>,
    },
    /// Send one raw HID request to the headset and print every frame that comes back
    /// (research). GROUP and COMMAND are hex, as in docs/protocol.md: `raw 0A 12`
    Raw {
        #[arg(value_parser = parse_hex, required_unless_present = "probe")]
        group: Option<u16>,
        #[arg(value_parser = parse_hex, required_unless_present = "probe")]
        command: Option<u16>,
        /// u16 values to append (decimal, or hex with 0x). They make the request a setter.
        #[arg(value_parser = parse_num)]
        values: Vec<u16>,
        /// Allow appending values. Setters can change settings stored on the headset.
        #[arg(long)]
        write: bool,
        /// Send the read-only requests the official service sends on connect
        #[arg(long, conflicts_with_all = ["group", "command", "values", "write"])]
        probe: bool,
        /// Milliseconds to collect frames after each request
        #[arg(long, default_value_t = 800)]
        listen: u64,
    },
}

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

fn parse_hex(s: &str) -> Result<u16, String> {
    let t = s.trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(t, 16).map_err(|e| format!("{s}: {e}"))
}

fn parse_num(s: &str) -> Result<u16, String> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u16::from_str_radix(h, 16),
        None => s.parse(),
    }
    .map_err(|e| format!("{s}: {e}"))
}

fn raw(req: Option<(u16, u16)>, values: Vec<u16>, write: bool, listen: u64) -> std::io::Result<()> {
    let refuse = |m: &str| Err(std::io::Error::other(m.to_string()));
    if let Some(r) = req {
        if FORBIDDEN.contains(&r) {
            return refuse("refusing to send this request: it puts the headset into firmware update mode");
        }
        if ACTIONS.contains(&r) && !write {
            return refuse(
                "this request may perform an action (firmware update or pairing related); add --write to send it",
            );
        }
    }
    if !values.is_empty() && !write {
        return refuse(
            "values turn the request into a setter, which may change a setting stored on the headset; add --write to send it",
        );
    }
    let list: Vec<(u16, u16, Vec<u16>, &str)> = match req {
        Some((g, c)) => vec![(g, c, values, "")],
        None => PROBE.iter().map(|&(g, c, what)| (g, c, Vec::new(), what)).collect(),
    };
    let h = eposctl::headset::Headset::open()?;
    let window = std::time::Duration::from_millis(listen);
    for (g, c, v, what) in list {
        let args: String = v.iter().map(|w| format!(" {w}")).collect();
        let note = if what.is_empty() { String::new() } else { format!("  ({what})") };
        println!("> {g:02X}/{c:02X}{args}{note}");
        let words: Vec<u16> = [g, c].into_iter().chain(v).collect();
        let frames = h.exchange(&words, window)?;
        if frames.is_empty() {
            println!("  no response");
        }
        for f in frames {
            // `<` marks the response that echoes the request's group and command.
            let mark = if f.group() == Some(g) && f.command() == Some(c) { '<' } else { ' ' };
            println!("{mark} kind={:#06x} node={} {}", f.kind, f.node, eposctl::report::describe(&f));
        }
    }
    Ok(())
}

#[derive(Subcommand)]
enum AudioCmd {
    /// Make a device the Windows default (matched by part of its name, or its id)
    Default {
        device: String,
        /// A recording device instead of a playback device
        #[arg(long)]
        input: bool,
    },
    /// List programs playing on the default playback device, with their volume
    Sessions,
}

fn audio(cmd: Option<AudioCmd>) -> std::io::Result<()> {
    use eposctl::audio::{self, Flow};
    let err = |e: windows::core::Error| std::io::Error::other(e.to_string());
    match cmd {
        None => {
            for (flow, title) in [(Flow::Output, "playback"), (Flow::Input, "recording")] {
                let default = audio::default_id(flow).map_err(err)?;
                println!("{title}");
                for ep in audio::endpoints(flow).map_err(err)? {
                    let mark = if default.as_deref() == Some(ep.id.as_str()) { "*" } else { " " };
                    println!("  {mark} {}", ep.name);
                }
            }
        }
        Some(AudioCmd::Default { device, input }) => {
            let flow = if input { Flow::Input } else { Flow::Output };
            let ep = audio::find(flow, &device).map_err(std::io::Error::other)?;
            audio::set_default(&ep.id).map_err(err)?;
            println!("default {}: {}", if input { "recording" } else { "playback" }, ep.name);
        }
        Some(AudioCmd::Sessions) => {
            for s in audio::sessions(None).map_err(err)? {
                let vol = s.volume().map(|v| format!("{:3.0}%", v * 100.0)).unwrap_or_else(|_| "  ?".into());
                println!("{vol}  {}", s.exe.as_deref().unwrap_or("?"));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

fn preset_names() -> clap::builder::PossibleValuesParser {
    presets::all().map(|p| p.name).collect::<Vec<_>>().into()
}

fn bands(v: &[f32]) -> [f32; BANDS] {
    v.try_into().expect("clap enforces the band count")
}

fn fmt_bands(b: &[f32]) -> String {
    b.iter().map(|g| format!("{g:+5.2}")).collect::<Vec<_>>().join(" ")
}

fn print_status(dev: DeviceId, s: &Settings, raw: bool) {
    let pct = |v: f32| v * 100.0;
    println!("device       {dev}");
    println!("header       {}", s.header());
    println!(
        "speaker EQ   [{}] dB{}",
        fmt_bands(&s.speaker_eq()),
        if s.speaker_eq_enabled() { "" } else { "  (disabled)" }
    );
    println!("mic EQ       [{}] dB{}", fmt_bands(&s.mic_eq()), if s.mic_eq_enabled() { "" } else { "  (disabled)" });
    println!("surround 3D  {}", if s.surround() { "on" } else { "off" });
    println!("reverb       {:.1}%{}", pct(s.reverb()), if s.reverb_enabled() { "" } else { "  (disabled)" });
    println!("noise gate   {:.1}%{}", pct(s.noise_gate()), if s.noise_gate_enabled() { "" } else { "  (disabled)" });
    println!("mic meters   {}", if s.mic_meters() { "on" } else { "off" });
    if raw {
        for (name, v) in s.named_ints() {
            println!("  {name:<30} {v}");
        }
    }
    println!("meters dB    [{}]", s.meters().iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
    if raw {
        for (i, chunk) in s.raw().chunks(16).enumerate() {
            if chunk.iter().any(|&b| b != 0) {
                let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                println!("{:04x}  {}", i * 16, hex.join(" "));
            }
        }
    }
}

fn monitor(raw: bool) -> std::io::Result<()> {
    use eposctl::capricorn::{Frame, Message};
    use eposctl::hid::{HidDevice, find_vendor_collections};

    let infos = find_vendor_collections();
    let info = infos
        .first()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no EPOS vendor HID collection found"))?;
    let dev = HidDevice::open_read(info)?;
    eprintln!("listening on {:04x}:{:04x} (Ctrl+C to stop)", info.vid, info.pid);
    loop {
        let Some(report) = dev.read(1000)? else { continue };
        let ts = chrono_like_now();
        match Frame::parse(&report) {
            Ok(f) => {
                let msg = f.decode();
                if raw || msg == Message::Unknown {
                    println!("{ts} kind={:#06x} node={} words={:04x?}", f.kind, f.node, f.words());
                }
                if msg != Message::Unknown {
                    println!("{ts} {msg:?}");
                }
            }
            Err(e) => println!("{ts} unparsed ({e:?}): {:02x?}", &report[..report.len().min(24)]),
        }
    }
}

/// Local wall-clock time as HH:MM:SS.mmm without pulling in a date crate.
fn chrono_like_now() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

fn info() -> std::io::Result<()> {
    let h = eposctl::headset::Headset::open()?;
    let show = |label: &str, r: std::io::Result<String>| match r {
        Ok(v) => println!("{label:<10} {v}"),
        Err(e) => println!("{label:<10} ({e})"),
    };
    show("product", h.product_name());
    show("serial", h.serial_number());
    show("firmware", h.firmware());
    show("battery", h.battery().map(|b| format!("{b}%")));
    show("sidetone", h.sidetone().map(|s| s.to_string()));
    show("noise red.", h.noise_reduction().map(|l| nr_name(l).to_string()));
    Ok(())
}

fn eqapo(file: Option<std::path::PathBuf>, off: bool, device: Option<DeviceId>) -> std::io::Result<()> {
    use eposctl::audio::{self, Flow};
    use eposctl::eqapo;
    fn other(m: impl Into<String>) -> std::io::Error {
        std::io::Error::other(m.into())
    }
    let dir = eqapo::config_dir().ok_or_else(|| {
        other("Equalizer APO is not installed. Get it from https://sourceforge.net/projects/equalizerapo/ and select the EPOS playback device in its Configurator.")
    })?;
    let ep = audio::endpoints(Flow::Output)
        .map_err(|e| other(e.to_string()))?
        .into_iter()
        .find(|e| audio::is_epos(&e.name))
        .ok_or_else(|| other("no EPOS playback device found"))?;
    let word = eqapo::device_word(&ep.id).ok_or_else(|| other(format!("unexpected endpoint id {}", ep.id)))?;
    if off {
        let path = eqapo::write(None, &word)?;
        println!("removed eposctl's bands ({})", path.display());
        return Ok(());
    }
    let Some(file) = file else {
        println!("Equalizer APO config: {}", dir.display());
        match eqapo::read() {
            Some(p) => {
                println!("eposctl bands for {}:", ep.name);
                print!("{}", p.to_apo_text());
            }
            None => println!("no bands from eposctl"),
        }
        return Ok(());
    };
    let text = std::fs::read_to_string(&file)?;
    let peq = eposctl::peq::Peq::from_apo_text(&text).map_err(other)?;
    let path = eqapo::write(Some(&peq), &word)?;
    // Otherwise the headset's own EQ would apply on top.
    ApoChannel::open(device)?.modify(|s| s.set_speaker_eq(&[0.0; BANDS]))?;
    println!("wrote {} bands for {} to {}", peq.bands.len(), ep.name, path.display());
    println!("set the headset's own 9-band EQ flat");
    println!("Equalizer APO only applies them if it is installed on this device (check in its Configurator).");
    Ok(())
}

fn nr_name(level: u16) -> String {
    match level {
        0 => "0 (off)".into(),
        1 => "1 (medium)".into(),
        2 => "2 (strong)".into(),
        l => format!("{l} (unknown)"),
    }
}

fn noise_reduction(level: Option<u16>) -> std::io::Result<()> {
    let h = eposctl::headset::Headset::open()?;
    if let Some(l) = level {
        let status = h.set_noise_reduction(l)?;
        if status != 0 {
            return Err(std::io::Error::other(format!("headset rejected noise reduction {l} (status {status})")));
        }
    }
    println!("noise reduction {}", nr_name(h.noise_reduction()?));
    Ok(())
}

fn sidetone(level: Option<u16>) -> std::io::Result<()> {
    let h = eposctl::headset::Headset::open()?;
    if let Some(l) = level {
        let status = h.set_sidetone(l)?;
        if status != 0 {
            return Err(std::io::Error::other(format!("headset rejected sidetone {l} (status {status})")));
        }
    }
    println!("sidetone {}", h.sidetone()?);
    Ok(())
}

fn run(cli: Cli) -> std::io::Result<()> {
    match cli.cmd {
        Cmd::Monitor { raw } => return monitor(raw),
        Cmd::Audio { cmd } => return audio(cmd),
        Cmd::Info => return info(),
        Cmd::Sidetone { level } => return sidetone(level),
        Cmd::NoiseReduction { level } => return noise_reduction(level),
        Cmd::Eqapo { file, off } => return eqapo(file, off, cli.device),
        Cmd::Raw { group, command, values, write, listen, .. } => {
            return raw(group.zip(command), values, write, listen);
        }
        Cmd::Report { seconds, query } => {
            let opts = eposctl::report::Options { listen: std::time::Duration::from_secs(seconds), query };
            print!("{}", eposctl::report::build(&opts, |m| eprintln!("{m}")));
            return Ok(());
        }
        _ => {}
    }
    if let Cmd::List = cli.cmd {
        let found = apo::discover()?;
        if found.is_empty() {
            println!("no EPOS APO channels found");
        }
        for (id, name) in found {
            println!("{id}  {name}");
        }
        return Ok(());
    }

    let mut imported = None;
    if let Cmd::EqImport { file, dry_run, .. } = &cli.cmd {
        let text = std::fs::read_to_string(file)?;
        let imp = eposctl::eqfile::parse(&text).map_err(std::io::Error::other)?;
        for (i, g) in imp.gains.iter().enumerate() {
            println!("{:>8}  {g:+.1} dB", eposctl::eqfile::band_freq(i));
        }
        if let Ok(peq) = eposctl::peq::Peq::from_apo_text(&text) {
            // How far the 9-band curve stays from the file's own curve.
            let off = |f: f64| eposctl::peq::epos_db(&imp.gains, f) - imp.shift as f64 - peq.db_at(f);
            // Same range as the fit: from half an octave below the lowest band.
            let grid = (0..=60).map(|k| 44.2 * (20000.0f64 / 44.2).powf(k as f64 / 60.0));
            let worst = grid.map(|f| (off(f).abs(), f)).fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
            println!(
                "largest difference from the file's curve: {:.1} dB at {:.0} Hz ({} filters on 9 bands)",
                worst.0,
                worst.1,
                peq.bands.len()
            );
        }
        if imp.shift != 0.0 {
            println!("shifted by {:+.1} dB to fit -6..+6 dB", imp.shift);
        }
        if imp.clipped {
            println!("the curve is taller than 12 dB; the ends were clipped");
        }
        if *dry_run {
            return Ok(());
        }
        imported = Some(imp.gains);
    }

    let ch = ApoChannel::open(cli.device)?;
    let written = match cli.cmd {
        Cmd::List
        | Cmd::Monitor { .. }
        | Cmd::Info
        | Cmd::Sidetone { .. }
        | Cmd::NoiseReduction { .. }
        | Cmd::Eqapo { .. }
        | Cmd::Report { .. }
        | Cmd::Audio { .. }
        | Cmd::Raw { .. } => {
            unreachable!()
        }
        Cmd::EqImport { mic, .. } => {
            let g = imported.expect("parsed above");
            ch.modify(|s| if mic { s.set_mic_eq(&g) } else { s.set_speaker_eq(&g) })?
        }
        Cmd::Status { raw } => {
            print_status(ch.device, &ch.read()?, raw);
            return Ok(());
        }
        Cmd::Preset { name } => {
            let p = presets::find(&name).expect("validated by clap");
            ch.modify(|s| p.apply(s))?
        }
        Cmd::Eq { gains } => ch.modify(|s| s.set_speaker_eq(&bands(&gains)))?,
        Cmd::MicEq { gains } => ch.modify(|s| s.set_mic_eq(&bands(&gains)))?,
        Cmd::Reverb { percent } => ch.modify(|s| s.set_reverb(percent / 100.0))?,
        Cmd::Gate { percent } => ch.modify(|s| s.set_noise_gate(percent / 100.0))?,
        Cmd::Surround { state } => ch.modify(|s| s.set_surround(matches!(state, OnOff::On)))?,
    };
    print_status(ch.device, &written, false);
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
