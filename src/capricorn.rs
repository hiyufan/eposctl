//! Framing for the EPOS "Capricorn" HID protocol used by the H3PRO dongle.
//!
//! Frames travel in HID report ID 1 on the dongle's vendor collection
//! (usage page 0xFFFF, interface 3, 106-byte payload). All fields are
//! little-endian u16:
//!
//! ```text
//! byte 0     report id (0x01)
//! u16 @1     magic 0xFEEB
//! u16 @3     0x0001 (protocol version?)
//! u16 @5     kind: 0x0004 response, 0x010F notification, 0x0110 link event, ...
//! u16 @7     node: 0 = dongle, 4 = headset (observed only after the headset connects)
//! u16 @9     body length in bytes
//! u16 @11    CRC-8 of the body (poly 0x07, init 0xFF, no reflection, no final xor)
//! @13        body: u16 group, u16 command, then command-specific u16 values
//! ```
//!
//! See `docs/protocol.md` for the evidence.

pub const REPORT_ID: u8 = 0x01;
pub const REPORT_LEN: usize = 107; // including the report id byte
pub const MAGIC: u16 = 0xFEEB;
const HEADER_LEN: usize = 13;

pub mod kind {
    pub const RESPONSE: u16 = 0x0004;
    pub const NOTIFICATION: u16 = 0x010F;
    pub const LINK: u16 = 0x0110;
}

/// CRC-8, polynomial 0x07, init 0xFF, MSB-first, no output xor.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0xFF;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u16,
    pub kind: u16,
    pub node: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    BadReportId(u8),
    BadMagic(u16),
    BadLength(usize),
    BadCrc { expected: u8, got: u16 },
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

impl Frame {
    /// Parse a raw input report (starting with the report id byte).
    pub fn parse(report: &[u8]) -> Result<Self, FrameError> {
        if report.len() < HEADER_LEN {
            return Err(FrameError::TooShort);
        }
        if report[0] != REPORT_ID {
            return Err(FrameError::BadReportId(report[0]));
        }
        let magic = u16_at(report, 1);
        if magic != MAGIC {
            return Err(FrameError::BadMagic(magic));
        }
        let len = u16_at(report, 9) as usize;
        if HEADER_LEN + len > REPORT_LEN || HEADER_LEN + len > report.len() {
            return Err(FrameError::BadLength(len));
        }
        let body = report[HEADER_LEN..HEADER_LEN + len].to_vec();
        let got = u16_at(report, 11);
        let expected = crc8(&body);
        if got != expected as u16 {
            return Err(FrameError::BadCrc { expected, got });
        }
        Ok(Frame { version: u16_at(report, 3), kind: u16_at(report, 5), node: u16_at(report, 7), body })
    }

    /// Serialise into a full 107-byte report.
    pub fn encode(&self) -> [u8; REPORT_LEN] {
        assert!(HEADER_LEN + self.body.len() <= REPORT_LEN, "body too long");
        let mut r = [0u8; REPORT_LEN];
        r[0] = REPORT_ID;
        r[1..3].copy_from_slice(&MAGIC.to_le_bytes());
        r[3..5].copy_from_slice(&self.version.to_le_bytes());
        r[5..7].copy_from_slice(&self.kind.to_le_bytes());
        r[7..9].copy_from_slice(&self.node.to_le_bytes());
        r[9..11].copy_from_slice(&(self.body.len() as u16).to_le_bytes());
        r[11..13].copy_from_slice(&(crc8(&self.body) as u16).to_le_bytes());
        r[HEADER_LEN..HEADER_LEN + self.body.len()].copy_from_slice(&self.body);
        r
    }

