//! Commands that talk to the headset over HID.

use std::io;

use eposctl::headset::Headset;

/// `info`: everything the headset reports about itself.
pub fn info() -> io::Result<()> {
    let h = Headset::open()?;
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
    let h = Headset::open()?;
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
