//! Software game/voice balance, like the ChatMix dial on some headsets.
//!
//! Works through the per-program volumes in the Windows volume mixer. Each stream's
//! volume when first seen is its *baseline*; the balance multiplies it by a factor,
//! so volumes set by hand are kept in proportion. Changing a volume in the mixer
//! makes that the new baseline. Turning the feature off (or quitting) puts the
//! baselines back.
//!
//! Windows remembers per-program volumes across restarts, so baselines are saved
//! to `%APPDATA%\eposctl\chatmix.json`; otherwise an attenuated volume would be
//! taken as the baseline next time and attenuated again.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use eposctl::audio::{self, Flow};
use serde::{Deserialize, Serialize};

use crate::config::ChatMix;

const TICK: Duration = Duration::from_millis(1500);
/// Volume differences below this are rounding, not a change by the user.
const SAME: f32 = 0.01;

/// (game factor, voice factor) for a balance in -1 (all game) ..= 1 (all voice).
pub fn factors(balance: f32) -> (f32, f32) {
    let b = balance.clamp(-1.0, 1.0);
    (1.0 - b.max(0.0), 1.0 + b.min(0.0))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Track {
    baseline: f32,
    /// Volume we last saw or set.
    last: f32,
    /// Factor `last` was set with.
    factor: f32,
}

impl Track {
    fn new(volume: f32) -> Self {
        Self { baseline: volume, last: volume, factor: 1.0 }
    }

    /// The stream is at `cur`. If that is not what we left it at, it was changed
    /// in the Windows mixer: that is the new level at the current factor.
    fn follow_mixer(&mut self, cur: f32) {
        if (cur - self.last).abs() <= SAME {
            return;
        }
        if self.factor > SAME {
            self.baseline = (cur / self.factor).min(1.0);
        } else {
            self.baseline = cur;
            self.factor = 1.0;
        }
        self.last = cur;
    }

    /// Scale to `factor`; `set` applies a volume and says whether it worked.
    fn scale(&mut self, factor: f32, set: impl FnOnce(f32) -> bool) {
        if (factor - self.factor).abs() <= 0.001 {
            return;
        }
        let want = (self.baseline * factor).clamp(0.0, 1.0);
        if set(want) {
            self.last = want;
            self.factor = factor;
        }
    }
}

enum Msg {
    Set(ChatMix),
    Stop(Sender<()>),
}

pub struct Mixer {
    tx: Sender<Msg>,
    current: Option<ChatMix>,
}

impl Mixer {
    pub fn spawn() -> Self {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("eposctl-chatmix".into()).spawn(move || run(rx)).ok();
        Self { tx, current: None }
    }

    /// Hand over the settings; cheap when nothing changed.
    pub fn set(&mut self, m: &ChatMix) {
        if self.current.as_ref() != Some(m) {
            self.current = Some(m.clone());
            let _ = self.tx.send(Msg::Set(m.clone()));
        }
    }

    /// Put the baselines back before the program exits (waits up to a second).
    pub fn shutdown(&self) {
        let (ack, done) = channel();
        if self.tx.send(Msg::Stop(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(1));
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("eposctl").join("chatmix.json"))
}

struct State {
    /// Per stream (session instance id).
    tracks: HashMap<String, Track>,
    /// Per device and program, saved across restarts.
    saved: HashMap<String, Track>,
    dirty: bool,
}

impl State {
    fn load() -> Self {
        let saved = path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { tracks: HashMap::new(), saved, dirty: false }
    }

    fn save(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let Some(p) = path() else { return };
        if self.saved.is_empty() {
            let _ = std::fs::remove_file(p);
        } else if let Ok(json) = serde_json::to_vec_pretty(&self.saved) {
            let _ = std::fs::write(p, json);
        }
    }

    /// Bring every stream to `baseline * factor`. Runs the COM calls on this thread.
    fn tick(&mut self, game: f32, voice: f32, voice_apps: &[String]) {
        let mut seen = HashSet::new();
        for ep in audio::endpoints(Flow::Output).unwrap_or_default() {
            for s in audio::sessions(Some(&ep.id)).unwrap_or_default() {
                let Some(exe) = s.exe.as_deref() else { continue };
                let factor = if voice_apps.iter().any(|a| a == exe) { voice } else { game };
                if self.tick_stream(&format!("{}|{exe}", ep.id), &s, factor) {
                    seen.insert(s.id.clone());
                }
            }
        }
        self.tracks.retain(|k, _| seen.contains(k));
    }

    /// One stream of `program` (`device|exe`). False if its volume is unreadable.
    fn tick_stream(&mut self, program: &str, s: &audio::Session, factor: f32) -> bool {
        let Ok(cur) = s.volume() else { return false };
        let saved = self.saved.get(program).copied();
        let t = self.tracks.entry(s.id.clone()).or_insert_with(|| match saved {
            // Same volume we left it at: it is still scaled from that baseline.
            Some(p) if (cur - p.last).abs() < SAME => p,
            _ => Track::new(cur),
        });
        let before = *t;
        t.follow_mixer(cur);
        t.scale(factor, |v| s.set_volume(v).is_ok());
        if *t != before || saved != Some(*t) {
            self.saved.insert(program.to_owned(), *t);
            self.dirty = true;
        }
        true
    }

    fn restore(&mut self) {
        if self.saved.is_empty() && self.tracks.is_empty() {
            return;
        }
        self.tick(1.0, 1.0, &[]);
        self.tracks.clear();
        self.saved.clear();
        self.dirty = true;
        self.save();
    }
}

fn run(rx: Receiver<Msg>) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut st = State::load();
        let mut mix: Option<ChatMix> = None;
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Msg::Set(m)) => mix = Some(m),
                Ok(Msg::Stop(ack)) => {
                    st.restore();
                    let _ = ack.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    st.restore();
                    return;
                }
            }
            match &mix {
                Some(m) if m.enabled => {
                    let (g, v) = factors(m.balance);
                    st.tick(g, v, &m.voice_apps);
                    st.save();
                }
                // Off, or not configured yet: undo anything left from last time.
                Some(_) => st.restore(),
                None => {}
            }
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::factors;

    #[test]
    fn balance_factors() {
        assert_eq!(factors(0.0), (1.0, 1.0));
        assert_eq!(factors(1.0), (0.0, 1.0));
        assert_eq!(factors(-1.0), (1.0, 0.0));
        assert_eq!(factors(0.4), (0.6, 1.0));
        assert_eq!(factors(-0.25), (1.0, 0.75));
        assert_eq!(factors(3.0), (0.0, 1.0));
    }
}
