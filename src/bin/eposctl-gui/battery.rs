//! Battery runtime estimate from how fast the level drops.
//!
//! Only time with the headset on counts, so turning it off overnight does not
//! make the battery look better than it is. The headset may report coarse steps,
//! so the rate is measured from the first drop seen to the latest one, and only
//! once the level has fallen a few percent over a reasonable stretch of time.

use std::time::{Duration, Instant};

/// A rate needs at least this much drop and on-time to be trusted.
const MIN_DROP: u8 = 3;
const MIN_TIME: Duration = Duration::from_secs(20 * 60);
/// Longer gaps between readings (sleep, hidden window not woken) are not counted.
const MAX_TICK: Duration = Duration::from_secs(5 * 60);
/// A rise of more than this is charging; smaller bumps are reading noise.
const NOISE: u8 = 2;

#[derive(Default)]
pub struct Tracker {
    on_time: Duration,
    last_tick: Option<Instant>,
    /// Lowest level seen in this discharge, i.e. the level we compare against.
    level: Option<u8>,
    /// Level and on-time of the first drop in this discharge.
    anchor: Option<(u8, Duration)>,
    charging: bool,
}

impl Tracker {
    /// Feed the current reading (`None` while the headset is off or unknown).
    /// Returns a fresh drain rate in percent per hour when one can be computed.
    pub fn observe(&mut self, percent: Option<u8>, now: Instant) -> Option<f32> {
        let Some(p) = percent else {
            self.last_tick = None;
            return None;
        };
        if let Some(t) = self.last_tick {
            self.on_time += now.saturating_duration_since(t).min(MAX_TICK);
        }
        self.last_tick = Some(now);

        let Some(level) = self.level else {
            self.level = Some(p);
            return None;
        };
        if p > level.saturating_add(NOISE) {
            // Charging: start a new discharge from here.
            self.charging = true;
            self.level = Some(p);
            self.anchor = None;
            return None;
        }
        if p >= level {
            return None;
        }
        self.charging = false;
        self.level = Some(p);
        let Some((start, at)) = self.anchor else {
            self.anchor = Some((p, self.on_time));
            return None;
        };
        let (drop, time) = (start - p, self.on_time - at);
        (drop >= MIN_DROP && time >= MIN_TIME).then(|| drop as f32 / time.as_secs_f32() * 3600.0)
    }

    /// The level went up recently, so a runtime estimate would be wrong.
    pub fn charging(&self) -> bool {
        self.charging
    }
}

/// Low-battery warnings: once per threshold as the level drains, re-armed once
/// it has recovered (charged).
#[derive(Default)]
pub struct LowAlarm {
    /// Lowest threshold already warned about.
    warned: Option<u8>,
}

impl LowAlarm {
    const THRESHOLDS: [u8; 3] = [5, 10, 20];
    const REARM_ABOVE: u8 = 25;

    /// Feed a reading; true when it crosses a threshold not yet warned about.
    pub fn check(&mut self, percent: u8) -> bool {
        if percent > Self::REARM_ABOVE {
            self.warned = None;
            return false;
        }
        let Some(level) = Self::THRESHOLDS.into_iter().find(|&t| percent <= t) else { return false };
        if self.warned.is_some_and(|w| level >= w) {
            return false;
        }
        self.warned = Some(level);
        true
    }
}

/// Hours left at `rate` percent per hour.
pub fn hours_left(percent: u8, rate: f32) -> Option<f32> {
    (rate > 0.05 && rate.is_finite()).then(|| percent as f32 / rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_alarm_warns_once_per_threshold() {
        let mut a = LowAlarm::default();
        let warned: Vec<u8> = [30, 20, 19, 12, 10, 9, 5, 4, 30, 20].into_iter().filter(|&p| a.check(p)).collect();
        assert_eq!(warned, [20, 10, 5, 20]);
    }

    /// Feed one reading per minute; returns the last rate computed.
    fn run(t: &mut Tracker, start: Instant, minute0: u64, levels: &[u8]) -> Option<f32> {
        let mut rate = None;
        for (i, &l) in levels.iter().enumerate() {
            let now = start + Duration::from_secs((minute0 + i as u64) * 60);
            rate = t.observe(Some(l), now).or(rate);
        }
        rate
    }

    #[test]
    fn rate_from_steady_drain() {
        // 1 % every 6 minutes = 10 %/h.
        let levels: Vec<u8> = (0..60).map(|m| 80 - (m / 6) as u8).collect();
        let mut t = Tracker::default();
        let rate = run(&mut t, Instant::now(), 0, &levels).unwrap();
        assert!((rate - 10.0).abs() < 0.5, "{rate}");
        assert!((hours_left(50, rate).unwrap() - 5.0).abs() < 0.3);
    }

    #[test]
    fn off_time_does_not_count() {
        let start = Instant::now();
        let mut t = Tracker::default();
        run(&mut t, start, 0, &[80, 80, 80, 80, 80, 80, 79]);
        t.observe(None, start + Duration::from_secs(7 * 60)); // headset off for 8 hours
        let levels: Vec<u8> = (0..60).map(|m| 79 - (m / 6) as u8).collect();
        let rate = run(&mut t, start, 8 * 60 + 7, &levels).unwrap();
        assert!((rate - 10.0).abs() < 1.0, "{rate}");
    }

    #[test]
    fn charging_resets_and_noise_does_not() {
        let mut t = Tracker::default();
        let s = Instant::now();
        run(&mut t, s, 0, &[60, 59, 60, 58]);
        assert!(!t.charging(), "a +1 bump is noise");
        run(&mut t, s, 4, &[70]);
        assert!(t.charging());
        assert_eq!(t.anchor, None);
    }
}
