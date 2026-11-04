//! Parametric EQ with any number of bands, and the closest the headset's 9-band
//! EQ can get to it.
//!
//! The EPOS APO's EQ is an "accurate graphic equalizer": `EAPO.dll` ships the
//! inverse of a 9x9 band interaction matrix (`SOLVE` below, the 48 kHz table
//! built at RVA `0x2581E0`) and uses it so that the response at each band
//! centre equals the gain set for that band. Inverted, the matrix says how much
//! of a band's gain reaches the other centres: 1/3 one octave away, 7.4 % two
//! octaves away, 1.8 % three away. `epos_db` models the whole curve from that,
//! and `fit` finds the nine gains whose curve is closest to a target over
//! 44 Hz - 20 kHz (least squares on a log-frequency grid). A curve with more
//! detail than nine octave-spaced points cannot be reproduced by this EQ; for
//! that, the same bands can be written to Equalizer APO instead (`crate::eqapo`).

use serde::{Deserialize, Serialize};

use crate::apo::layout::BANDS;
use crate::eqfile::{CENTRES_HZ, Filter, Kind};

/// Inverse band interaction matrix of the APO's EQ at 48 kHz, from `EAPO.dll`:
/// filter gains = `SOLVE` x band gains.
const SOLVE: [[f64; BANDS]; BANDS] = [
    [1.125262, -0.388196, 0.047562, -0.007386, 0.000922, -0.000138, 0.000017, -0.000002, 0.000000],
    [-0.388185, 1.259170, -0.404599, 0.050098, -0.007699, 0.000966, -0.000142, 0.000017, -0.000002],
    [0.047561, -0.404557, 1.261131, -0.404889, 0.050088, -0.007682, 0.000951, -0.000135, 0.000014],
    [-0.007385, 0.050095, -0.404722, 1.260979, -0.404807, 0.049893, -0.007591, 0.000889, -0.000118],
    [0.000922, -0.007693, 0.050074, -0.404135, 1.260176, -0.404455, 0.049112, -0.007257, 0.000714],
    [-0.000137, 0.000965, -0.007659, 0.049835, -0.401759, 1.256942, -0.403102, 0.046094, -0.006466],
    [0.000017, -0.000140, 0.000948, -0.007495, 0.048859, -0.392160, 1.243726, -0.399073, 0.037435],
    [-0.000002, 0.000017, -0.000128, 0.000868, -0.006801, 0.044669, -0.352207, 1.187340, -0.411902],
    [0.000000, -0.000001, 0.000010, -0.000074, 0.000523, -0.003907, 0.026746, -0.193291, 1.069967],
];

/// Normalised response of a band filter `d` octaves from its centre: a smooth
/// fit through the inverted matrix (1, 0.3315, 0.0742, 0.0179 at 0..3 octaves).
fn shape(d: f64) -> f64 {
    (-3.653 * d * d / (1.0 + 2.309 * d.abs())).exp()
}

/// The band interaction matrix (`SOLVE` inverted): entry `[i][j]` is band `j`'s
/// normalised response at centre `i`.
fn interaction() -> &'static [[f64; BANDS]; BANDS] {
    static B: std::sync::OnceLock<[[f64; BANDS]; BANDS]> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        let cols: [[f64; BANDS]; BANDS] = std::array::from_fn(|j| {
            let mut e = [0.0; BANDS];
            e[j] = 1.0;
            solve(SOLVE, e)
        });
        std::array::from_fn(|i| std::array::from_fn(|j| cols[j][i]))
    })
}

/// Band `j`'s normalised response at `f` Hz: `shape`, corrected so that it
/// matches the interaction matrix exactly at every band centre (the top bands
/// are not symmetric because of the Nyquist limit).
fn band_shape(j: usize, f: f64) -> f64 {
    let b = interaction();
    let x = (f / CENTRES_HZ[0] as f64).log2(); // position in bands, 0 = 62.5 Hz
    let corr = |k: usize| b[k][j] / shape(k as f64 - j as f64);
    let c = if x <= 0.0 {
        corr(0)
    } else if x >= (BANDS - 1) as f64 {
        corr(BANDS - 1)
    } else {
        let k = x.floor() as usize;
        let t = x - k as f64;
        corr(k) * (1.0 - t) + corr(k + 1) * t
    };
    c * shape(x - j as f64)
}

