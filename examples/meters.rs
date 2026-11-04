//! Guided capture to work out which APO level meter is which.
//!
//! Samples the 15 meters at 20 Hz through three phases (silence, speaking,
//! playback) and prints per-phase average and peak for each channel.
//! The APO only updates the meters while `mic_meters_enabled` is set and a
//! program is recording, so the flag is switched on for the capture and
//! restored afterwards (`--reset` clears it if an interrupted run left it on).
//! Keep a recording app open on the headset mic meanwhile.

use std::time::{Duration, Instant};

use eposctl::apo::{ApoChannel, layout::METER_COUNT};

const PHASES: [(&str, u64); 3] = [
    ("保持安静，不说话，也不要播放声音 / Stay silent, no playback", 6),
    ("对着麦克风持续说话或数数 / Talk into the mic continuously", 8),
    ("停止说话，播放一段音乐或视频 / Stop talking, play some music", 8),
];

fn main() -> std::io::Result<()> {
    let ch = ApoChannel::open(None)?;
    if std::env::args().any(|a| a == "--reset") {
        // Clear the flags, e.g. after an interrupted capture left them on.
        ch.modify(|s| s.set_mic_meters(false))?;
        println!("meter flag cleared");
        return Ok(());
    }
    let original = ch.read()?.mic_meters();
    println!("device {}  meter flag before: {original}", ch.device);
    ch.modify(|s| s.set_mic_meters(true))?;
    let result = capture(&ch);
    ch.modify(|s| s.set_mic_meters(original))?;
    println!("meter flag restored to {original}");
    result
}

fn capture(ch: &ApoChannel) -> std::io::Result<()> {
    let mut results = Vec::new();
    for (i, (what, secs)) in PHASES.iter().enumerate() {
        eprintln!("\n[{}/3] {what}  ({secs} s) — 3 秒后开始 / starting in 3 s", i + 1);
        std::thread::sleep(Duration::from_secs(3));
        eprintln!("      开始 / go");
        let end = Instant::now() + Duration::from_secs(*secs);
        let mut sum = [0f64; METER_COUNT];
        let mut peak = [f32::MIN; METER_COUNT];
        let mut n = 0;
        while Instant::now() < end {
            let m = ch.read()?.meters();
            for k in 0..METER_COUNT {
                sum[k] += m[k] as f64;
                peak[k] = peak[k].max(m[k]);
            }
            n += 1;
            std::thread::sleep(Duration::from_millis(50));
        }
        let avg: Vec<f32> = sum.iter().map(|s| (*s / n as f64) as f32).collect();
        results.push((avg, peak));
        eprintln!("      完成 / done");
    }

    println!("\nmeter  silence avg/peak   speech avg/peak    music avg/peak");
    for k in 0..METER_COUNT {
        print!("{k:>5}");
        for (avg, peak) in &results {
            print!("   {:>7.1} {:>7.1}", avg[k], peak[k]);
        }
        println!();
    }
    Ok(())
}
