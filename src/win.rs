//! Small Win32 helpers shared by the library and both executables.

use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

/// A predefined registry root.
#[derive(Clone, Copy, Debug)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    pub fn handle(self) -> HKEY {
        match self {
            Self::CurrentUser => HKEY_CURRENT_USER,
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

/// A NUL-terminated UTF-16 copy of `s`, for `W` APIs.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A `REG_SZ` value, or `None` if the key or value is missing (or longer than
/// 1023 characters).
pub fn reg_string(hive: Hive, key: &str, value: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(value));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the name pointers are NUL-terminated and outlive the call; `buf`
    // is writable for `size` bytes, which the call updates to the bytes written.
    let rc = unsafe {
        RegGetValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    // `size` counts the terminating NUL.
    (rc == 0).then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
}
