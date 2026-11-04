//! Import of Equalizer APO and AutoEQ curves into the 9-band EQ.
//!
//! The APO's bands sit on octave centres from 62.5 Hz to 16 kHz: `EAPO.dll`
//! builds exactly this table of nine floats and hands it to its EQ together with
//! the sample rate (static analysis of the 2023 driver; not measured).
//!
//! Parametric filters are evaluated as the biquads Equalizer APO uses (RBJ
//! cookbook at 48 kHz), graphic points are interpolated on a log-frequency axis.
//! The nine band gains are then the least-squares fit of the headset EQ's
//! modelled curve to that target over 44 Hz - 20 kHz (`crate::peq::fit`). The
//! overall level (preamp) does not matter for the shape, so the result is
//! shifted to fit the -6..+6 dB range of the headset and only clipped when the
//! curve is taller than 12 dB.

use std::f64::consts::PI;

use crate::apo::{EQ_MAX_DB, EQ_MIN_DB, layout::BANDS};

/// Band centre frequencies in Hz, lowest first.
pub const CENTRES_HZ: [f32; BANDS] = [62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// Short label for band `i`: "63", "1k", "16k".
pub fn band_label(i: usize) -> String {
    let f = CENTRES_HZ[i];
    if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{}", f.round()) }
}

/// Full name of band `i`: "62.5 Hz", "1 kHz".
pub fn band_freq(i: usize) -> String {
    let f = CENTRES_HZ[i];
    if f >= 1000.0 { format!("{} kHz", f / 1000.0) } else { format!("{f} Hz") }
}

const FS: f64 = 48_000.0;

/// Equalizer APO filter types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
    AllPass,
}

/// One parametric filter, as on an Equalizer APO `Filter:` line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: Kind,
    pub fc: f64,
    pub gain: f64,
    pub q: f64,
}

impl Kind {
    /// The type keyword in Equalizer APO's `Filter:` syntax.
    pub fn apo_name(self) -> &'static str {
        match self {
            Kind::Peak => "PK",
            Kind::LowShelf => "LSC",
            Kind::HighShelf => "HSC",
            Kind::LowPass => "LPQ",
            Kind::HighPass => "HPQ",
            Kind::Notch => "NO",
            Kind::AllPass => "AP",
        }
    }

    /// Whether the filter has a gain parameter.
    pub fn has_gain(self) -> bool {
        matches!(self, Kind::Peak | Kind::LowShelf | Kind::HighShelf)
    }
}

impl Filter {
    /// Gain in dB at `f` Hz.
    pub fn db_at(&self, f: f64) -> f64 {
        let w0 = 2.0 * PI * self.fc.clamp(1.0, FS / 2.0 - 1.0) / FS;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * self.q.max(0.01));
        let a = 10f64.powf(self.gain / 40.0);
        // (b0, b1, b2, a0, a1, a2), RBJ audio EQ cookbook.
        let c = match self.kind {
            Kind::Peak => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
            Kind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            Kind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            Kind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Kind::HighPass => {
                ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            Kind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Kind::AllPass => return 0.0,
        };
        let (b0, b1, b2, a0, a1, a2) = c;
        let w = 2.0 * PI * f / FS;
        // |H(e^jw)|^2 with z^-1 = e^-jw.
        let mag2 = |c0: f64, c1: f64, c2: f64| {
            let re = c0 + c1 * w.cos() + c2 * (2.0 * w).cos();
            let im = -(c1 * w.sin() + c2 * (2.0 * w).sin());
            re * re + im * im
        };
        let h = mag2(b0, b1, b2) / mag2(a0, a1, a2);
        10.0 * h.max(1e-12).log10()
    }
}

/// The result of an import.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    /// Band gains, already shifted and clamped to the headset's range.
    pub gains: [f32; BANDS],
    /// dB added to every band to fit the range (0 when it fitted as is).
    pub shift: f32,
    /// Some bands had to be clipped because the curve spans more than 12 dB.
    pub clipped: bool,
    /// Number of filters and graphic EQ points that were used.
    pub used: usize,
}

/// Parse an Equalizer APO config (`Filter:` / `GraphicEQ:` lines, as written by
/// AutoEQ's ParametricEQ.txt and GraphicEQ.txt) or an AutoEQ CSV with an
/// `equalization` column.
pub fn parse(text: &str) -> Result<Imported, String> {
    let text = text.trim_start_matches('\u{feff}');
    let (raw, used) = if looks_like_csv(text) { parse_csv(text)? } else { parse_apo(text)? };
    if used == 0 {
        return Err("no Equalizer APO filters, GraphicEQ points or AutoEQ columns found".into());
    }
    Ok(fit_range(raw, used))
}

/// Shift raw band gains into the headset's range (clipping only what still
/// does not fit) and round them to 0.1 dB.
pub fn fit_range(raw: [f64; BANDS], used: usize) -> Imported {
    let (min, max) = raw.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let (lo, hi) = (EQ_MIN_DB as f64, EQ_MAX_DB as f64);
    let shift = if max - min > hi - lo {
        -(max + min) / 2.0
    } else if max > hi {
        hi - max
    } else if min < lo {
        lo - min
    } else {
        0.0
    };
    let mut clipped = false;
    let gains = raw.map(|v| {
        let g = v + shift;
        if g > hi + 0.05 || g < lo - 0.05 {
            clipped = true;
        }
        ((g.clamp(lo, hi) * 10.0).round() / 10.0) as f32
    });
    Imported { gains, shift: ((shift * 10.0).round() / 10.0) as f32, clipped, used }
}

