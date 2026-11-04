//! A/B test of APO settings on the microphone signal.
//!
//! Each CONDITION is a comma-separated list of `field=value` assignments from
//! `apo::layout::NAMED` (e.g. `noise_gate_enabled=1,noise_gate_level=1`). For
//! each one the fields are written, then the EPOS microphone is recorded through
//! Windows (so the APO's capture processing applies) and summarised. Conditions
//! are cycled `--rounds` times; the original values are restored at the end.
//!
//!     cargo run --example miclab -- --secs 4 --rounds 2 noise_gate_enabled=0 noise_gate_enabled=1
//!     cargo run --example miclab -- --set noise_gate_enabled=0      (write only, no capture)

use std::time::{Duration, Instant};

use eposctl::apo::{ApoChannel, Settings};

type Cond = Vec<(String, f64)>;

fn parse_cond(s: &str) -> Result<Cond, String> {
    s.split(',')
        .map(|kv| {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("expected field=value, got {kv}"))?;
            let v: f64 = v.parse().map_err(|e| format!("{kv}: {e}"))?;
            Ok((k.to_string(), v))
        })
        .collect()
}

/// `mic_eq=G` sets all nine mic EQ bands to G dB; other names are `layout::NAMED` fields.
fn set(s: &mut Settings, k: &str, v: f64) -> Result<(), String> {
    if k == "mic_eq" {
        // set_mic_eq also switches mic_eq_enabled; keep it as the condition set it.
        let on = s.mic_eq_enabled();
        s.set_mic_eq(&[v as f32; 9]);
        return s.set_named("mic_eq_enabled", on as u8 as f64);
    }
    s.set_named(k, v)
}

fn apply(ch: &ApoChannel, c: &Cond) -> std::io::Result<Settings> {
    let mut err = None;
    let s = ch.modify(|s| {
        for (k, v) in c {
            if let Err(e) = set(s, k, *v) {
                err = Some(e);
            }
        }
    })?;
    match err {
        Some(e) => Err(std::io::Error::other(e)),
        None => Ok(s),
    }
}

