//! Corsair USB devices, from sysfs: each device with its interfaces, their
//! hidraw nodes and report descriptors. No device I/O here.
//!
//! `CORSAIR_MOCK=galleon-100-sd` (add `,denied` for a device without access)
//! stands in a simulated Galleon 100 SD, for tests and screenshots.

use crate::descriptor;
use crate::models::CORSAIR;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub number: u8,
    pub class: u8,
    /// 1 boot keyboard, 2 boot mouse, 0 none.
    pub protocol: u8,
    /// `hidraw3`, if the kernel made a node for it.
    pub hidraw: Option<String>,
    pub descriptor: Vec<u8>,
}

impl Interface {
    pub fn summary(&self) -> descriptor::Summary {
        descriptor::parse(&self.descriptor)
    }

    pub fn node(&self) -> Option<PathBuf> {
        self.hidraw.as_ref().map(|h| Path::new("/dev").join(h))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub product: u16,
    pub name: String,
    pub serial: String,
    /// `bcdDevice`: the USB release number the firmware reports.
    pub release: u16,
    pub speed: String,
    /// sysfs name, e.g. `1-7`.
    pub port: String,
    pub interfaces: Vec<Interface>,
    pub simulated: bool,
}

impl Device {
    pub fn has_keyboard(&self) -> bool {
        self.interfaces.iter().any(|i| i.class == 3 && i.protocol == 1)
    }

    pub fn interface(&self, n: u8) -> Option<&Interface> {
        self.interfaces.iter().find(|i| i.number == n)
    }

    /// `1.04` from bcdDevice 0x0104.
    pub fn release_text(&self) -> String {
        format!("{:x}.{:02x}", self.release >> 8, self.release & 0xFF)
    }

    /// A stable page id: model plus serial (or USB port).
    pub fn page_id(&self) -> String {
        let tail = if self.serial.is_empty() { self.port.clone() } else { self.serial.to_lowercase() };
        let tail: String = tail.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        format!("{:04x}-{tail}", self.product)
    }
}

pub fn mock_spec() -> Option<String> {
    std::env::var("CORSAIR_MOCK").ok().filter(|s| !s.trim().is_empty())
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn hex16(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim(), 16).ok()
}

fn interface_from_sysfs(dir: &Path) -> Option<Interface> {
    let number = u8::from_str_radix(&read(dir, "bInterfaceNumber"), 16).ok()?;
    let class = u8::from_str_radix(&read(dir, "bInterfaceClass"), 16).unwrap_or(0);
    let protocol = u8::from_str_radix(&read(dir, "bInterfaceProtocol"), 16).unwrap_or(0);
    // …/1-7:1.1/0003:1B1C:2B0C.0004/{report_descriptor,hidraw/hidraw5}
    let hid = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("0003:")) && p.join("report_descriptor").exists()
    });
    let (hidraw, descriptor) = match hid {
        Some(h) => {
            let node = std::fs::read_dir(h.join("hidraw"))
                .ok()
                .and_then(|mut rd| rd.next())
                .and_then(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string());
            (node, std::fs::read(h.join("report_descriptor")).unwrap_or_default())
        }
        None => (None, Vec::new()),
    };
    Some(Interface { number, class, protocol, hidraw, descriptor })
}

pub fn device_from_sysfs(dir: &Path) -> Option<Device> {
    if hex16(&read(dir, "idVendor"))? != CORSAIR {
        return None;
    }
    let port = dir.file_name()?.to_string_lossy().to_string();
    let mut interfaces: Vec<Interface> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{port}:"))))
        .filter_map(|p| interface_from_sysfs(&p))
        .collect();
    interfaces.sort_by_key(|i| i.number);
    let product_name = read(dir, "product");
    Some(Device {
        product: hex16(&read(dir, "idProduct"))?,
        name: if product_name.is_empty() { "Corsair device".into() } else { product_name },
        serial: read(dir, "serial"),
        release: hex16(&read(dir, "bcdDevice")).unwrap_or(0),
        speed: read(dir, "speed"),
        port,
        interfaces,
        simulated: false,
    })
}

/// Every Corsair USB device plugged in now.
pub fn scan() -> Vec<Device> {
    if let Some(spec) = mock_spec() {
        return mock(&spec);
    }
    let Ok(rd) = std::fs::read_dir("/sys/bus/usb/devices") else { return vec![] };
    let mut out: Vec<Device> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| !n.to_string_lossy().contains(':')))
        .filter_map(|p| device_from_sysfs(&p))
        .collect();
    out.sort_by(|a, b| a.port.cmp(&b.port));
    out
}

/// Whether we may open the device's hidraw nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Access {
    Allowed,
    Denied,
    /// No hidraw nodes to try.
    Unknown,
}

