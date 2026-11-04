//! EPOS / Sennheiser GSP 370 (dongle `1395:009A`): battery and sidetone.
//!
//! Not the Capricorn HID protocol of the H3PRO. The GSP 370 dongle's HID
//! interface only carries media keys; its settings go over the dongle's USB
//! serial interface (CDC ACM, interface 4, a COM port on Windows) at 921600
//! baud, in packets framed like Bluetooth HCI vendor commands:
//!
//! ```text
//! command: 01 <op> FC <param len> <params...>
//! reply:   04 0E <len> 01 <op> FC <status> <values...>
//! ```
//!
//! Commands: `9C` battery percent; `A2 level` sets sidetone (0 = off, else
//! 6..=16), preceded by `A0` (turning it on) or `A1` (off). Status `0C` means
//! the headset is off. These facts come from captures of the official software
//! published by GSP 370 owners (HeadsetControl issue #619); not tested with a
//! GSP 370 here. Unrelated events from the dongle are skipped.

use std::io;
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, PURGE_RXCLEAR, PURGE_TXCLEAR, PurgeComm, SetCommState,
    SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, ReadFile, WriteFile};

use crate::win::{Hive, reg_string, reg_subkeys, wide};

pub const PID: u16 = 0x009A;
const USB_KEY: &str = r"SYSTEM\CurrentControlSet\Enum\USB\VID_1395&PID_009A&MI_04";
const BAUD: u32 = 921_600;

const OP_BATTERY: u8 = 0x9C;
const OP_SIDETONE_ON: u8 = 0xA0;
const OP_SIDETONE_OFF: u8 = 0xA1;
const OP_SIDETONE_SET: u8 = 0xA2;
const STATUS_OK: u8 = 0x00;
const STATUS_HEADSET_OFF: u8 = 0x0C;
/// Device sidetone levels above off.
pub const SIDETONE_MIN: u8 = 0x06;
pub const SIDETONE_MAX: u8 = 0x10;

/// A command packet.
pub fn command(op: u8, params: &[u8]) -> Vec<u8> {
    let mut p = vec![0x01, op, 0xFC, params.len() as u8];
    p.extend_from_slice(params);
    p
}

/// The reply to `op` in `received`: status byte then values. `None` until a
/// complete reply has arrived.
pub fn parse_reply(received: &[u8], op: u8) -> Option<Vec<u8>> {
    const HEADER: usize = 6;
    (0..received.len().saturating_sub(HEADER)).find_map(|i| {
        let r = &received[i..];
        let is_reply = r[0] == 0x04 && r[1] == 0x0E && r[3] == 0x01 && r[4] == op && r[5] == 0xFC;
        let end = 3 + r[2] as usize;
        (is_reply && r[2] >= 4 && end <= r.len()).then(|| r[HEADER..end].to_vec())
    })
}

/// Map 0.0..=1.0 to the device's sidetone level (0 = off, else 6..=16).
pub fn sidetone_level(fraction: f32) -> u8 {
    if fraction <= 0.0 {
        return 0;
    }
    let span = f32::from(SIDETONE_MAX - SIDETONE_MIN);
    SIDETONE_MIN + (fraction.min(1.0) * span).round() as u8
}

/// COM port names of connected GSP 370 dongles (from the USB device's
/// "Device Parameters\PortName"; readable without special rights).
pub fn ports() -> Vec<String> {
    reg_subkeys(Hive::LocalMachine, USB_KEY)
        .into_iter()
        .filter_map(|inst| reg_string(Hive::LocalMachine, &format!(r"{USB_KEY}\{inst}\Device Parameters"), "PortName"))
        .filter(|p| p.to_ascii_uppercase().starts_with("COM"))
        .collect()
}

/// An open GSP 370 dongle serial port.
pub struct Gsp370 {
    h: HANDLE,
}

// The handle is only used by the owning thread at a time.
unsafe impl Send for Gsp370 {}

