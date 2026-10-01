//! Logitech receivers (Unifying, Bolt, Lightspeed, Nano) through their HID++ 1.0
//! registers: paired devices, connection notifications, pairing and unpairing.

use super::device::{Kind, be16};
use super::link::{DIRECT, GET_LONG_REGISTER, Link, Result, SET_LONG_REGISTER, SET_REGISTER};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    Unifying,
    Bolt,
    Lightspeed,
    Nano,
}

impl ReceiverKind {
    pub fn from_product(pid: u16) -> Option<Self> {
        Some(match pid {
            0xC52B | 0xC532 => ReceiverKind::Unifying,
            0xC548 => ReceiverKind::Bolt,
            0xC539 | 0xC53A | 0xC53D | 0xC53F | 0xC541 | 0xC545 | 0xC547 | 0xC54D => ReceiverKind::Lightspeed,
            0xC51A | 0xC51B | 0xC521 | 0xC525 | 0xC526 | 0xC52E | 0xC52F | 0xC531 | 0xC534 | 0xC537 | 0xC542 => {
                ReceiverKind::Nano
            }
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            ReceiverKind::Unifying => "Unifying receiver",
            ReceiverKind::Bolt => "Bolt receiver",
            ReceiverKind::Lightspeed => "Lightspeed receiver",
            ReceiverKind::Nano => "Nano receiver",
        }
    }

    pub fn slots(self) -> u8 {
        match self {
            ReceiverKind::Unifying | ReceiverKind::Bolt => 6,
            ReceiverKind::Lightspeed => 2,
            ReceiverKind::Nano => 1,
        }
    }

    /// Whether this app can pair new devices to it.
    pub fn can_pair(self) -> bool {
        matches!(self, ReceiverKind::Unifying | ReceiverKind::Nano | ReceiverKind::Lightspeed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub slot: u8,
    pub wpid: u16,
    pub kind: Kind,
    pub name: String,
    pub serial: String,
}

const RECEIVER_INFO: u8 = 0xB5;
const NOTIFICATIONS: u8 = 0x00;
const CONNECTION_STATE: u8 = 0x02;
const PAIRING: u8 = 0xB2;
const BOLT_PAIRING: u8 = 0xC1;

/// Ask the receiver to report connections, battery and so on.
pub fn enable_notifications(link: &mut Link) -> Result<()> {
    link.register(DIRECT, SET_REGISTER, NOTIFICATIONS, &[0x00, 0x09, 0x00]).map(|_| ())
}

/// Ask for a connection notification (0x41) for every device that's connected now.
pub fn request_connections(link: &mut Link) -> Result<()> {
    link.register(DIRECT, SET_REGISTER, CONNECTION_STATE, &[0x02, 0x00, 0x00]).map(|_| ())
}

pub fn serial(link: &mut Link, kind: ReceiverKind) -> String {
    let addr = if kind == ReceiverKind::Bolt { 0xFB } else { 0x03 };
    link.register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[addr])
        .ok()
        .filter(|r| r.len() >= 5)
        .map(|r| r[1..5].iter().map(|b| format!("{b:02X}")).collect())
        .unwrap_or_default()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_matches(char::from(0)).trim().to_string()
}

/// What the receiver remembers about the device in `slot`, if one is paired there.
pub fn paired(link: &mut Link, kind: ReceiverKind, slot: u8) -> Option<Paired> {
    if kind == ReceiverKind::Bolt {
        let r = link.register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[0x50 + slot]).ok()?;
        if r.len() < 8 {
            return None;
        }
        let wpid = u16::from_le_bytes([r[2], r[3]]);
        if wpid == 0 {
            return None;
        }
        let serial = r[4..8].iter().map(|b| format!("{b:02X}")).collect();
        let name = link
            .register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[0x60 + slot, 0x01])
            .ok()
            .filter(|n| n.len() >= 3)
            .map(|n| {
                let len = (n[2] as usize).min(n.len() - 3);
                text(&n[3..3 + len])
            })
            .unwrap_or_default();
        return Some(Paired { slot, wpid, kind: Kind::from_receiver_code(r[1]), name, serial });
    }
    let n = slot - 1;
    let r = link.register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[0x20 + n]).ok()?;
    if r.len() < 8 {
        return None;
    }
    let wpid = be16(&r, 3);
    if wpid == 0 {
        return None;
    }
    let serial = link
        .register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[0x30 + n])
        .ok()
        .filter(|s| s.len() >= 5)
        .map(|s| s[1..5].iter().map(|b| format!("{b:02X}")).collect())
        .unwrap_or_default();
    let name = link
        .register(DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, &[0x40 + n])
        .ok()
        .filter(|s| s.len() >= 2)
        .map(|s| {
            let len = (s[1] as usize).min(s.len() - 2);
            text(&s[2..2 + len])
        })
        .unwrap_or_default();
    Some(Paired { slot, wpid, kind: Kind::from_receiver_code(r[7]), name, serial })
}

