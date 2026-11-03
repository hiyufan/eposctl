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
    /// Apply a factory preset (speaker side)
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
    }
    Ok(())
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

fn preset_names() -> clap::builder::PossibleValuesParser {
    presets::FACTORY.iter().map(|p| p.name).collect::<Vec<_>>().into()
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
    println!("mic EQ       [{}] dB", fmt_bands(&s.mic_eq()));
    println!("surround 3D  {}", if s.surround() { "on" } else { "off" });
    println!(
        "reverb       {:.1}%{}",
        pct(s.reverb()),
        if s.reverb_enabled() { "" } else { "  (disabled)" }
    );
    println!("noise gate   {:.1}%", pct(s.noise_gate()));
    println!("mic monitors {:?}", s.mic_monitors());
    println!(
        "meters dB    [{}]",
        s.meters().iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" ")
    );
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
    let info = infos.first().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no EPOS vendor HID collection found")
    })?;
    let dev = HidDevice::open_read(info)?;
    eprintln!("listening on {:04x}:{:04x} (Ctrl+C to stop)", info.vid, info.pid);
    loop {
        let Some(report) = dev.read(1000)? else { continue };
        let ts = chrono_like_now();
        match Frame::parse(&report) {
            Ok(f) => {
                let msg = f.decode();
                if raw || msg == Message::Unknown {
                    println!(
                        "{ts} kind={:#06x} node={} words={:04x?}",
                        f.kind,
                        f.node,
                        f.words()
                    );
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

    let ch = ApoChannel::open(cli.device)?;
    let written = match cli.cmd {
        Cmd::List | Cmd::Monitor { .. } | Cmd::Info | Cmd::Sidetone { .. } | Cmd::Report { .. } | Cmd::Audio { .. } => {
            unreachable!()
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
