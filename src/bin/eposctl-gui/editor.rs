//! The DSP settings being edited: the connection to the EPOS driver, undo
//! history and A/B comparison. No UI here; pages call these methods.

use std::time::{Duration, Instant};

use eposctl::apo::{ApoChannel, DeviceId, Settings, layout::BANDS};
use eposctl::audio::KeepRecording;

use crate::config::{Last, Snapshot, Sound};
use crate::history::History;

const RETRY_OPEN: Duration = Duration::from_secs(2);
const RESYNC: Duration = Duration::from_millis(1000);

/// Mic metering we switched on, and what to put back.
struct Metering {
    /// `mic_meters_enabled` before we switched it on.
    original: bool,
    /// The APO only meters the mic while some program records from it.
    _recorder: Option<KeepRecording>,
}

pub struct Editor {
    channel: Option<ApoChannel>,
    /// Last block read from or written to the driver.
    settings: Option<Settings>,
    next_open: Instant,
    next_sync: Instant,
    history: History<Snapshot>,
    /// A/B reference: the sound when a preset was last picked or the driver was found.
    reference: Option<Sound>,
    /// While comparing: the edited sound, put back when the comparison ends.
    comparing: Option<Sound>,
    metering: Option<Metering>,
}

impl Editor {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            channel: None,
            settings: None,
            next_open: now,
            next_sync: now,
            history: History::default(),
            reference: None,
            comparing: None,
            metering: None,
        }
    }

    // ---- connection ----

    pub fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    pub fn device(&self) -> Option<DeviceId> {
        self.channel.as_ref().map(|c| c.device)
    }

    /// Open the driver's channel if it is not open, at most every few seconds.
    /// `restore` is written once on opening.
    pub fn connect(&mut self, restore: Option<&Last>) {
        if self.channel.is_some() || Instant::now() < self.next_open {
            return;
        }
        self.next_open = Instant::now() + RETRY_OPEN;
        let Ok(ch) = ApoChannel::open(None) else { return };
        if let Some(last) = restore {
            let _ = ch.modify(|s| last.write_to(s));
        }
        self.settings = ch.read().ok();
        self.reference = self.sound();
        self.comparing = None;
        self.channel = Some(ch);
    }

    /// Re-read the block so changes from elsewhere (CLI, official app) show up.
    /// Skipped during a drag so the control does not jump.
    pub fn sync(&mut self, dragging: bool) {
        if dragging || Instant::now() < self.next_sync {
            return;
        }
        self.next_sync = Instant::now() + RESYNC;
        let Some(ch) = &self.channel else { return };
        match ch.read() {
            Ok(s) => self.settings = Some(s),
            Err(_) => self.disconnect(),
        }
    }

    fn disconnect(&mut self) {
        self.channel = None;
        self.settings = None;
        self.metering = None;
    }

    /// Write to the driver without touching history. False if it failed (the
    /// channel is then dropped and reopened later).
    fn write(&mut self, f: impl FnOnce(&mut Settings)) -> bool {
        let Some(ch) = &self.channel else { return false };
        match ch.modify(f) {
            Ok(s) => {
                self.settings = Some(s);
                true
            }
            Err(_) => {
                self.disconnect();
                false
            }
        }
    }

    // ---- edits ----

    pub fn sound(&self) -> Option<Sound> {
        self.settings.as_ref().map(Sound::of)
    }

    fn snapshot(&self) -> Option<Snapshot> {
        self.settings.as_ref().map(Snapshot::of)
    }

    /// A user edit: ends any A/B comparison and records an undo step. Returns
    /// whether the driver took it.
    #[must_use]
    pub fn apply(&mut self, f: impl FnOnce(&mut Settings)) -> bool {
        self.stop_compare();
        let before = self.snapshot();
        if !self.write(f) {
            return false;
        }
        if let (Some(b), Some(now)) = (before, self.snapshot())
            && b != now
        {
            self.history.record(b, Instant::now());
        }
        true
    }

    /// Apply a preset: its own undo step, and the new A/B reference.
    #[must_use]
    pub fn apply_preset(&mut self, sound: &Sound) -> bool {
        self.history.close_step();
        let changed = self.apply(|s| sound.write_to(s));
        self.reference = self.sound();
        self.history.close_step();
        changed
    }

    /// Keep the current undo step open while a drag goes on.
    pub fn dragging(&mut self) {
        self.history.extend_step(Instant::now());
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo() && !self.is_comparing()
    }

    #[must_use]
    pub fn undo(&mut self) -> bool {
        self.travel(true)
    }

    #[must_use]
    pub fn redo(&mut self) -> bool {
        self.travel(false)
    }

    /// One step back (`undo`) or forward through the history.
    fn travel(&mut self, back: bool) -> bool {
        self.stop_compare();
        let Some(current) = self.snapshot() else { return false };
        // Moved out so that `restore` can borrow `self` for the write.
        let mut history = std::mem::take(&mut self.history);
        let restore = |target: &Snapshot| self.write(|s| target.write_to(s));
        let done = if back { history.undo(current, restore) } else { history.redo(current, restore) };
        self.history = history;
        done
    }

    // ---- A/B comparison ----

    pub fn is_comparing(&self) -> bool {
        self.comparing.is_some()
    }

    /// The sound differs from the A/B reference, so there is something to compare.
    pub fn differs_from_reference(&self) -> bool {
        match (&self.reference, self.sound()) {
            (Some(r), Some(cur)) => !cur.matches(r),
            _ => false,
        }
    }

    pub fn start_compare(&mut self) {
        let (Some(reference), Some(current)) = (self.reference.clone(), self.sound()) else { return };
        if self.write(|s| reference.write_to(s)) {
            self.comparing = Some(current);
        }
    }

    pub fn stop_compare(&mut self) {
        if let Some(current) = self.comparing.take() {
            self.write(|s| current.write_to(s));
        }
    }

    // ---- microphone meters ----

    pub fn is_metering(&self) -> bool {
        self.metering.is_some()
    }

    /// The APO only updates its level meters while `mic_meters_enabled` is set.
    /// Switch it on while the meters are on screen and put back whatever was
    /// there before. (The official app also switched on the mic EQ and the gate
    /// here, so they only acted while its mic page was open.)
    pub fn set_metering(&mut self, on: bool) {
        let Some(ch) = &self.channel else { return };
        match (on, &self.metering) {
            (true, None) => {
                let Ok(cur) = ch.read() else { return };
                if ch.modify(|s| s.set_mic_meters(true)).is_ok() {
                    self.metering = Some(Metering { original: cur.mic_meters(), _recorder: KeepRecording::epos() });
                }
            }
            (false, Some(m)) => {
                let original = m.original;
                // Stop recording before putting the flag back.
                self.metering = None;
                let _ = ch.modify(|s| s.set_mic_meters(original));
            }
            _ => {}
        }
    }

    /// Current per-band mic levels in dB, while metering.
    pub fn meters(&self) -> Option<[f32; BANDS]> {
        self.metering.as_ref()?;
        let m = self.channel.as_ref()?.read().ok()?.meters();
        m.get(..BANDS)?.try_into().ok()
    }
}