/// Modelled response in dB of the headset EQ with band gains `gains` at `f` Hz.
/// Exact at the band centres; smooth in between.
pub fn epos_db(gains: &[f32; BANDS], f: f64) -> f64 {
    (0..BANDS)
        .map(|j| {
            let w: f64 = (0..BANDS).map(|i| SOLVE[j][i] * gains[i] as f64).sum();
            w * band_shape(j, f)
        })
        .sum()
}

/// Log-spaced frequencies used for fitting: half an octave below the lowest
/// band (44 Hz) to 20 kHz. Below that the EQ has no band to follow a target with,
/// and asking it to would only push the 62.5 Hz band too far.
fn grid() -> impl Iterator<Item = f64> {
    const N: usize = 109;
    let lo = CENTRES_HZ[0] as f64 / std::f64::consts::SQRT_2;
    let span = 20000.0 / lo;
    (0..N).map(move |k| lo * span.powf(k as f64 / (N - 1) as f64))
}

/// The nine band gains (unshifted, unclamped) whose modelled curve is closest
/// to `target` (dB at a frequency) over 44 Hz - 20 kHz.
pub fn fit(target: impl Fn(f64) -> f64) -> [f64; BANDS] {
    // Column i: modelled response to 1 dB on band i.
    let rows: Vec<([f64; BANDS], f64)> = grid()
        .map(|f| {
            let col = std::array::from_fn(|i| {
                let mut g = [0.0f32; BANDS];
                g[i] = 1.0;
                epos_db(&g, f)
            });
            (col, target(f))
        })
        .collect();
    // Normal equations with a little damping so that bands outside the target's
    // reach stay near 0.
    let mut ata = [[0.0f64; BANDS]; BANDS];
    let mut atb = [0.0f64; BANDS];
    for (a, t) in &rows {
        for i in 0..BANDS {
            atb[i] += a[i] * t;
            for j in 0..BANDS {
                ata[i][j] += a[i] * a[j];
            }
        }
    }
    for (i, row) in ata.iter_mut().enumerate() {
        row[i] += 1e-3 * rows.len() as f64;
    }
    solve(ata, atb)
}

/// Gaussian elimination with partial pivoting.
fn solve(mut a: [[f64; BANDS]; BANDS], mut b: [f64; BANDS]) -> [f64; BANDS] {
    for c in 0..BANDS {
        let p = (c..BANDS).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap_or(c);
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..BANDS {
            let f = a[r][c] / a[c][c];
            let pivot = a[c];
            for (x, p) in a[r][c..].iter_mut().zip(&pivot[c..]) {
                *x -= f * p;
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0; BANDS];
    for c in (0..BANDS).rev() {
        let s: f64 = (c + 1..BANDS).map(|k| a[c][k] * x[k]).sum();
        x[c] = (b[c] - s) / a[c][c];
    }
    x
}

/// One band of a parametric EQ.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub kind: Kind,
    /// Centre or corner frequency in Hz.
    pub freq: f32,
    /// dB; ignored by filter types without gain.
    pub gain: f32,
    pub q: f32,
    #[serde(default = "on")]
    pub on: bool,
}

fn on() -> bool {
    true
}

impl Band {
    pub fn peak(freq: f32, gain: f32, q: f32) -> Self {
        Self { kind: Kind::Peak, freq, gain, q, on: true }
    }

    fn filter(&self) -> Filter {
        Filter { kind: self.kind, fc: self.freq as f64, gain: self.gain as f64, q: self.q as f64 }
    }

    pub fn db_at(&self, f: f64) -> f64 {
        if self.on { self.filter().db_at(f) } else { 0.0 }
    }
}

/// A parametric EQ: any number of bands.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Peq {
    pub bands: Vec<Band>,
}

impl Peq {
    /// From an Equalizer APO config such as AutoEQ's ParametricEQ.txt (preamp ignored).
    pub fn from_apo_text(text: &str) -> Result<Self, String> {
        let (_, filters) = crate::eqfile::parse_filters(text)?;
        if filters.is_empty() {
            return Err("no Filter lines found".into());
        }
        let bands = filters
            .into_iter()
            .map(|f| Band { kind: f.kind, freq: f.fc as f32, gain: f.gain as f32, q: f.q as f32, on: true })
            .collect();
        Ok(Self { bands })
    }

