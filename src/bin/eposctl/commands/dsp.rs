//! Commands on the EPOS driver's DSP settings (the APO settings channel).

use std::io;
use std::path::Path;

use eposctl::apo::{self, ApoChannel, DeviceId, Settings, layout::BANDS};
use eposctl::eqfile::{self, Imported};
use eposctl::peq::Peq;
use eposctl::presets;

/// `list`: every device with an APO settings channel.
pub fn list() -> io::Result<()> {
    let found = apo::discover()?;
    if found.is_empty() {
        println!("no EPOS APO channels found");
    }
    for (id, name) in found {
        println!("{id}  {name}");
    }
    Ok(())
}

/// `status`: print the current settings.
pub fn status(device: Option<DeviceId>, raw: bool) -> io::Result<()> {
    let ch = ApoChannel::open(device)?;
    print_status(ch.device, &ch.read()?, raw);
    Ok(())
}

/// Change the settings with `f`, then print what the driver holds now.
pub fn modify(device: Option<DeviceId>, f: impl FnOnce(&mut Settings)) -> io::Result<()> {
    let ch = ApoChannel::open(device)?;
    let written = ch.modify(f)?;
    print_status(ch.device, &written, false);
    Ok(())
}

/// `preset`: apply a factory or game preset.
pub fn preset(device: Option<DeviceId>, name: &str) -> io::Result<()> {
    let p = presets::find(name).expect("validated by clap");
    modify(device, |s| p.apply(s))
}

/// The band count is enforced by clap, so this cannot fail.
pub fn bands(v: &[f32]) -> [f32; BANDS] {
    v.try_into().expect("clap enforces the band count")
}

/// `eq-import`: load an EQ file into the speaker or microphone EQ.
pub fn eq_import(device: Option<DeviceId>, file: &Path, mic: bool, dry_run: bool) -> io::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let imp = eqfile::parse(&text).map_err(io::Error::other)?;
    print_import(&imp, &text);
    if dry_run {
        return Ok(());
    }
    let g = imp.gains;
    modify(device, |s| if mic { s.set_mic_eq(&g) } else { s.set_speaker_eq(&g) })
}

fn print_import(imp: &Imported, text: &str) {
    for (i, g) in imp.gains.iter().enumerate() {
        println!("{:>8}  {g:+.1} dB", eqfile::band_freq(i));
    }
    if let Ok(peq) = Peq::from_apo_text(text) {
        // How far the 9-band curve stays from the file's own curve.
        let (db, hz) = peq.worst_gap(imp);
        println!(
            "largest difference from the file's curve: {db:.1} dB at {hz:.0} Hz ({} filters on 9 bands)",
            peq.bands.len()
        );
    }
    if imp.shift != 0.0 {
        println!("shifted by {:+.1} dB to fit -6..+6 dB", imp.shift);
    }
    if imp.clipped {
        println!("the curve is taller than 12 dB; the ends were clipped");
    }
}

fn fmt_bands(b: &[f32]) -> String {
    b.iter().map(|g| format!("{g:+5.2}")).collect::<Vec<_>>().join(" ")
}

/// `"  (disabled)"` after a value whose stage is off.
fn disabled(enabled: bool) -> &'static str {
    if enabled { "" } else { "  (disabled)" }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn print_status(dev: DeviceId, s: &Settings, raw: bool) {
    let pct = |v: f32| v * 100.0;
    println!("device       {dev}");
    println!("header       {}", s.header());
    println!("speaker EQ   [{}] dB{}", fmt_bands(&s.speaker_eq()), disabled(s.speaker_eq_enabled()));
    println!("mic EQ       [{}] dB{}", fmt_bands(&s.mic_eq()), disabled(s.mic_eq_enabled()));
    println!("surround 3D  {}", on_off(s.surround()));
    println!("reverb       {:.1}%{}", pct(s.reverb()), disabled(s.reverb_enabled()));
    println!("noise gate   {:.1}%{}", pct(s.noise_gate()), disabled(s.noise_gate_enabled()));
    println!("mic meters   {}", on_off(s.mic_meters()));
    if raw {
        for (name, v) in s.named_ints() {
            println!("  {name:<30} {v}");
        }
    }
    println!("meters dB    [{}]", s.meters().iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
    if raw {
        print_hex_dump(s.raw());
    }
}

/// Non-zero 16-byte rows of the raw settings block.
fn print_hex_dump(bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(16).enumerate() {
        if chunk.iter().any(|&b| b != 0) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            println!("{:04x}  {}", i * 16, hex.join(" "));
        }
    }
}
