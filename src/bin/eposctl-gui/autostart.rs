//! Start with Windows: a `Run` entry under the current user.

use std::io;

use eposctl::win::{Hive, reg_string, wide};
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "eposctl";

/// What the entry runs: this executable, started hidden in the tray.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --minimized", exe.display()))
}

/// The entry exists and points at this executable.
pub fn is_enabled() -> bool {
    let Some(current) = reg_string(Hive::CurrentUser, RUN_KEY, VALUE) else { return false };
    command().is_some_and(|c| c.eq_ignore_ascii_case(&current))
}

pub fn set(enabled: bool) -> io::Result<()> {
    let (key, val) = (wide(RUN_KEY), wide(VALUE));
    let rc = if enabled {
        let cmd = wide(&command().ok_or_else(|| io::Error::other("cannot locate executable"))?);
        // SAFETY: all pointers are NUL-terminated UTF-16 buffers alive for the call;
        // the data length is the buffer's size in bytes.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                val.as_ptr(),
                REG_SZ,
                cmd.as_ptr().cast(),
                (cmd.len() * 2) as u32,
            )
        }
    } else {
        // SAFETY: as above.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr()) } {
            ERROR_FILE_NOT_FOUND => 0, // already off
            rc => rc,
        }
    };
    if rc == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(rc as i32)) }
}
