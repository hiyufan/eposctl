//! System-wide hotkeys (`RegisterHotKey`), handled on a background thread.
//!
//! The thread owns the registrations and its message queue. The UI thread hands it
//! the wanted combos with `set`, which posts a message so the thread re-registers.
//! A pressed hotkey sets a bit and wakes the UI, which acts on it in `logic`.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key, Modifiers};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NextPreset,
    PrevPreset,
    ToggleSurround,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::NextPreset, Action::PrevPreset, Action::ToggleSurround];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A key with Ctrl and/or Alt (and optionally Shift).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Combo {
    /// From a key press while recording. Needs Ctrl or Alt so a plain key
    /// never stops working in other programs.
    pub fn from_press(key: Key, m: Modifiers) -> Option<Self> {
        let c = Self { ctrl: m.ctrl, alt: m.alt, shift: m.shift, key };
        ((m.ctrl || m.alt) && vk(key).is_some()).then_some(c)
    }

    /// Parse the stored form, e.g. `Ctrl+Alt+PageDown`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = Key::from_name(parts.pop()?)?;
        let mut m = Modifiers::NONE;
        for p in parts {
            match p.to_ascii_lowercase().as_str() {
                "ctrl" => m.ctrl = true,
                "alt" => m.alt = true,
                "shift" => m.shift = true,
                _ => return None,
            }
        }
        Self::from_press(key, m)
    }

    /// Stored and displayed form.
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(self.key.name());
        s
    }

    fn modifiers(&self) -> u32 {
        let mut m = MOD_NOREPEAT;
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.alt {
            m |= MOD_ALT;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        m
    }
}

/// Windows virtual-key code for the keys a hotkey may use.
fn vk(key: Key) -> Option<u32> {
    let name = key.name();
    let b = name.as_bytes();
    let code = match name {
        _ if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) => b[0] as u32,
        _ if b.len() >= 2 && b[0] == b'F' && name[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) => {
            0x6F + name[1..].parse::<u32>().ok()?
        }
        "Space" => 0x20,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "End" => 0x23,
        "Home" => 0x24,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        _ => return None,
    };
    Some(code)
}

const WM_REREGISTER: u32 = WM_APP + 1;

struct Shared {
    wanted: Mutex<[Option<Combo>; 3]>,
    /// Combos that could not be registered (another program already uses them).
    failed: Mutex<[bool; 3]>,
    fired: AtomicU8,
    ctx: egui::Context,
}

pub struct Hotkeys {
    shared: Arc<Shared>,
    thread: u32,
    /// Last set passed to the thread, to skip redundant re-registration.
    current: [Option<Combo>; 3],
}

impl Hotkeys {
    pub fn spawn(ctx: egui::Context) -> Option<Self> {
        let shared = Arc::new(Shared {
            wanted: Mutex::new([None; 3]),
            failed: Mutex::new([false; 3]),
            fired: AtomicU8::new(0),
            ctx,
        });
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sh = shared.clone();
        std::thread::spawn(move || unsafe {
            let mut msg: MSG = std::mem::zeroed();
            // Create the message queue before telling anyone our thread id.
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            let _ = tx.send(GetCurrentThreadId());
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_REREGISTER => register(&sh),
                    WM_HOTKEY => {
                        let id = msg.wParam;
                        if (1..=3).contains(&id) {
                            sh.fired.fetch_or(1 << (id - 1), Ordering::Relaxed);
                            sh.ctx.request_repaint();
                        }
                    }
                    _ => {}
                }
            }
        });
        let thread = rx.recv().ok()?;
        Some(Self { shared, thread, current: [None; 3] })
    }

    /// Register exactly these combos (indexed like `Action::ALL`); `None` = no hotkey.
    pub fn set(&mut self, wanted: [Option<Combo>; 3]) {
        if wanted == self.current {
            return;
        }
        self.current = wanted;
        *self.shared.wanted.lock().unwrap() = wanted;
        unsafe { PostThreadMessageW(self.thread, WM_REREGISTER, 0, 0) };
    }

    pub fn take_fired(&self) -> Vec<Action> {
        let bits = self.shared.fired.swap(0, Ordering::Relaxed);
        Action::ALL.into_iter().filter(|a| bits & (1 << a.index()) != 0).collect()
    }

    /// The combo for `a` is wanted but another program holds it.
    pub fn failed(&self, a: Action) -> bool {
        self.shared.failed.lock().unwrap()[a.index()]
    }
}

/// Runs on the hotkey thread: registrations belong to the thread that made them.
fn register(sh: &Shared) {
    let wanted = *sh.wanted.lock().unwrap();
    let mut failed = [false; 3];
    for (i, combo) in wanted.iter().enumerate() {
        let id = i as i32 + 1;
        unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
        if let Some(c) = combo {
            let ok = vk(c.key).is_some_and(|k| unsafe { RegisterHotKey(std::ptr::null_mut(), id, c.modifiers(), k) } != 0);
            failed[i] = !ok;
        }
    }
    *sh.failed.lock().unwrap() = failed;
    sh.ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_round_trip() {
        for s in ["Ctrl+Alt+PageDown", "Alt+Shift+F5", "Ctrl+0", "Ctrl+Alt+Left", "Ctrl+S"] {
            assert_eq!(Combo::parse(s).map(|c| c.label()).as_deref(), Some(s));
        }
        assert_eq!(Combo::parse("Shift+A"), None, "needs Ctrl or Alt");
        assert_eq!(Combo::parse("Ctrl+Escape"), None, "unsupported key");
        assert_eq!(vk(Key::F1), Some(0x70));
        assert_eq!(vk(Key::F24), Some(0x87));
        assert_eq!(vk(Key::Q), Some(0x51));
    }
}
