//! Request/response access to a Capricorn headset through its dongle.
//!
//! Command table recovered from `CCapricornDatagram::getHIDBuffers` in
//! EPOSGamingSuiteService 1.12.2: each internal opcode maps to a fixed
//! `(group, command)` body; setters append one u16 value. Requests use the same
//! frame as responses (kind 4) and are addressed to node 4 (the headset).

use std::io;
use std::time::{Duration, Instant};

use crate::capricorn::{Frame, kind};
use crate::hid::{HidDevice, find_vendor_collections};

/// Node address of the headset behind the dongle (observed in every headset frame).
pub const HEADSET_NODE: u16 = 4;

/// `(group, command)` pairs from the service's opcode table.
pub mod cmd {
    pub const FIRMWARE_VERSION: (u16, u16) = (0x01, 0x03); // opcode 0
    pub const SERIAL_NUMBER: (u16, u16) = (0x01, 0x0B); // opcode 1
    pub const PRODUCT_NAME: (u16, u16) = (0x09, 0x01); // opcode 2
    pub const BATTERY: (u16, u16) = (0x12, 0x03); // opcode 14
    pub const SIDETONE_GET: (u16, u16) = (0x0A, 0x0C); // opcode 16 (GetSideToneGain)
    pub const SIDETONE_SET: (u16, u16) = (0x0A, 0x0B); // opcode 17, setter
}

pub struct Headset {
    dev: HidDevice,
    pub node: u16,
    /// Frames received while waiting for a response that belong to someone else
    /// (notifications, responses to the official service). Drained by `poll_frame`.
    pending: std::cell::RefCell<std::collections::VecDeque<Frame>>,
}

fn timeout_err(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, format!("no response to {what}; is the headset on and linked?"))
}

impl Headset {
    pub fn open() -> io::Result<Self> {
        let infos = find_vendor_collections();
        let info = infos.first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no EPOS vendor HID collection found")
        })?;
        Ok(Self { dev: HidDevice::open_rw(info)?, node: HEADSET_NODE, pending: Default::default() })
    }

    fn stash(&self, f: Frame) {
        let mut p = self.pending.borrow_mut();
        if p.len() >= 64 {
            p.pop_front();
        }
        p.push_back(f);
    }

    /// Next unsolicited frame (notification or foreign response), waiting up to `timeout_ms`.
    pub fn poll_frame(&self, timeout_ms: u32) -> io::Result<Option<Frame>> {
        if let Some(f) = self.pending.borrow_mut().pop_front() {
            return Ok(Some(f));
        }
        match self.dev.read(timeout_ms)? {
            Some(report) => Ok(Frame::parse(&report).ok()),
            None => Ok(None),
        }
    }

    /// Send `(group, command[, value])` and wait for the matching response frame.
    pub fn request(&self, (group, command): (u16, u16), value: Option<u16>, timeout: Duration) -> io::Result<Frame> {
        let mut body = Vec::with_capacity(6);
        body.extend_from_slice(&group.to_le_bytes());
        body.extend_from_slice(&command.to_le_bytes());
        if let Some(v) = value {
            body.extend_from_slice(&v.to_le_bytes());
        }
        let req = Frame { version: 1, kind: kind::RESPONSE, node: self.node, body };
        // Move anything already queued aside so we match only fresh responses.
        while let Some(report) = self.dev.read(0)? {
            if let Ok(f) = Frame::parse(&report) {
                self.stash(f);
            }
        }
        self.dev.write(&req.encode())?;

        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(report) = self.dev.read(left.as_millis().max(1) as u32)? else { break };
            let Ok(f) = Frame::parse(&report) else { continue };
            if f.kind == kind::RESPONSE && f.group() == Some(group) && f.command() == Some(command) {
                return Ok(f);
            }
            self.stash(f);
        }
        Err(timeout_err(&format!("{group:#04x}/{command:#04x}")))
    }

    fn get(&self, c: (u16, u16)) -> io::Result<Frame> {
        self.request(c, None, Duration::from_millis(1500))
    }

    fn value(&self, c: (u16, u16)) -> io::Result<u16> {
        self.get(c)?.words().get(2).copied().ok_or_else(|| io::Error::other("short response"))
    }

    pub fn battery(&self) -> io::Result<u8> {
        Ok(self.value(cmd::BATTERY)? as u8)
    }

    pub fn firmware(&self) -> io::Result<String> {
        let w = self.get(cmd::FIRMWARE_VERSION)?.words();
        if w.len() < 5 {
            return Err(io::Error::other("short firmware response"));
        }
        Ok(format!("{}.{}.{}", w[2], w[3], w[4]))
    }

    fn text(&self, c: (u16, u16)) -> io::Result<String> {
        let w = self.get(c)?.words();
        let chars: Vec<u16> = w.into_iter().skip(2).take_while(|&c| c != 0).collect();
        Ok(String::from_utf16_lossy(&chars))
    }

    pub fn serial_number(&self) -> io::Result<String> {
        self.text(cmd::SERIAL_NUMBER)
    }

    pub fn product_name(&self) -> io::Result<String> {
        self.text(cmd::PRODUCT_NAME)
    }

    pub fn sidetone(&self) -> io::Result<u16> {
        self.value(cmd::SIDETONE_GET)
    }

    /// Set sidetone. Returns the status word of the acknowledgement (0 = success, inferred).
    pub fn set_sidetone(&self, level: u16) -> io::Result<u16> {
        let f = self.request(cmd::SIDETONE_SET, Some(level), Duration::from_millis(1500))?;
        Ok(f.words().get(2).copied().unwrap_or(0xFFFF))
    }
}
