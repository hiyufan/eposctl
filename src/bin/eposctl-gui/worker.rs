//! Background thread that owns the HID connection to the dongle.
//!
//! It keeps `DevState` up to date from responses and notifications, polls the
//! battery, and executes commands from the UI. Every update wakes the UI.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eposctl::capricorn::Message;
use eposctl::headset::Headset;

#[derive(Clone, Debug, Default)]
pub struct DevState {
    /// The dongle's vendor HID interface is open.
    pub dongle: bool,
    /// The headset answered or reported a link; `None` until known.
    pub linked: Option<bool>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub battery: Option<u8>,
    /// Raw device value: 0xFF = quietest, 0x00 = loudest.
    pub sidetone: Option<u16>,
    /// Microphone noise reduction in the headset, 0..=2; `None` if unsupported.
    pub noise_reduction: Option<u16>,
    pub anc: Option<bool>,
}

pub enum Cmd {
    SetSidetone(u16),
    SetNoiseReduction(u16),
    Refresh,
}

pub type Shared = Arc<Mutex<DevState>>;

const BATTERY_EVERY: Duration = Duration::from_secs(60);
const RECONNECT_EVERY: Duration = Duration::from_secs(2);

pub fn spawn(wake: impl Fn() + Send + 'static) -> (Shared, Sender<Cmd>) {
    let state: Shared = Default::default();
    let (tx, rx) = channel();
    let st = state.clone();
    std::thread::Builder::new().name("eposctl-hid".into()).spawn(move || run(st, rx, wake)).expect("spawn HID thread");
    (state, tx)
}

fn update(state: &Shared, wake: &impl Fn(), f: impl FnOnce(&mut DevState)) {
    f(&mut state.lock().unwrap());
    wake();
}

fn run(state: Shared, rx: Receiver<Cmd>, wake: impl Fn()) {
    loop {
        let h = match Headset::open() {
            Ok(h) => h,
            Err(_) => {
                update(&state, &wake, |s| *s = DevState::default());
                match rx.recv_timeout(RECONNECT_EVERY) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    _ => continue,
                }
            }
        };
        update(&state, &wake, |s| s.dongle = true);
        match session(&h, &state, &rx, &wake) {
            Ok(()) => return, // UI went away
            Err(_) => {
                // Dongle unplugged or I/O error: start over.
                drop(h);
                update(&state, &wake, |s| *s = DevState::default());
                std::thread::sleep(RECONNECT_EVERY);
            }
        }
    }
}

/// Query everything the UI shows. A timeout means the headset is not linked.
fn query_all(h: &Headset, state: &Shared, wake: &impl Fn()) -> std::io::Result<()> {
    let product = match h.product_name() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            update(state, wake, |s| s.linked = Some(false));
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let serial = h.serial_number().ok();
    let firmware = h.firmware().ok();
    let battery = h.battery().ok();
    let sidetone = h.sidetone().ok();
    // The official app treats -1 as "no noise reduction in this headset".
    let noise_reduction = h.noise_reduction().ok().filter(|&l| l <= 2);
    update(state, wake, |s| {
        s.linked = Some(true);
        s.product = Some(product);
        s.serial = serial;
        s.firmware = firmware;
        s.battery = battery;
        s.sidetone = sidetone;
        s.noise_reduction = noise_reduction;
    });
    Ok(())
}

fn session(h: &Headset, state: &Shared, rx: &Receiver<Cmd>, wake: &impl Fn()) -> std::io::Result<()> {
    query_all(h, state, wake)?;
    let mut next_battery = Instant::now() + BATTERY_EVERY;
    // After a link-up, give the headset a moment before querying it.
    let mut requery_at: Option<Instant> = None;

    loop {
        // Commands: keep only the latest sidetone value so slider drags don't queue up.
        let mut sidetone = None;
        let mut noise_reduction = None;
        let mut refresh = false;
        loop {
            match rx.try_recv() {
                Ok(Cmd::SetSidetone(v)) => sidetone = Some(v),
                Ok(Cmd::SetNoiseReduction(v)) => noise_reduction = Some(v),
                Ok(Cmd::Refresh) => refresh = true,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some(v) = sidetone
            && let Ok(0) = h.set_sidetone(v)
        {
            update(state, wake, |s| s.sidetone = Some(v));
        }
        if let Some(v) = noise_reduction
            && let Ok(0) = h.set_noise_reduction(v)
        {
            // Read back: the headset keeps this setting.
            let got = h.noise_reduction().ok();
            update(state, wake, |s| s.noise_reduction = got);
        }
        if refresh {
            query_all(h, state, wake)?;
        }
        if requery_at.is_some_and(|t| Instant::now() >= t) {
            requery_at = None;
            query_all(h, state, wake)?;
        }
        let linked = state.lock().unwrap().linked == Some(true);
        if linked && Instant::now() >= next_battery {
            next_battery = Instant::now() + BATTERY_EVERY;
            match h.battery() {
                Ok(b) => update(state, wake, |s| s.battery = Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
        }

        let Some(frame) = h.poll_frame(150)? else { continue };
        match frame.decode() {
            Message::HeadsetLink { connected } => {
                update(state, wake, |s| {
                    s.linked = Some(connected);
                    if !connected {
                        s.battery = None;
                        s.anc = None;
                    }
                });
                if connected {
                    requery_at = Some(Instant::now() + Duration::from_millis(1500));
                }
            }
            // Responses to anyone's queries (including the official service) are fair game.
            Message::Battery { percent } => update(state, wake, |s| s.battery = Some(percent)),
            Message::SidetoneChanged { raw } => update(state, wake, |s| s.sidetone = Some(raw)),
            Message::Anc { on } => update(state, wake, |s| s.anc = Some(on)),
            Message::ProductName(p) => update(state, wake, |s| s.product = Some(p)),
            Message::FirmwareVersion(f) => update(state, wake, |s| s.firmware = Some(f)),
            Message::SerialNumber(n) => update(state, wake, |s| s.serial = Some(n)),
            _ => {}
        }
    }
}