pub fn unpair(link: &mut Link, kind: ReceiverKind, slot: u8) -> Result<()> {
    if kind == ReceiverKind::Bolt {
        link.register(DIRECT, SET_LONG_REGISTER, BOLT_PAIRING, &[0x03, slot]).map(|_| ())
    } else {
        link.register(DIRECT, SET_REGISTER, PAIRING, &[0x03, slot, 0x00]).map(|_| ())
    }
}

/// Let a new device pair for `seconds` (Unifying, Nano, Lightspeed).
pub fn open_pairing(link: &mut Link, seconds: u8) -> Result<()> {
    link.register(DIRECT, SET_REGISTER, PAIRING, &[0x01, 0x00, seconds]).map(|_| ())
}

pub fn close_pairing(link: &mut Link) -> Result<()> {
    link.register(DIRECT, SET_REGISTER, PAIRING, &[0x02, 0x00, 0x00]).map(|_| ())
}

/// Something a receiver announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A device connected or went away (asleep, switched off, out of range).
    Connection { slot: u8, online: bool, wpid: u16, kind: Kind },
    /// A device was unpaired.
    Unpaired { slot: u8 },
    /// The pairing lock opened or closed; `error` is set when pairing failed.
    Pairing { open: bool, error: Option<u8> },
}

pub fn parse_notice(r: &[u8]) -> Option<Notice> {
    if r.len() < 7 {
        return None;
    }
    match r[2] {
        0x41 => Some(Notice::Connection {
            slot: r[1],
            online: r[3] & 0x40 == 0,
            wpid: u16::from_le_bytes([r[4], r[5]]),
            kind: Kind::from_receiver_code(r[3]),
        }),
        0x40 => Some(Notice::Unpaired { slot: r[1] }),
        0x4A => Some(Notice::Pairing { open: r[3] & 0x01 != 0, error: (r[4] != 0).then_some(r[4]) }),
        _ => None,
    }
}

pub fn pairing_error(code: u8) -> &'static str {
    match code {
        0x01 => "No device turned up in time.",
        0x02 => "That device isn't supported by this receiver.",
        0x03 => "The receiver is full. Unpair a device first.",
        0x06 => "The connection was interrupted.",
        _ => "Pairing failed.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_notices() {
        let online = parse_notice(&[0x10, 0x02, 0x41, 0x02, 0x34, 0xB0, 0x00]).unwrap();
        assert_eq!(online, Notice::Connection { slot: 2, online: true, wpid: 0xB034, kind: Kind::Mouse });
        let offline = parse_notice(&[0x10, 0x01, 0x41, 0x41, 0x5B, 0xB3, 0x00]).unwrap();
        assert!(matches!(offline, Notice::Connection { online: false, kind: Kind::Keyboard, .. }));
        assert_eq!(parse_notice(&[0x10, 0xFF, 0x4A, 0x00, 0x01, 0, 0]), Some(Notice::Pairing { open: false, error: Some(1) }));
    }

    #[test]
    fn receiver_kinds() {
        assert_eq!(ReceiverKind::from_product(0xC52B), Some(ReceiverKind::Unifying));
        assert_eq!(ReceiverKind::from_product(0xC548), Some(ReceiverKind::Bolt));
        assert_eq!(ReceiverKind::from_product(0xB034), None);
    }
}