fn looks_like_csv(text: &str) -> bool {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.to_ascii_lowercase().starts_with("frequency") && l.contains(','))
}

fn parse_csv(text: &str) -> Result<([f64; BANDS], usize), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> =
        lines.next().unwrap_or_default().split(',').map(|h| h.trim().to_ascii_lowercase()).collect();
    let col = header
        .iter()
        .position(|h| h == "equalization")
        .or(if header.len() == 2 { Some(1) } else { None })
        .ok_or("the CSV has no \"equalization\" column")?;
    let mut points = Vec::new();
    for l in lines {
        let cells: Vec<&str> = l.split(',').collect();
        let (Some(f), Some(g)) = (cells.first(), cells.get(col)) else { continue };
        if let (Ok(f), Ok(g)) = (f.trim().parse::<f64>(), g.trim().parse::<f64>()) {
            points.push((f, g));
        }
    }
    let n = points.len();
    let points = sorted(points);
    Ok((crate::peq::fit(|f| interp(&points, f)), n))
}

fn parse_apo(text: &str) -> Result<([f64; BANDS], usize), String> {
    let mut filters = Vec::new();
    let mut graphics: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut used = 0;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((cmd, rest)) = line.split_once(':') else { continue };
        let cmd = cmd.trim().to_ascii_lowercase();
        if cmd == "graphiceq" {
            let mut points = Vec::new();
            for pair in rest.split(';') {
                let mut it = pair.split_whitespace();
                if let (Some(f), Some(g)) = (it.next(), it.next())
                    && let (Ok(f), Ok(g)) = (f.parse::<f64>(), g.parse::<f64>())
                {
                    points.push((f, g));
                }
            }
            used += points.len();
            graphics.push(sorted(points));
        } else if (cmd == "filter" || cmd.starts_with("filter "))
            && let Some(f) = filter(rest)?
        {
            used += 1;
            filters.push(f);
        }
    }
    let target =
        |f: f64| filters.iter().map(|x| x.db_at(f)).sum::<f64>() + graphics.iter().map(|g| interp(g, f)).sum::<f64>();
    Ok((crate::peq::fit(target), used))
}

/// The `Preamp:` value (summed) and the active `Filter:` lines of an Equalizer
/// APO config. `GraphicEQ:` lines are not filters and are left out.
pub fn parse_filters(text: &str) -> Result<(f64, Vec<Filter>), String> {
    let mut preamp = 0.0;
    let mut out = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((cmd, rest)) = line.split_once(':') else { continue };
        let cmd = cmd.trim().to_ascii_lowercase();
        if cmd == "preamp" {
            preamp += rest.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        } else if (cmd == "filter" || cmd.starts_with("filter "))
            && let Some(f) = filter(rest)?
        {
            out.push(f);
        }
    }
    Ok((preamp, out))
}

/// One `Filter:` line after the colon. `Ok(None)` for a filter that is off.
fn filter(rest: &str) -> Result<Option<Filter>, String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("on")) {
        i = 1;
    } else if words.first().is_some_and(|w| w.eq_ignore_ascii_case("off")) {
        return Ok(None);
    }
    let Some(ty) = words.get(i) else { return Ok(None) };
    let kind = match ty.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" | "MODAL" => Kind::Peak,
        "LS" | "LSC" | "LSQ" => Kind::LowShelf,
        "HS" | "HSC" | "HSQ" => Kind::HighShelf,
        "LP" | "LPQ" => Kind::LowPass,
        "HP" | "HPQ" => Kind::HighPass,
        "NO" => Kind::Notch,
        "AP" => Kind::AllPass,
        "NONE" => return Ok(None),
        other => return Err(format!("unsupported filter type {other}")),
    };
    let num = |key: &str| -> Option<f64> {
        let k = words.iter().position(|w| w.eq_ignore_ascii_case(key))?;
        words.get(k + 1)?.parse().ok()
    };
    let fc = num("fc").ok_or_else(|| format!("filter without a frequency: {}", rest.trim()))?;
    let gain = num("gain").unwrap_or(0.0);
    let q = if let Some(q) = num("q") {
        q
    } else if let Some(bw) = words.iter().position(|w| w.eq_ignore_ascii_case("bw")).and_then(|k| {
        // "BW Oct 1.0"
        words.get(k + 2)?.parse::<f64>().ok()
    }) {
        let n = 2f64.powf(bw);
        n.sqrt() / (n - 1.0)
    } else {
        match kind {
            Kind::Peak | Kind::Notch => return Err(format!("filter without Q: {}", rest.trim())),
            // Equalizer APO's plain shelves: 12 dB/oct, i.e. Q = 1/sqrt(2).
            _ => std::f64::consts::FRAC_1_SQRT_2,
        }
    };
    Ok(Some(Filter { kind, fc, gain, q }))
}

