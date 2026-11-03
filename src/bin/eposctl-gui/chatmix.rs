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
                let (Some(exe), Ok(cur)) = (s.exe.as_deref(), s.volume()) else { continue };
                let f = if voice_apps.iter().any(|a| a == exe) { voice } else { game };
                let program = format!("{}|{exe}", ep.id);
                seen.insert(s.id.clone());
                let saved = self.saved.get(&program).copied();
                let t = self.tracks.entry(s.id.clone()).or_insert_with(|| match saved {
                    // Same volume we left it at: it is still scaled from that baseline.
                    Some(p) if (cur - p.last).abs() < SAME => p,
                    _ => Track { baseline: cur, last: cur, factor: 1.0 },
                });
                let before = *t;
                if (cur - t.last).abs() > SAME {
                    // Changed in the Windows mixer: that is the new level at the current factor.
                    if t.factor > SAME {
                        t.baseline = (cur / t.factor).min(1.0);
                    } else {
                        t.baseline = cur;
                        t.factor = 1.0;
                    }
                    t.last = cur;
                }
                if (f - t.factor).abs() > 0.001 {
                    let want = (t.baseline * f).clamp(0.0, 1.0);
                    if s.set_volume(want).is_ok() {
                        t.last = want;
                        t.factor = f;
                    }
                }
                if *t != before || saved != Some(*t) {
                    self.saved.insert(program, *t);
                    self.dirty = true;
                }
            }
        }
        self.tracks.retain(|k, _| seen.contains(k));
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