    /// Body interpreted as little-endian u16 words.
    pub fn words(&self) -> Vec<u16> {
        self.body.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    pub fn group(&self) -> Option<u16> {
        self.words().first().copied()
    }

    pub fn command(&self) -> Option<u16> {
        self.words().get(1).copied()
    }

    /// Decode a UTF-16 string carried after group/command/count words.
    fn utf16_tail(&self, skip_words: usize) -> String {
        let w: Vec<u16> = self.words().into_iter().skip(skip_words).take_while(|&c| c != 0).collect();
        String::from_utf16_lossy(&w)
    }

    pub fn decode(&self) -> Message {
        let w = self.words();
        let at = |i: usize| w.get(i).copied();
        match (self.kind, at(0), at(1)) {
            (kind::LINK, Some(0x01), Some(0xF0)) => Message::HeadsetLink { connected: at(3) == Some(1) },
            (kind::RESPONSE, Some(0x12), Some(0x03)) => match at(2) {
                Some(p) => Message::Battery { percent: p as u8 },
                None => Message::Unknown,
            },
            (kind::RESPONSE, Some(0x01), Some(0x03)) if w.len() >= 5 => {
                Message::FirmwareVersion(format!("{}.{}.{}", w[2], w[3], w[4]))
            }
            (kind::RESPONSE, Some(0x01), Some(0x0B)) => Message::SerialNumber(self.utf16_tail(2)),
            (kind::RESPONSE, Some(0x09), Some(0x01)) => Message::ProductName(self.utf16_tail(2)),
            (kind::RESPONSE, Some(0x0A), Some(0x0B)) => Message::SidetoneAck { status: at(2).unwrap_or(0xFFFF) },
            (kind::NOTIFICATION, Some(0x0A), Some(0xF0)) if at(2) == Some(2) => {
                Message::SidetoneChanged { raw: at(3).unwrap_or(0) }
            }
            (kind::NOTIFICATION, Some(0x17), Some(0x07)) => Message::Anc { on: at(2) == Some(1) },
            _ => Message::Unknown,
        }
    }
}

/// Messages whose meaning has been established from captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    HeadsetLink { connected: bool },
    Battery { percent: u8 },
    FirmwareVersion(String),
    SerialNumber(String),
    ProductName(String),
    /// Acknowledgement of a sidetone write; 0 = success (inferred).
    SidetoneAck { status: u16 },
    /// Raw sidetone value: official slider at minimum gave 0xFF, at maximum 0x00.
    SidetoneChanged { raw: u16 },
    Anc { on: bool },
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames captured from an H3PRO dongle (1395:0303) while the official
    /// service was running (the serial-number frame uses a placeholder serial
    /// with its CRC recomputed). Trailing bytes past the body are stale buffer
    /// content in the real reports and are irrelevant here.
    fn hex(s: &str) -> Vec<u8> {
        let mut v: Vec<u8> = s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
        v.resize(REPORT_LEN, 0);
        v
    }

    #[test]
    fn crc_matches_captures() {
        assert_eq!(crc8(&[0x12, 0, 0x03, 0, 0x64, 0]), 0xF8);
        assert_eq!(crc8(&[0x0A, 0, 0x0B, 0, 0, 0]), 0x38);
        assert_eq!(crc8(&[0x17, 0, 0x07, 0, 0x01, 0]), 0x99);
    }

    #[test]
    fn battery() {
        let f = Frame::parse(&hex("01 eb fe 01 00 04 00 04 00 06 00 f8 00 12 00 03 00 64 00")).unwrap();
        assert_eq!(f.node, 4);
        assert_eq!(f.decode(), Message::Battery { percent: 100 });
    }

    #[test]
    fn firmware_and_serial() {
        let f = Frame::parse(&hex("01 eb fe 01 00 04 00 04 00 0a 00 22 00 01 00 03 00 01 00 03 00 05 00")).unwrap();
        assert_eq!(f.decode(), Message::FirmwareVersion("1.3.5".into()));
        let f = Frame::parse(&hex(
            "01 eb fe 01 00 04 00 04 00 24 00 94 00 01 00 0b 00 41 00 30 00 30 00 30 00 30 00 30 00 30 00 30 00 31 00 32 00 33 00 34 00 35 00 36 00 37 00 38 00",
        ))
        .unwrap();
        assert_eq!(f.decode(), Message::SerialNumber("A000000012345678".into()));
    }

    #[test]
    fn anc_and_sidetone_events() {
        let f = Frame::parse(&hex("01 eb fe 01 00 0f 01 04 00 06 00 99 00 17 00 07 00 01 00")).unwrap();
        assert_eq!(f.decode(), Message::Anc { on: true });
        let f = Frame::parse(&hex("01 eb fe 01 00 0f 01 04 00 08 00 57 00 0a 00 f0 00 02 00 8b 00")).unwrap();
        assert_eq!(f.decode(), Message::SidetoneChanged { raw: 0x8B });
    }

    #[test]
    fn link_event() {
        let f = Frame::parse(&hex("01 eb fe 01 00 10 01 00 00 08 00 f4 00 01 00 f0 00 01 00 01 00")).unwrap();
        assert_eq!(f.decode(), Message::HeadsetLink { connected: true });
    }

    #[test]
    fn rejects_bad_crc() {
        let r = hex("01 eb fe 01 00 04 00 04 00 06 00 f7 00 12 00 03 00 64 00");
        assert_eq!(Frame::parse(&r), Err(FrameError::BadCrc { expected: 0xF8, got: 0xF7 }));
    }

    #[test]
    fn encode_roundtrip() {
        let f = Frame { version: 1, kind: kind::RESPONSE, node: 4, body: vec![0x12, 0, 0x03, 0, 0x64, 0] };
        let r = f.encode();
        assert_eq!(&r[..19], &hex("01 eb fe 01 00 04 00 04 00 06 00 f8 00 12 00 03 00 64 00")[..19]);
        assert_eq!(Frame::parse(&r).unwrap(), f);
    }
}
