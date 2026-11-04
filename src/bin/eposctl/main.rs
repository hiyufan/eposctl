//! `eposctl`: command-line control of EPOS headsets.

mod args;
mod commands;

use std::io;
use std::process::ExitCode;

use clap::Parser;

use args::{Cli, Cmd};
use commands::{audio, diag, dsp, eqapo, headset};

fn run(cli: Cli) -> io::Result<()> {
    let dev = cli.device;
    match cli.cmd {
        // DSP settings in the driver.
        Cmd::List => dsp::list(),
        Cmd::Status { raw } => dsp::status(dev, raw),
        Cmd::Preset { name } => dsp::preset(dev, &name),
        Cmd::Eq { gains } => dsp::modify(dev, |s| s.set_speaker_eq(&dsp::bands(&gains))),
        Cmd::MicEq { gains } => dsp::modify(dev, |s| s.set_mic_eq(&dsp::bands(&gains))),
        Cmd::EqImport { file, mic, dry_run } => dsp::eq_import(dev, &file, mic, dry_run),
        Cmd::Reverb { percent } => dsp::modify(dev, |s| s.set_reverb(percent / 100.0)),
        Cmd::Gate { percent } => dsp::modify(dev, |s| s.set_noise_gate(percent / 100.0)),
        Cmd::Surround { state } => dsp::modify(dev, |s| s.set_surround(state.is_on())),
        Cmd::Eqapo { file, off } => eqapo::run(dev, file, off),

        // The headset itself, over HID.
        Cmd::Info => headset::info(),
        Cmd::Sidetone { level } => headset::sidetone(level),
        Cmd::NoiseReduction { level } => headset::noise_reduction(level),

        // Windows audio devices.
        Cmd::Audio { cmd } => audio::run(cmd),

        // Research and bug reports.
        Cmd::Report { seconds, query } => diag::report(seconds, query),
        Cmd::Monitor { raw } => diag::monitor(raw),
        Cmd::Raw { group, command, values, write, listen, .. } => diag::raw(group.zip(command), values, write, listen),
    }
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
