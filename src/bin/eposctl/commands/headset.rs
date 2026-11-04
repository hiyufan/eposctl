//! Commands that talk to the headset: over HID (H3PRO and its Capricorn
//! protocol), or over the dongle's serial port for the GSP 370.

use std::io;

use eposctl::gsp370::{self, Gsp370};
use eposctl::headset::Headset;

/// `info`: everything the headset reports about itself.
pub fn info() -> io::Result<()> {
    let h = match Headset::open() {
        Ok(h) => h,
        Err(e) => return gsp370_info().map_err(|_| e),
    };
    let show = |label: &str, r: io::Result<String>| match r {
        Ok(v) => println!("{label:<10} {v}"),
        Err(e) => println!("{label:<10} ({e})"),
    };
    show("product", h.product_name());
    show("serial", h.serial_number());
    show("firmware", h.firmware());
    show("battery", h.battery().map(|b| format!("{b}%")));
    show("sidetone", h.sidetone().map(|s| s.to_string()));
    show("noise red.", h.noise_reduction().map(nr_name));
    Ok(())
}

/// `sidetone [LEVEL]`
pub fn sidetone(level: Option<u16>) -> io::Result<()> {
    let h = match Headset::open() {
        Ok(h) => h,
        Err(e) => return gsp370_sidetone(level).map_err(|g| if g.kind() == io::ErrorKind::NotFound { e } else { g }),
    };
    if let Some(l) = level {
        check("sidetone", l, h.set_sidetone(l)?)?;
    }
    println!("sidetone {}", h.sidetone()?);
    Ok(())
}

/// `noise-reduction [LEVEL]`
pub fn noise_reduction(level: Option<u16>) -> io::Result<()> {
    let h = Headset::open()?;
    if let Some(l) = level {
        check("noise reduction", l, h.set_noise_reduction(l)?)?;
    }
    println!("noise reduction {}", nr_name(h.noise_reduction()?));
    Ok(())
}

/// `info` for a GSP 370 (no HID settings collection; serial port instead).
fn gsp370_info() -> io::Result<()> {
    let g = Gsp370::open()?;
    println!("{:<10} EPOS GSP 370 (experimental support)", "product");
    match g.battery() {
        Ok(b) => println!("{:<10} {b}%", "battery"),
        Err(e) => println!("{:<10} ({e})", "battery"),
    }
    Ok(())
}

/// `sidetone LEVEL` for a GSP 370: LEVEL 0-255 as loudness (0 = off). It has
/// no known way to read the level back.
fn gsp370_sidetone(level: Option<u16>) -> io::Result<()> {
    let g = Gsp370::open()?;
    let Some(l) = level else {
        return Err(io::Error::other("the GSP 370 cannot report its sidetone level; give one to set (0-255)"));
    };
    let device = gsp370::sidetone_level(f32::from(l.min(255)) / 255.0);
    g.set_sidetone(device)?;
    println!("sidetone set (device level {device})");
    Ok(())
}

/// A setter's status word: 0 means the headset took the value.
fn check(what: &str, value: u16, status: u16) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("headset rejected {what} {value} (status {status})")))
    }
}

fn nr_name(level: u16) -> String {
    match level {
        0 => "0 (off)".into(),
        1 => "1 (medium)".into(),
        2 => "2 (strong)".into(),
        l => format!("{l} (unknown)"),
    }
}
