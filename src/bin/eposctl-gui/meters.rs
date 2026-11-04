//! Smoothed microphone meters for display: fast attack, steady fall, peak hold.

use std::time::{Duration, Instant};

use eposctl::apo::layout::BANDS;

use crate::eq::METER_FLOOR_DB;

pub struct Meters {
    pub bands: [f32; BANDS],
    pub level: f32,
    pub peak: f32,
    peak_at: Instant,
}

impl Default for Meters {
    fn default() -> Self {
        let f = METER_FLOOR_DB;
        Self { bands: [f; BANDS], level: f, peak: f, peak_at: Instant::now() }
    }
}

impl Meters {
    const FALL_DB_PER_S: f32 = 30.0;
    const PEAK_HOLD: Duration = Duration::from_millis(1500);

    /// Feed raw band levels in dB; `dt` is the time since the last update.
    pub fn update(&mut self, raw: &[f32; BANDS], dt: f32) {
        let f = METER_FLOOR_DB;
        let follow = |d: &mut f32, t: f32| {
            *d = if t >= *d { t } else { (*d - Self::FALL_DB_PER_S * dt).max(t) };
        };
        for (d, &r) in self.bands.iter_mut().zip(raw) {
            follow(d, r.clamp(f, 0.0));
        }
        // Overall level: power sum of the bands.
        let p: f32 = raw.iter().map(|&r| 10f32.powf(r.clamp(f, 0.0) / 10.0)).sum();
        let total = (10.0 * p.log10()).clamp(f, 0.0);
        follow(&mut self.level, total);
        if total >= self.peak || self.peak_at.elapsed() > Self::PEAK_HOLD {
            self.peak = total;
            self.peak_at = Instant::now();
        }
    }
}