#[derive(Default, Clone, Copy)]
struct Stats {
    rms: f64,
    low: f64,
    high: f64,
    peak: f64,
    zero: f64,
    n: usize,
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

/// Record `secs` from the endpoint `id` (first channel).
fn record(id: &str, secs: f64) -> windows::core::Result<(Vec<f32>, u32)> {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let (mut out, mut sr) = (Vec::new(), 48000);
    eposctl::audio::capture(id, || Instant::now() < end, |x, rate| {
        out.extend_from_slice(x);
        sr = rate;
    })?;
    Ok((out, sr))
}

fn analyse(x: &[f32], rate: u32) -> Stats {
    // One-pole filters: low band below ~150 Hz, high band above ~4 kHz.
    let a = |fc: f64| 1.0 - (-2.0 * std::f64::consts::PI * fc / rate as f64).exp();
    let (al, ah) = (a(150.0), a(4000.0));
    let (mut lp, mut lp4) = (0.0f64, 0.0f64);
    let mut s = Stats { n: x.len(), ..Default::default() };
    let (mut sum, mut sl, mut sh, mut zero) = (0.0, 0.0, 0.0, 0usize);
    for &v in x {
        let v = v as f64;
        lp += al * (v - lp);
        lp4 += ah * (v - lp4);
        let hp = v - lp4;
        sum += v * v;
        sl += lp * lp;
        sh += hp * hp;
        s.peak = s.peak.max(v.abs());
        if v.abs() < 1e-6 {
            zero += 1;
        }
    }
    let n = x.len().max(1) as f64;
    s.rms = (sum / n).sqrt();
    s.low = (sl / n).sqrt();
    s.high = (sh / n).sqrt();
    s.zero = zero as f64 / n;
    s
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    let (mut secs, mut rounds, mut mic, mut set_only) = (4.0, 1usize, "EPOS".to_string(), false);
    let mut conds: Vec<(String, Cond)> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--secs" => secs = args.next().ok_or("--secs N")?.parse()?,
            "--rounds" => rounds = args.next().ok_or("--rounds N")?.parse()?,
            "--mic" => mic = args.next().ok_or("--mic NAME")?,
            "--set" => set_only = true,
            _ => conds.push((a.clone(), parse_cond(&a)?)),
        }
    }
    if conds.is_empty() {
        return Err("give at least one condition, e.g. noise_gate_enabled=1".into());
    }
    let ch = ApoChannel::open(None)?;
    if set_only {
        for (_, c) in &conds {
            apply(&ch, c)?;
        }
        println!("written");
        return Ok(());
    }

    // Original values of every field we touch, restored at the end.
    let before = ch.read()?;
    let mut fields: Vec<String> = conds.iter().flat_map(|(_, c)| c.iter().map(|(k, _)| k.clone())).collect();
    fields.sort();
    fields.dedup();
    let mic_eq = before.mic_eq();
    if fields.iter().any(|k| k == "mic_eq") && mic_eq.iter().any(|&g| g != mic_eq[0]) {
        return Err("mic_eq needs all original mic EQ bands equal, so they can be restored".into());
    }
    let original: Cond = fields
        .iter()
        .map(|k| {
            let v = if k == "mic_eq" { Some(mic_eq[0] as f64) } else { before.named(k) };
            Ok((k.clone(), v.ok_or(format!("unknown field {k}"))?))
        })
        .collect::<Result<_, String>>()?;
    let restore = original.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",");
    eprintln!("original: {restore}");
    eprintln!("if interrupted, restore with: cargo run --example miclab -- --set {restore}");

    let ep = eposctl::audio::find(eposctl::audio::Flow::Input, &mic)?;
    eprintln!("recording from {}", ep.name);

    let mut acc = vec![Stats::default(); conds.len()];
    // 20 ms block levels (dBFS) per condition, for gate measurements.
    let mut blocks: Vec<Vec<f64>> = vec![Vec::new(); conds.len()];
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for r in 0..rounds {
            for (i, (label, c)) in conds.iter().enumerate() {
                // Every condition starts from the original values.
                apply(&ch, &original)?;
                apply(&ch, c)?;
                std::thread::sleep(Duration::from_millis(400));
                let (x, rate) = record(&ep.id, secs)?;
                let s = analyse(&x, rate);
                let n = (rate / 50) as usize;
                blocks[i].extend(x.chunks_exact(n).map(|b| {
                    db((b.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / n as f64).sqrt())
                }));
                eprintln!(
                    "round {} {label:<40} rms {:6.1}  low {:6.1}  high {:6.1}  peak {:6.1} dBFS  zero {:4.0}%",
                    r + 1,
                    db(s.rms),
                    db(s.low),
                    db(s.high),
                    db(s.peak),
                    s.zero * 100.0
                );
                let a = &mut acc[i];
                a.rms += s.rms * s.rms;
                a.low += s.low * s.low;
                a.high += s.high * s.high;
                a.peak = a.peak.max(s.peak);
                a.zero += s.zero;
                a.n += 1;
            }
        }
        Ok(())
    })();
    apply(&ch, &original)?;
    eprintln!("restored: {restore}");
    result?;

    println!(
        "\n{:<40} {:>7} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "condition", "rms", "low", "high", "peak", "zero", "open", "p10", "p50", "p90"
    );
    for (((label, _), a), b) in conds.iter().zip(&acc).zip(&mut blocks) {
        let n = a.n.max(1) as f64;
        // "open": share of 20 ms blocks above -90 dBFS (i.e. not gated to silence).
        let open = b.iter().filter(|&&l| l > -90.0).count() as f64 / b.len().max(1) as f64;
        b.sort_by(|x, y| x.total_cmp(y));
        let q = |p: f64| b.get(((b.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(f64::NAN);
        println!(
            "{label:<40} {:7.1} {:7.1} {:7.1} {:7.1} {:5.0}% {:5.0}% {:6.1} {:6.1} {:6.1}",
            db((a.rms / n).sqrt()),
            db((a.low / n).sqrt()),
            db((a.high / n).sqrt()),
            db(a.peak),
            a.zero / n * 100.0,
            open * 100.0,
            q(0.1),
            q(0.5),
            q(0.9)
        );
    }
    Ok(())
}
