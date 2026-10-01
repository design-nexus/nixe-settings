//! Finding Logitech HID++ interfaces in `/sys/class/hidraw` and talking to them
//! through `/dev/hidraw*`.

use super::link::Io;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub const LOGITECH: u16 = 0x046D;
pub const BUS_USB: u16 = 0x0003;
pub const BUS_BLUETOOTH: u16 = 0x0005;

/// One hidraw node that speaks HID++.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    pub path: PathBuf,
    pub name: String,
    pub bus: u16,
    pub product: u16,
    pub short: bool,
    pub long: bool,
}

fn parse_hid_id(value: &str) -> Option<(u16, u16, u16)> {
    let mut parts = value.split(':');
    let bus = u16::from_str_radix(parts.next()?, 16).ok()?;
    let vendor = u32::from_str_radix(parts.next()?, 16).ok()? as u16;
    let product = u32::from_str_radix(parts.next()?, 16).ok()? as u16;
    Some((bus, vendor, product))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Which HID++ report sizes a report descriptor declares, if it's a HID++
/// interface at all (vendor usage page 0xFF00, or 0xFF43 over Bluetooth).
pub fn hidpp_reports(descriptor: &[u8]) -> Option<(bool, bool)> {
    let vendor_page = contains(descriptor, &[0x06, 0x00, 0xFF]) || contains(descriptor, &[0x06, 0x43, 0xFF]);
    let short = contains(descriptor, &[0x85, 0x10]);
    let long = contains(descriptor, &[0x85, 0x11]);
    (vendor_page && (short || long)).then_some((short, long))
}

/// Parse one `/sys/class/hidraw/hidrawN` entry.
pub fn node_from_sysfs(dir: &Path) -> Option<NodeInfo> {
    let uevent = std::fs::read_to_string(dir.join("device/uevent")).ok()?;
    let field = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k).map(str::to_string));
    let (bus, vendor, product) = parse_hid_id(&field("HID_ID=")?)?;
    if vendor != LOGITECH {
        return None;
    }
    // Devices paired to a receiver also get their own node from hid-logitech-dj
    // (phys "…/input2:1"); we reach them through the receiver instead.
    let phys = field("HID_PHYS=").unwrap_or_default();
    if bus == BUS_USB && phys.rsplit('/').next().is_some_and(|last| last.contains(':')) {
        return None;
    }
    let descriptor = std::fs::read(dir.join("device/report_descriptor")).ok()?;
    let (short, long) = hidpp_reports(&descriptor)?;
    let name = dir.file_name()?.to_string_lossy().to_string();
    Some(NodeInfo {
        path: PathBuf::from("/dev").join(name),
        name: field("HID_NAME=").unwrap_or_else(|| "Logitech device".into()),
        bus,
        product,
        short,
        long,
    })
}

/// Every Logitech HID++ interface on the system.
pub fn scan() -> Vec<NodeInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/hidraw") else { return vec![] };
    let mut nodes: Vec<NodeInfo> = entries.flatten().filter_map(|e| node_from_sysfs(&e.path())).collect();
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    nodes
}

pub struct Hidraw {
    file: File,
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self { file })
    }
}

impl Io for Hidraw {
    fn write(&mut self, report: &[u8]) -> io::Result<()> {
        self.file.write_all(report)
    }

    fn read(&mut self, timeout_ms: i32) -> io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the duration of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if n == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "device was unplugged"));
        }
        let mut buf = [0u8; 64];
        let len = self.file.read(&mut buf)?;
        Ok(Some(buf[..len].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hid_id() {
        assert_eq!(parse_hid_id("0003:0000046D:0000C52B"), Some((3, 0x046D, 0xC52B)));
        assert_eq!(parse_hid_id("bogus"), None);
    }

    #[test]
    fn detects_hidpp_descriptor() {
        // Excerpt of a Unifying receiver's HID++ interface.
        let d = [0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x10, 0x95, 0x06, 0x85, 0x11, 0x95, 0x13];
        assert_eq!(hidpp_reports(&d), Some((true, true)));
        assert_eq!(hidpp_reports(&[0x05, 0x01, 0x09, 0x02, 0x85, 0x02]), None);
    }
}
