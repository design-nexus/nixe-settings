//! HID++ framing over one hidraw node: request/reply matching, error decoding,
//! and a queue for the notifications that arrive in between.
//!
//! Reports: short `0x10` (7 bytes), long `0x11` (20 bytes), very long `0x12`
//! (64 bytes). Byte 1 is the device index (`0xFF` for the receiver itself or a
//! directly connected device), byte 2 the feature index (HID++ 2.0) or sub-id
//! (HID++ 1.0), byte 3 the function and software id (2.0) or register address.

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

pub const SHORT: u8 = 0x10;
pub const LONG: u8 = 0x11;
pub const VERY_LONG: u8 = 0x12;

/// Device index for the receiver itself, or a device plugged in directly.
pub const DIRECT: u8 = 0xFF;

/// HID++ 1.0 register sub-ids.
pub const SET_REGISTER: u8 = 0x80;
pub const SET_LONG_REGISTER: u8 = 0x82;
pub const GET_LONG_REGISTER: u8 = 0x83;
const ERROR_10: u8 = 0x8F;
const ERROR_20: u8 = 0xFF;

pub fn report_len(id: u8) -> Option<usize> {
    match id {
        SHORT => Some(7),
        LONG => Some(20),
        VERY_LONG => Some(64),
        _ => None,
    }
}

/// Raw access to a HID++ interface.
pub trait Io: Send {
    fn write(&mut self, report: &[u8]) -> io::Result<()>;
    /// The next input report, or `None` if nothing arrives within `timeout_ms`.
    fn read(&mut self, timeout_ms: i32) -> io::Result<Option<Vec<u8>>>;
}

#[derive(Debug)]
pub enum Error {
    Timeout,
    Io(io::Error),
    /// HID++ 1.0 error code (from the receiver, or an old device).
    V10(u8),
    /// HID++ 2.0 error code.
    V20(u8),
    /// The device doesn't have the feature the request needs.
    NoFeature(u16),
    /// The reply was shorter than expected.
    Short,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Timeout => write!(f, "the device didn't answer"),
            Error::Io(e) => write!(f, "{e}"),
            Error::V10(code) => write!(f, "{}", v10_message(*code)),
            Error::V20(code) => write!(f, "{}", v20_message(*code)),
            Error::NoFeature(id) => write!(f, "the device doesn't support feature {id:04X}"),
            Error::Short => write!(f, "the device sent an incomplete reply"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// The device is asleep, switched off or out of range.
    pub fn is_offline(&self) -> bool {
        matches!(self, Error::Timeout | Error::V10(0x09) | Error::V10(0x03) | Error::V10(0x04))
    }
}

fn v10_message(code: u8) -> &'static str {
    match code {
        0x01 => "invalid request",
        0x02 => "invalid address",
        0x03 => "invalid value",
        0x04 => "connection failed",
        0x05 => "too many devices",
        0x06 => "already exists",
        0x07 => "busy",
        0x08 => "unknown device",
        0x09 => "the device isn't connected",
        0x0A => "request unavailable",
        0x0B => "invalid parameter",
        0x0C => "wrong PIN code",
        _ => "unknown receiver error",
    }
}

fn v20_message(code: u8) -> &'static str {
    match code {
        0x01 => "unknown error",
        0x02 => "invalid argument",
        0x03 => "out of range",
        0x04 => "hardware error",
        0x05 => "not allowed by the device",
        0x06 => "invalid feature index",
        0x07 => "invalid function",
        0x08 => "busy",
        0x09 => "unsupported",
        _ => "unknown device error",
    }
}

/// One open HID++ interface.
pub struct Link {
    io: Box<dyn Io>,
    short: bool,
    long: bool,
    swid: u8,
    /// Reports that arrived while waiting for a reply and weren't it.
    pub events: Vec<Vec<u8>>,
    pub timeout: Duration,
}

impl Link {
    /// `short`/`long`: which report sizes the interface accepts.
    pub fn new(io: Box<dyn Io>, short: bool, long: bool) -> Self {
        Self { io, short, long: long || !short, swid: 0x0F, events: Vec::new(), timeout: Duration::from_millis(1500) }
    }

    /// Software ids 0x2..=0xF, so our replies never look like notifications (0).
    fn next_swid(&mut self) -> u8 {
        self.swid = if self.swid >= 0x0F { 0x02 } else { self.swid + 1 };
        self.swid
    }

    fn frame(&self, device: u8, b2: u8, b3: u8, params: &[u8], force_long: bool) -> Vec<u8> {
        // Short when it fits (or the interface has nothing else, like some Nano receivers).
        let fits = params.len() <= 3 && !force_long;
        let (id, len) = if self.short && (fits || !self.long) { (SHORT, 7) } else { (LONG, 20) };
        let mut r = vec![0u8; len];
        r[0] = id;
        r[1] = device;
        r[2] = b2;
        r[3] = b3;
        for (i, p) in params.iter().take(len - 4).enumerate() {
            r[4 + i] = *p;
        }
        r
    }

