//! Watches which program is in the foreground, for per-app presets.
//!
//! A background thread owns a `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` hook and its
//! message loop, so nothing polls. Our own windows are skipped, so opening eposctl to
//! edit a rule does not count as switching programs.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG,
    TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

/// A program has to stay in front this long before its preset is applied, so
/// Alt+Tab passing over other windows does not switch presets back and forth.
pub const SETTLE: Duration = Duration::from_millis(600);
const RECENT: usize = 12;

#[derive(Default)]
pub struct State {
    /// Lower-case executable name of the foreground program, and since when.
    pub current: Option<(String, Instant)>,
    /// Recently focused programs, most recent first.
    pub recent: Vec<String>,
}

pub type Shared = Arc<Mutex<State>>;

struct Hook {
    state: Shared,
    ctx: egui::Context,
}

static HOOK: OnceLock<Hook> = OnceLock::new();

/// Start watching. Safe to call once; later calls return the same state.
pub fn spawn(ctx: egui::Context) -> Shared {
    let state = Shared::default();
    if HOOK.set(Hook { state: state.clone(), ctx }).is_err() {
        return HOOK.get().unwrap().state.clone();
    }
    std::thread::spawn(|| unsafe {
        focused(GetForegroundWindow());
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if hook.is_null() {
            return;
        }
        // Out-of-context hooks are delivered through this thread's message queue.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    state
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if id_object == 0 {
        // OBJID_WINDOW
        focused(hwnd);
    }
}

fn focused(hwnd: HWND) {
    let Some(hook) = HOOK.get() else { return };
    let Some(exe) = exe_of(hwnd) else { return };
    {
        let mut st = hook.state.lock().unwrap();
        if st.current.as_ref().is_some_and(|(c, _)| *c == exe) {
            return;
        }
        st.recent.retain(|r| *r != exe);
        st.recent.insert(0, exe.clone());
        st.recent.truncate(RECENT);
        st.current = Some((exe, Instant::now()));
    }
    // Wake the UI now (to note the change) and once the program has settled.
    hook.ctx.request_repaint();
    hook.ctx.request_repaint_after(SETTLE + Duration::from_millis(50));
}

/// Lower-case file name of the process owning `hwnd`, or `None` for our own
/// process and for processes we may not query.
fn exe_of(hwnd: HWND) -> Option<String> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return None;
    }
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(h) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let name = path.rsplit(['\\', '/']).next()?.to_lowercase();
    (!name.is_empty()).then_some(name)
}
