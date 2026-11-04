//! The official EPOS Gaming Suite next to eposctl (model, no UI): its state,
//! refreshed now and then, and switching it off or on. Switching runs this
//! executable again with administrator rights (`--suite on|off`, handled in
//! `main` before any window opens), on a thread so the UI keeps drawing while
//! Windows asks the user.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use eposctl::suite::{self, Status};

/// The argument that makes this executable switch the suite and exit.
pub const ARG: &str = "--suite";

/// How a switch ended.
pub enum Outcome {
    Done { on: bool },
    Declined,
    Failed(String),
}

#[derive(Default)]
pub struct Official {
    status: Status,
    checked: Option<Instant>,
    pending: Option<Receiver<Outcome>>,
}

impl Official {
    const REFRESH: Duration = Duration::from_secs(3);

    /// The state, read again when the last reading is a few seconds old.
    pub fn status(&mut self) -> Status {
        if self.checked.is_none_or(|t| t.elapsed() >= Self::REFRESH) {
            self.status = suite::status();
            self.checked = Some(Instant::now());
        }
        self.status
    }

    /// A switch is waiting for the user or still running.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Switch everything off (`false`) or back on (`true`).
    pub fn switch(&mut self, on: bool, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = match std::env::current_exe() {
                Ok(exe) => run(&exe, on),
                Err(e) => Outcome::Failed(e.to_string()),
            };
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
        self.pending = Some(rx);
    }

    /// The result of a finished switch, once.
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        let rx = self.pending.as_ref()?;
        match rx.try_recv() {
            Ok(o) => {
                self.pending = None;
                self.checked = None; // read the new state right away
                Some(o)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Outcome::Failed("the switch did not finish".into()))
            }
        }
    }
}

fn run(exe: &std::path::Path, on: bool) -> Outcome {
    let args = format!("{ARG} {}", if on { "on" } else { "off" });
    match suite::run_elevated(exe, &args) {
        Ok(0) => {
            if !on {
                suite::quit_app();
            }
            Outcome::Done { on }
        }
        Ok(code) => Outcome::Failed(format!("exit code {code}")),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Outcome::Declined,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// When started as `<exe> --suite on|off` (with administrator rights), do the
/// switch and return the exit code; `None` for a normal start.
pub fn helper_mode() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next()? != ARG {
        return None;
    }
    let on = args.next().as_deref() == Some("on");
    Some(if suite::set_enabled(on).is_ok() { 0 } else { 1 })
}
