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

/// A `REG_BINARY` value, or `None` if the key or value is missing.
pub fn reg_binary(hive: Hive, key: &str, value: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Registry::RRF_RT_REG_BINARY;
    let (key, value) = (wide(key), wide(value));
    let mut buf = [0u8; 256];
    let mut size = buf.len() as u32;
    // SAFETY: the name pointers are NUL-terminated and outlive the call; `buf`
    // is writable for `size` bytes, which the call updates to the bytes written.
    let rc = unsafe {
        RegGetValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (rc == 0).then(|| buf[..size as usize].to_vec())
}

/// Write a `REG_BINARY` value, creating the key if needed.
pub fn set_reg_binary(hive: Hive, key: &str, value: &str, data: &[u8]) -> std::io::Result<()> {
    use windows_sys::Win32::System::Registry::{REG_BINARY, RegSetKeyValueW};
    let (key, value) = (wide(key), wide(value));
    // SAFETY: the name pointers are NUL-terminated and outlive the call; `data`
    // is readable for the length passed.
    let rc = unsafe {
        RegSetKeyValueW(
            hive.handle(),
            key.as_ptr(),
            value.as_ptr(),
            REG_BINARY,
            data.as_ptr().cast(),
            data.len() as u32,
        )
    };
    if rc == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(rc as i32)) }
}
