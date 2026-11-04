//! `audio`: Windows playback and recording devices.

use std::io;

use eposctl::audio::{self, Flow};

use crate::args::AudioCmd;

pub fn run(cmd: Option<AudioCmd>) -> io::Result<()> {
    match cmd {
        None => list(),
        Some(AudioCmd::Default { device, input }) => set_default(&device, input),
        Some(AudioCmd::Sessions) => sessions(),
    }
}

fn win(e: windows::core::Error) -> io::Error {
    io::Error::other(e.to_string())
}

/// Every device, `*` marking the defaults.
fn list() -> io::Result<()> {
    for (flow, title) in [(Flow::Output, "playback"), (Flow::Input, "recording")] {
        let default = audio::default_id(flow).map_err(win)?;
        println!("{title}");
        for ep in audio::endpoints(flow).map_err(win)? {
            let mark = if default.as_deref() == Some(ep.id.as_str()) { "*" } else { " " };
            println!("  {mark} {}", ep.name);
        }
    }
    Ok(())
}

fn set_default(device: &str, input: bool) -> io::Result<()> {
    let flow = if input { Flow::Input } else { Flow::Output };
    let ep = audio::find(flow, device).map_err(io::Error::other)?;
    audio::set_default(&ep.id).map_err(win)?;
    println!("default {}: {}", if input { "recording" } else { "playback" }, ep.name);
    Ok(())
}

/// Programs playing on the default playback device, with their volume.
fn sessions() -> io::Result<()> {
    for s in audio::sessions(None).map_err(win)? {
        let vol = s.volume().map(|v| format!("{:3.0}%", v * 100.0)).unwrap_or_else(|_| "  ?".into());
        println!("{vol}  {}", s.exe.as_deref().unwrap_or("?"));
    }
    Ok(())
}
