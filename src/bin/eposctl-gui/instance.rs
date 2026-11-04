//! Single instance: a second launch signals the running one, which shows its
//! window, and exits.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use eframe::egui;
use eposctl::win::wide;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};

use crate::tray::Flags;

/// Named event shared by all instances.
const SHOW_EVENT: &str = r"Local\eposctl-gui-show";

/// The "show yourself" event of the first instance.
pub struct ShowEvent(Option<isize>);

/// `None` if another instance is running (it has been told to show itself).
pub fn claim() -> Option<ShowEvent> {
    // Debug builds taking a screenshot run next to a normal instance.
    #[cfg(debug_assertions)]
    if std::env::var_os("EPOSCTL_SHOT").is_some() {
        return Some(ShowEvent(None));
    }
    let name = wide(SHOW_EVENT);
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let h = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if h.is_null() {
        return Some(ShowEvent(None)); // can't coordinate; just run
    }
    // SAFETY: plain Win32 calls on the handle just created.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { SetEvent(h) };
        return None;
    }
    Some(ShowEvent(Some(h as isize)))
}

impl ShowEvent {
    /// Raise the show flag and wake the UI whenever another launch signals us.
    pub fn watch(self, flags: Arc<Flags>, ctx: egui::Context) {
        let Some(h) = self.0 else { return };
        std::thread::spawn(move || {
            // SAFETY: the event handle stays open for the life of the process.
            while unsafe { WaitForSingleObject(h as HANDLE, INFINITE) } == 0 {
                flags.show.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        });
    }
}