impl Gsp370 {
    /// Open the first port that opens (ports of unplugged dongles stay in the
    /// registry).
    pub fn open() -> io::Result<Self> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no GSP 370 dongle found");
        for port in ports() {
            match Self::open_port(&port) {
                Ok(p) => return Ok(p),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn open_port(port: &str) -> io::Result<Self> {
        let path = wide(&format!(r"\\.\{port}"));
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let port = Self { h };
        port.configure()?;
        Ok(port)
    }

    fn configure(&self) -> io::Result<()> {
        // SAFETY: `self.h` is an open COM handle; the structs are zeroed and sized.
        unsafe {
            let mut dcb: DCB = std::mem::zeroed();
            dcb.DCBlength = size_of::<DCB>() as u32;
            if GetCommState(self.h, &mut dcb) == 0 {
                return Err(io::Error::last_os_error());
            }
            dcb.BaudRate = BAUD;
            dcb.ByteSize = 8;
            dcb.Parity = NOPARITY;
            dcb.StopBits = ONESTOPBIT;
            dcb._bitfield = 1; // fBinary only: no flow control
            if SetCommState(self.h, &dcb) == 0 {
                return Err(io::Error::last_os_error());
            }
            // Return from ReadFile after 50 ms with whatever arrived.
            let t = COMMTIMEOUTS {
                ReadIntervalTimeout: 20,
                ReadTotalTimeoutMultiplier: 0,
                ReadTotalTimeoutConstant: 50,
                WriteTotalTimeoutMultiplier: 0,
                WriteTotalTimeoutConstant: 500,
            };
            if SetCommTimeouts(self.h, &t) == 0 {
                return Err(io::Error::last_os_error());
            }
            PurgeComm(self.h, PURGE_RXCLEAR | PURGE_TXCLEAR);
        }
        Ok(())
    }

    /// Send `op` and wait up to a second for its reply; returns the values.
    pub fn request(&self, op: u8, params: &[u8]) -> io::Result<Vec<u8>> {
        let pkt = command(op, params);
        let mut written = 0u32;
        // SAFETY: `pkt` is readable for its length; `self.h` is open.
        if unsafe { WriteFile(self.h, pkt.as_ptr(), pkt.len() as u32, &mut written, std::ptr::null_mut()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut received = Vec::new();
        while Instant::now() < deadline {
            let mut buf = [0u8; 256];
            let mut n = 0u32;
            // SAFETY: `buf` is writable for its length; `self.h` is open.
            if unsafe { ReadFile(self.h, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            received.extend_from_slice(&buf[..n as usize]);
            if let Some(reply) = parse_reply(&received, op) {
                return check(reply, op);
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, format!("no reply to GSP 370 command {op:#04x}")))
    }

    pub fn battery(&self) -> io::Result<u8> {
        match self.request(OP_BATTERY, &[])?.as_slice() {
            [p] if *p <= 100 => Ok(*p),
            other => Err(io::Error::other(format!("unexpected battery reply {other:02x?}"))),
        }
    }

    /// Set sidetone to a device level (0 = off, else 6..=16).
    pub fn set_sidetone(&self, level: u8) -> io::Result<()> {
        let level = if level == 0 { 0 } else { level.clamp(SIDETONE_MIN, SIDETONE_MAX) };
        self.request(if level == 0 { OP_SIDETONE_OFF } else { OP_SIDETONE_ON }, &[])?;
        self.request(OP_SIDETONE_SET, &[level])?;
        Ok(())
    }
}

fn check(reply: Vec<u8>, op: u8) -> io::Result<Vec<u8>> {
    match reply.split_first() {
        Some((&STATUS_OK, values)) => Ok(values.to_vec()),
        // Reported like the H3PRO's "not delivered", so callers treat it the same.
        Some((&STATUS_HEADSET_OFF, _)) => Err(io::Error::new(io::ErrorKind::TimedOut, "the headset is off")),
        Some((s, _)) => Err(io::Error::other(format!("GSP 370 command {op:#04x} failed with status {s:#04x}"))),
        None => Err(io::Error::other("empty GSP 370 reply")),
    }
}

impl Drop for Gsp370 {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open_port` and is closed once.
        unsafe { CloseHandle(self.h) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_framed_like_hci_vendor_commands() {
        assert_eq!(command(OP_BATTERY, &[]), [0x01, 0x9C, 0xFC, 0x00]);
        assert_eq!(command(OP_SIDETONE_SET, &[0x08]), [0x01, 0xA2, 0xFC, 0x01, 0x08]);
    }

    #[test]
    fn replies_are_found_among_other_events() {
        // An unrelated event, then the battery reply: status 0, 87 %.
        let rx = [0x04, 0xE9, 0x00, 0x04, 0x0E, 0x05, 0x01, 0x9C, 0xFC, 0x00, 87];
        assert_eq!(parse_reply(&rx, OP_BATTERY), Some(vec![0x00, 87]));
        assert_eq!(parse_reply(&rx[..9], OP_BATTERY), None, "incomplete");
        assert_eq!(parse_reply(&rx, OP_SIDETONE_SET), None, "other opcode");
        assert_eq!(check(vec![0x00, 87], OP_BATTERY).unwrap(), vec![87]);
        assert_eq!(check(vec![0x0C], OP_BATTERY).unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn sidetone_levels_skip_the_gap_above_off() {
        assert_eq!(sidetone_level(0.0), 0);
        assert_eq!(sidetone_level(0.01), SIDETONE_MIN);
        assert_eq!(sidetone_level(1.0), SIDETONE_MAX);
        assert_eq!(sidetone_level(2.0), SIDETONE_MAX);
    }
}
