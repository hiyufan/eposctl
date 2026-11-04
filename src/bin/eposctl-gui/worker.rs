//! Background thread that owns the HID connection to the dongle.
//!
//! It keeps `DevState` up to date from responses and notifications, polls the
//! battery, and executes commands from the UI. Every update wakes the UI.

use std::io;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
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

/// Reconnects whenever the dongle goes away; returns when the UI is gone.
fn run(state: Shared, rx: Receiver<Cmd>, wake: impl Fn()) {
    let ctx = Ctx { state, wake };
    loop {
        let Ok(h) = Headset::open() else {
            ctx.update(|s| *s = DevState::default());
            match rx.recv_timeout(RECONNECT_EVERY) {
                Err(RecvTimeoutError::Disconnected) => return,
                _ => continue,
            }
        };
        ctx.update(|s| s.dongle = true);
        if session(&h, &ctx, &rx).is_ok() {
            return; // UI went away
        }
        // Dongle unplugged or I/O error: start over.
        drop(h);
        ctx.update(|s| *s = DevState::default());
        std::thread::sleep(RECONNECT_EVERY);
    }
}

/// Shared state plus the UI wake-up, for publishing changes.
struct Ctx<W: Fn()> {
    state: Shared,
    wake: W,
}

impl<W: Fn()> Ctx<W> {
    fn update(&self, f: impl FnOnce(&mut DevState)) {
        update(&self.state, &self.wake, f);
    }

    fn linked(&self) -> bool {
        self.state.lock().unwrap().linked == Some(true)
    }
}

/// Commands since the last look, collapsed: only the latest value of each
/// setting, so slider drags don't queue up.
#[derive(Default)]
struct Pending {
    sidetone: Option<u16>,
    noise_reduction: Option<u16>,
    refresh: bool,
}

/// `None` once the UI has gone away.
fn drain(rx: &Receiver<Cmd>) -> Option<Pending> {
    let mut p = Pending::default();
    loop {
        match rx.try_recv() {
            Ok(Cmd::SetSidetone(v)) => p.sidetone = Some(v),
            Ok(Cmd::SetNoiseReduction(v)) => p.noise_reduction = Some(v),
            Ok(Cmd::Refresh) => p.refresh = true,
            Err(TryRecvError::Empty) => return Some(p),
            Err(TryRecvError::Disconnected) => return None,
        }
    }
}

/// A timeout means the headset did not answer: not an I/O failure.
fn timed_out<T>(r: &io::Result<T>) -> bool {
    matches!(r, Err(e) if e.kind() == io::ErrorKind::TimedOut)
}

/// Query everything the UI shows. A timeout means the headset is not linked.
fn query_all(h: &Headset, ctx: &Ctx<impl Fn()>) -> io::Result<()> {
    let product = h.product_name();
    if timed_out(&product) {
        ctx.update(|s| s.linked = Some(false));
        return Ok(());
    }
    let product = product?;
    let serial = h.serial_number().ok();
    let firmware = h.firmware().ok();
    let battery = h.battery().ok();
    let sidetone = h.sidetone().ok();
    // The official app treats -1 as "no noise reduction in this headset".
    let noise_reduction = h.noise_reduction().ok().filter(|&l| l <= 2);
    ctx.update(|s| {
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

/// Talk to one dongle until it fails (`Err`) or the UI goes away (`Ok`).
fn session(h: &Headset, ctx: &Ctx<impl Fn()>, rx: &Receiver<Cmd>) -> io::Result<()> {
    query_all(h, ctx)?;
    let mut next_battery = Instant::now() + BATTERY_EVERY;
    // After a link-up, give the headset a moment before querying it.
    let mut requery_at: Option<Instant> = None;
    loop {
        let Some(pending) = drain(rx) else { return Ok(()) };
        execute(h, ctx, &pending)?;
        if requery_at.is_some_and(|t| Instant::now() >= t) {
            requery_at = None;
            query_all(h, ctx)?;
        }
        if ctx.linked() && Instant::now() >= next_battery {
            next_battery = Instant::now() + BATTERY_EVERY;
            poll_battery(h, ctx)?;
        }
        let Some(frame) = h.poll_frame(150)? else { continue };
        if on_message(ctx, frame.decode()) {
            requery_at = Some(Instant::now() + Duration::from_millis(1500));
        }
    }
}

fn execute(h: &Headset, ctx: &Ctx<impl Fn()>, p: &Pending) -> io::Result<()> {
    if let Some(v) = p.sidetone
        && let Ok(0) = h.set_sidetone(v)
    {
        ctx.update(|s| s.sidetone = Some(v));
    }
    if let Some(v) = p.noise_reduction
        && let Ok(0) = h.set_noise_reduction(v)
    {
        // Read back: the headset keeps this setting.
        let got = h.noise_reduction().ok();
        ctx.update(|s| s.noise_reduction = got);
    }
    if p.refresh {
        query_all(h, ctx)?;
    }
    Ok(())
}

fn poll_battery(h: &Headset, ctx: &Ctx<impl Fn()>) -> io::Result<()> {
    let b = h.battery();
    if timed_out(&b) {
        return Ok(());
    }
    let b = b?;
    ctx.update(|s| s.battery = Some(b));
    Ok(())
}

/// Apply a notification or response. Responses to anyone's queries (including
/// the official service) are fair game. True when the headset just linked up.
fn on_message(ctx: &Ctx<impl Fn()>, msg: Message) -> bool {
    match msg {
        Message::HeadsetLink { connected } => {
            ctx.update(|s| {
                s.linked = Some(connected);
                if !connected {
                    s.battery = None;
                    s.anc = None;
                }
            });
            return connected;
        }
        Message::Battery { percent } => ctx.update(|s| s.battery = Some(percent)),
        Message::SidetoneChanged { raw } => ctx.update(|s| s.sidetone = Some(raw)),
        Message::Anc { on } => ctx.update(|s| s.anc = Some(on)),
        Message::ProductName(p) => ctx.update(|s| s.product = Some(p)),
        Message::FirmwareVersion(f) => ctx.update(|s| s.firmware = Some(f)),
        Message::SerialNumber(n) => ctx.update(|s| s.serial = Some(n)),
        _ => {}
    }
    false
}