    /// Call a HID++ 2.0 feature function. Returns the reply parameters.
    pub fn call(&mut self, device: u8, feature_index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let b3 = (function << 4) | self.next_swid();
        let req = self.frame(device, feature_index, b3, params, false);
        self.io.write(&req)?;
        self.wait(|r| {
            if r[1] != device {
                return None;
            }
            if r[2] == feature_index && r[3] == b3 {
                Some(Ok(r[4..].to_vec()))
            } else if r[2] == ERROR_20 && r[3] == feature_index && r[4] == b3 {
                Some(Err(Error::V20(r[5])))
            } else if r[2] == ERROR_10 && r[3] == feature_index && r[4] == b3 {
                Some(Err(Error::V10(r[5])))
            } else {
                None
            }
        })
    }

    /// Read or write a HID++ 1.0 register (receivers). Returns the reply parameters.
    pub fn register(&mut self, device: u8, sub_id: u8, address: u8, params: &[u8]) -> Result<Vec<u8>> {
        let req = self.frame(device, sub_id, address, params, sub_id == SET_LONG_REGISTER);
        self.io.write(&req)?;
        self.wait(|r| {
            if r[1] != device {
                return None;
            }
            if r[2] == sub_id && r[3] == address {
                Some(Ok(r[4..].to_vec()))
            } else if r[2] == ERROR_10 && r[3] == sub_id && r[4] == address {
                Some(Err(Error::V10(r[5])))
            } else {
                None
            }
        })
    }

    fn wait(&mut self, mut matches: impl FnMut(&[u8]) -> Option<Result<Vec<u8>>>) -> Result<Vec<u8>> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let Some(r) = self.io.read(left.as_millis().max(1) as i32)? else { continue };
            if r.len() < 7 || report_len(r[0]).is_none() {
                continue;
            }
            if let Some(result) = matches(&r) {
                return result;
            }
            self.events.push(r);
        }
    }

    /// Collect everything that arrives in the next `ms` milliseconds.
    pub fn listen(&mut self, ms: u64) -> io::Result<Vec<Vec<u8>>> {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if let Some(r) = self.io.read(left.as_millis().max(1) as i32)?
                && r.len() >= 7
                && report_len(r[0]).is_some()
            {
                self.events.push(r);
            }
        }
        Ok(std::mem::take(&mut self.events))
    }

    /// Collect notifications that are already waiting, without blocking.
    pub fn poll(&mut self) -> io::Result<Vec<Vec<u8>>> {
        while let Some(r) = self.io.read(0)? {
            if r.len() >= 7 && report_len(r[0]).is_some() {
                self.events.push(r);
            }
        }
        Ok(std::mem::take(&mut self.events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Replays canned replies and records what was written.
    #[derive(Clone, Default)]
    struct Script {
        written: Arc<Mutex<Vec<Vec<u8>>>>,
        replies: Arc<Mutex<VecDeque<Vec<u8>>>>,
    }

    impl Io for Script {
        fn write(&mut self, report: &[u8]) -> io::Result<()> {
            self.written.lock().unwrap().push(report.to_vec());
            Ok(())
        }
        fn read(&mut self, _: i32) -> io::Result<Option<Vec<u8>>> {
            Ok(self.replies.lock().unwrap().pop_front())
        }
    }

    fn long(bytes: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 20];
        r[..bytes.len()].copy_from_slice(bytes);
        r
    }

    #[test]
    fn short_requests_when_params_fit() {
        let s = Script::default();
        let swid = 0x02;
        s.replies.lock().unwrap().push_back(long(&[LONG, 0xFF, 0x00, 0x10 | swid, 4, 5, 0xAA]));
        let mut link = Link::new(Box::new(s.clone()), true, true);
        let reply = link.call(DIRECT, 0x00, 1, &[0, 0, 0xAA]).unwrap();
        assert_eq!(&reply[..3], &[4, 5, 0xAA]);
        let w = s.written.lock().unwrap();
        assert_eq!(w[0], vec![SHORT, 0xFF, 0x00, 0x12, 0, 0, 0xAA]);
    }

    #[test]
    fn long_only_interfaces_get_long_reports() {
        let s = Script::default();
        s.replies.lock().unwrap().push_back(long(&[LONG, 0x01, 0x03, 0x22, 1]));
        let mut link = Link::new(Box::new(s.clone()), false, true);
        link.call(0x01, 0x03, 2, &[]).unwrap();
        assert_eq!(s.written.lock().unwrap()[0].len(), 20);
    }

    #[test]
    fn decodes_errors_and_queues_notifications() {
        let s = Script::default();
        {
            let mut q = s.replies.lock().unwrap();
            // A battery notification (swid 0) arrives first.
            q.push_back(long(&[LONG, 0x01, 0x04, 0x00, 50]));
            q.push_back(long(&[LONG, 0x01, ERROR_20, 0x05, 0x12, 0x02]));
        }
        let mut link = Link::new(Box::new(s.clone()), true, true);
        match link.call(0x01, 0x05, 1, &[]) {
            Err(Error::V20(2)) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(link.events.len(), 1);
    }

    #[test]
    fn register_errors() {
        let s = Script::default();
        s.replies.lock().unwrap().push_back(vec![SHORT, 0xFF, ERROR_10, 0x81, 0x02, 0x02, 0]);
        let mut link = Link::new(Box::new(s.clone()), true, true);
        assert!(matches!(link.register(DIRECT, 0x81, 0x02, &[]), Err(Error::V10(2))));
    }
}
