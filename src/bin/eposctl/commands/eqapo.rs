//! `eqapo`: parametric EQ through Equalizer APO.

use std::io;
use std::path::{Path, PathBuf};

use eposctl::apo::{ApoChannel, DeviceId, layout::BANDS};
use eposctl::audio::{self, Endpoint, Flow};
use eposctl::eqapo;
use eposctl::peq::Peq;

const NOT_INSTALLED: &str = "Equalizer APO is not installed. Get it from \
    https://sourceforge.net/projects/equalizerapo/ and select the EPOS playback device in its Configurator.";

pub fn run(device: Option<DeviceId>, file: Option<PathBuf>, off: bool) -> io::Result<()> {
    let dir = eqapo::config_dir().ok_or_else(|| io::Error::other(NOT_INSTALLED))?;
    let ep = audio::epos_endpoint(Flow::Output)
        .map_err(|e| io::Error::other(e.to_string()))?
        .ok_or_else(|| io::Error::other("no EPOS playback device found"))?;
    let word =
        eqapo::device_word(&ep.id).ok_or_else(|| io::Error::other(format!("unexpected endpoint id {}", ep.id)))?;
    match (file, off) {
        (_, true) => remove(&word),
        (None, false) => show(&dir, &ep),
        (Some(file), false) => write(device, &file, &word, &ep),
    }
}

fn remove(word: &str) -> io::Result<()> {
    let path = eqapo::write(None, word)?;
    println!("removed eposctl's bands ({})", path.display());
    Ok(())
}

fn show(dir: &Path, ep: &Endpoint) -> io::Result<()> {
    println!("Equalizer APO config: {}", dir.display());
    match eqapo::read() {
        Some(p) => {
            println!("eposctl bands for {}:", ep.name);
            print!("{}", p.to_apo_text());
        }
        None => println!("no bands from eposctl"),
    }
    Ok(())
}

fn write(device: Option<DeviceId>, file: &Path, word: &str, ep: &Endpoint) -> io::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let peq = Peq::from_apo_text(&text).map_err(io::Error::other)?;
    let path = eqapo::write(Some(&peq), word)?;
    // Otherwise the headset's own EQ would apply on top.
    ApoChannel::open(device)?.modify(|s| s.set_speaker_eq(&[0.0; BANDS]))?;
    println!("wrote {} bands for {} to {}", peq.bands.len(), ep.name, path.display());
    println!("set the headset's own 9-band EQ flat");
    println!("Equalizer APO only applies them if it is installed on this device (check in its Configurator).");
    Ok(())
}