/// Valid graphic points, sorted by frequency.
fn sorted(mut points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    points.retain(|(f, g)| *f > 0.0 && f.is_finite() && g.is_finite());
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
}

/// A graphic curve's value at `c` Hz, interpolated on log frequency and held
/// flat beyond its ends.
fn interp(points: &[(f64, f64)], c: f64) -> f64 {
    match points.iter().position(|(f, _)| *f >= c) {
        None => points.last().map_or(0.0, |p| p.1),
        Some(0) => points[0].1,
        Some(k) => {
            let ((f0, g0), (f1, g1)) = (points[k - 1], points[k]);
            let t = (c.ln() - f0.ln()) / (f1.ln() - f0.ln());
            g0 + (g1 - g0) * t
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_filter_hits_its_band() {
        let f = Filter { kind: Kind::Peak, fc: 1000.0, gain: 4.0, q: 1.41 };
        assert!((f.db_at(1000.0) - 4.0).abs() < 0.01);
        assert!(f.db_at(16000.0).abs() < 0.2);
    }

    #[test]
    fn shelves_reach_their_gain() {
        let ls = Filter { kind: Kind::LowShelf, fc: 105.0, gain: 5.0, q: 0.7 };
        assert!((ls.db_at(20.0) - 5.0).abs() < 0.3);
        assert!(ls.db_at(8000.0).abs() < 0.1);
        let hs = Filter { kind: Kind::HighShelf, fc: 10000.0, gain: -3.0, q: 0.7 };
        assert!((hs.db_at(20000.0) + 3.0).abs() < 0.5);
    }

    #[test]
    fn autoeq_parametric() {
        let text = "Preamp: -6.4 dB\n\
                    Filter 1: ON LSC Fc 105 Hz Gain 5.0 dB Q 0.70\n\
                    Filter 2: ON PK Fc 1000 Hz Gain 3.0 dB Q 1.41\n\
                    Filter 3: OFF PK Fc 4000 Hz Gain 9.0 dB Q 1.41\n\
                    # a comment\n";
        let r = parse(text).unwrap();
        assert_eq!(r.used, 2);
        // The least-squares fit may overshoot the 5 dB shelf slightly at the low end.
        assert!(r.shift.abs() <= 0.5, "{r:?}");
        assert!(r.gains[0] > 3.0, "{:?}", r.gains);
        // Q 1.41 is narrower than a headset band, so the fit lands a little under 3 dB.
        assert!((r.gains[4] - 3.0).abs() < 0.3, "{:?}", r.gains);
        assert!(r.gains[8].abs() < 0.2, "{:?}", r.gains);
    }

    #[test]
    fn graphic_eq_is_interpolated_and_fitted() {
        // 0 dB up to 500 Hz, then a straight -8 dB, plus a 10 dB bass bump: 18 dB tall.
        let text = "GraphicEQ: 20 10; 62.5 10; 125 0; 500 0; 1000 -8; 20000 -8";
        let r = parse(text).unwrap();
        assert_eq!(r.used, 6);
        assert!(r.clipped);
        assert!((-2.0..=-0.5).contains(&r.shift), "{r:?}");
        assert_eq!(r.gains[0], 6.0);
        // 250 Hz sits on the 0 dB plateau, pulled down a little by the -8 dB step
        // one octave above it.
        assert!((-3.0..=-0.5).contains(&r.gains[2]), "{r:?}");
        assert_eq!(r.gains[8], -6.0);
        // 250 Hz lies between 125 and 500 at 0 dB.
        let r = parse("GraphicEQ: 100 -2; 1000 -8").unwrap();
        assert!(!r.clipped);
        // -8 -> -6, plus a little for the fit's overshoot at the step.
        assert!((2.0..=2.6).contains(&r.shift), "{r:?}");
        assert!((r.gains[0] - r.shift).abs() < 2.6, "{r:?}");
        assert_eq!(r.gains[8], -6.0);
    }

    #[test]
    fn autoeq_csv() {
        let text = "frequency,raw,error,smoothed,error_smoothed,equalization,parametric_eq,fixed_band_eq,equalized_raw,equalized_smoothed,target\n\
                    20.0,1,1,1,1,2.0,0,0,0,0,0\n\
                    1000.0,1,1,1,1,0.0,0,0,0,0,0\n\
                    20000.0,1,1,1,1,-2.0,0,0,0,0,0\n";
        let r = parse(text).unwrap();
        assert_eq!(r.used, 3);
        assert_eq!(r.gains[4], 0.0);
        assert!(r.gains[0] > 1.0 && r.gains[8] < -1.0);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse("hello world").is_err());
        assert!(parse("Filter 1: ON XX Fc 100 Hz").is_err());
        assert!(parse("Filter 1: ON PK Fc 100 Hz Gain 3 dB").is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(band_label(0), "63");
        assert_eq!(band_label(4), "1k");
        assert_eq!(band_label(8), "16k");
        assert_eq!(band_freq(0), "62.5 Hz");
        assert_eq!(band_freq(4), "1 kHz");
    }
}
