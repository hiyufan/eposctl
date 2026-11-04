//! Command-line syntax. Only parsing lives here; `commands` does the work.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use eposctl::apo::{DeviceId, layout::BANDS};
use eposctl::presets;

#[derive(Parser)]
#[command(version, about = "Control EPOS headsets without the EPOS Gaming Suite")]
pub struct Cli {
    /// Device as VID:PID in hex (default: first one found), e.g. 1395:0303
    #[arg(long, global = true)]
    pub device: Option<DeviceId>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
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
        file: PathBuf,
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
        file: Option<PathBuf>,
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

#[derive(Subcommand)]
pub enum AudioCmd {
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

#[derive(Clone, Copy, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

fn preset_names() -> clap::builder::PossibleValuesParser {
    presets::all().map(|p| p.name).collect::<Vec<_>>().into()
}

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

impl OnOff {
    pub fn is_on(self) -> bool {
        matches!(self, Self::On)
    }
}