pub fn access(dev: &Device) -> Access {
    if dev.simulated {
        return if mock_spec().is_some_and(|s| s.contains("denied")) { Access::Denied } else { Access::Allowed };
    }
    let mut result = Access::Unknown;
    for node in dev.interfaces.iter().filter_map(Interface::node) {
        match std::fs::OpenOptions::new().read(true).write(true).open(&node) {
            Ok(_) => return Access::Allowed,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => result = Access::Denied,
            Err(_) => {}
        }
    }
    result
}

// ----- Simulated hardware -----

const BOOT_KEYBOARD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x75, 0x08, 0x95, 0x01, 0x81, 0x01,
    0x95, 0x06, 0x81, 0x00, 0x05, 0x08, 0x75, 0x01, 0x95, 0x05, 0x91, 0x02, 0x75, 0x03, 0x95, 0x01, 0x91, 0x01, 0xC0,
];
const VENDOR_64: &[u8] = &[
    0x06, 0x42, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x09, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40, 0x81, 0x02,
    0x09, 0x02, 0x95, 0x40, 0x91, 0x02, 0xC0,
];
const CONSUMER: &[u8] = &[0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00, 0xC0];

fn mock(spec: &str) -> Vec<Device> {
    if !spec.split(',').any(|s| s.trim() == "galleon-100-sd") {
        return vec![];
    }
    let iface = |number, protocol, node: &str, d: &[u8]| Interface {
        number,
        class: 3,
        protocol,
        hidraw: Some(node.into()),
        descriptor: d.to_vec(),
    };
    vec![
        Device {
            product: 0x2B0C,
            name: "CORSAIR GALLEON 100 SD Mechanical Gaming Keyboard".into(),
            serial: "SIM0000GALLEON".into(),
            release: 0x0104,
            speed: "480".into(),
            port: "1-7.1".into(),
            interfaces: vec![
                iface(0, 1, "hidraw90", BOOT_KEYBOARD),
                iface(1, 0, "hidraw91", VENDOR_64),
                iface(2, 0, "hidraw92", CONSUMER),
            ],
            simulated: true,
        },
        Device {
            product: 0x2B18,
            name: "GALLEON 100 SD Stream Deck".into(),
            serial: "SIM0000DECK".into(),
            release: 0x0100,
            speed: "480".into(),
            port: "1-7.2".into(),
            interfaces: vec![iface(0, 0, "hidraw93", VENDOR_64)],
            simulated: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, name: &str, v: &str) {
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join(name), v).unwrap();
    }

    #[test]
    fn reads_a_device_from_sysfs() {
        let root = std::env::temp_dir().join(format!("corsair-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dev = root.join("1-7");
        for (k, v) in [("idVendor", "1b1c\n"), ("idProduct", "2b0c\n"), ("product", "CORSAIR GALLEON 100 SD\n"), ("serial", "ABC123\n"), ("bcdDevice", "0104\n"), ("speed", "480\n")] {
            write(&dev, k, v);
        }
        for (n, proto, node, d) in [("0", "01", "hidraw4", BOOT_KEYBOARD), ("1", "00", "hidraw5", VENDOR_64)] {
            let i = dev.join(format!("1-7:1.{n}"));
            write(&i, "bInterfaceNumber", &format!("0{n}\n"));
            write(&i, "bInterfaceClass", "03\n");
            write(&i, "bInterfaceProtocol", &format!("{proto}\n"));
            let hid = i.join(format!("0003:1B1C:2B0C.000{n}"));
            std::fs::create_dir_all(hid.join("hidraw").join(node)).unwrap();
            std::fs::write(hid.join("report_descriptor"), d).unwrap();
        }
        let d = device_from_sysfs(&dev).unwrap();
        assert_eq!((d.product, d.serial.as_str(), d.release_text()), (0x2B0C, "ABC123", "1.04".to_string()));
        assert_eq!(d.interfaces.len(), 2);
        assert!(d.has_keyboard());
        assert_eq!(d.interface(1).unwrap().hidraw.as_deref(), Some("hidraw5"));
        assert_eq!(d.interface(1).unwrap().summary().output_len, 64);
        assert_eq!(d.page_id(), "2b0c-abc123");
        // Other vendors are ignored.
        write(&root.join("1-8"), "idVendor", "046d\n");
        assert!(device_from_sysfs(&root.join("1-8")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn simulated_galleon() {
        let d = mock("galleon-100-sd");
        assert_eq!(d.len(), 2);
        assert!(d[0].has_keyboard() && !d[1].has_keyboard());
        assert!(d[0].interface(1).unwrap().summary().is_vendor());
        assert!(mock("other").is_empty());
    }
}