    /// Exact response in dB at `f` Hz (biquads as in Equalizer APO, at 48 kHz).
    pub fn db_at(&self, f: f64) -> f64 {
        self.bands.iter().map(|b| b.db_at(f)).sum()
    }

    /// The headset EQ setting closest to this curve, shifted to fit -6..+6 dB.
    pub fn to_epos(&self) -> crate::eqfile::Imported {
        crate::eqfile::fit_range(fit(|f| self.db_at(f)), self.bands.iter().filter(|b| b.on).count())
    }

    /// Largest difference between a 9-band setting and this curve over the
    /// range the fit covers, as (dB, Hz).
    pub fn worst_gap(&self, setting: &crate::eqfile::Imported) -> (f64, f64) {
        (0..=60)
            .map(|k| 44.2 * (20000.0f64 / 44.2).powf(k as f64 / 60.0))
            .map(|f| ((epos_db(&setting.gains, f) - setting.shift as f64 - self.db_at(f)).abs(), f))
            .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// Highest point of the curve over 44 Hz - 20 kHz, in dB (for a preamp).
    pub fn peak_db(&self) -> f64 {
        grid().map(|f| self.db_at(f)).fold(f64::MIN, f64::max)
    }

    /// Equalizer APO config text: a preamp that avoids clipping, then the bands.
    pub fn to_apo_text(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let pre = -self.peak_db().max(0.0);
        let _ = writeln!(s, "Preamp: {pre:.1} dB");
        for (n, b) in self.bands.iter().enumerate() {
            let state = if b.on { "ON" } else { "OFF" };
            let gain = if b.kind.has_gain() { format!(" Gain {:.1} dB", b.gain) } else { String::new() };
            let _ =
                writeln!(s, "Filter {}: {state} {} Fc {:.1} Hz{gain} Q {:.3}", n + 1, b.kind.apo_name(), b.freq, b.q);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_hits_the_centres() {
        let g = [3.0, -2.0, 0.0, 4.5, -6.0, 1.0, 2.0, -1.0, 5.0];
        for (i, &c) in CENTRES_HZ.iter().enumerate() {
            let got = epos_db(&g, c as f64);
            assert!((got - g[i] as f64).abs() < 0.05, "band {i}: {got} vs {}", g[i]);
        }
    }

    #[test]
    fn fitting_a_reachable_curve_recovers_it() {
        let g = [2.0f32, 1.0, -1.0, -3.0, 0.0, 2.5, 4.0, 1.0, -2.0];
        let got = fit(|f| epos_db(&g, f));
        for i in 0..BANDS {
            assert!((got[i] - g[i] as f64).abs() < 0.2, "band {i}: {} vs {}", got[i], g[i]);
        }
    }

    #[test]
    fn a_narrow_peak_between_centres_spreads_over_its_neighbours() {
        // 3 dB at 1.41 kHz, between the 1 k and 2 k bands.
        let peq = Peq { bands: vec![Band::peak(1414.0, 3.0, 2.0)] };
        let g = fit(|f| peq.db_at(f));
        assert!(g[4] > 0.3 && g[5] > 0.3, "{g:?}");
        assert!(g[0].abs() < 0.3 && g[8].abs() < 0.3, "{g:?}");
    }

    #[test]
    fn apo_text_round_trips() {
        let peq = Peq {
            bands: vec![
                Band::peak(100.0, 3.0, 1.41),
                Band { kind: Kind::HighShelf, freq: 8000.0, gain: -4.0, q: 0.7, on: true },
                Band { on: false, ..Band::peak(3000.0, 6.0, 4.0) },
            ],
        };
        let text = peq.to_apo_text();
        assert!(text.starts_with("Preamp: -"), "{text}");
        let back = Peq::from_apo_text(&text).unwrap();
        assert_eq!(back.bands.len(), 2, "the OFF band is not an active filter");
        assert!((back.bands[0].freq - 100.0).abs() < 0.1 && (back.bands[1].gain + 4.0).abs() < 0.1);
    }
}
