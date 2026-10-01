//! Just enough of a HID report-descriptor reader to describe an interface:
//! its first usage page, whether it numbers its reports, and report sizes.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub usage_page: u16,
    pub report_ids: bool,
    /// Largest input / output report, in bytes (without a report id).
    pub input_len: usize,
    pub output_len: usize,
}

impl Summary {
    /// Vendor-defined pages (0xFF00-0xFFFF) carry a device's own protocol.
    pub fn is_vendor(&self) -> bool {
        self.usage_page >= 0xFF00
    }

    pub fn page_name(&self) -> String {
        match self.usage_page {
            0x01 => "Generic desktop".into(),
            0x07 => "Keyboard".into(),
            0x0C => "Consumer (media keys)".into(),
            0x0D => "Digitizer".into(),
            p if p >= 0xFF00 => format!("Vendor-defined (0x{p:04X})"),
            p => format!("0x{p:04X}"),
        }
    }
}

pub fn parse(d: &[u8]) -> Summary {
    let mut s = Summary::default();
    let (mut size_bits, mut count) = (0usize, 0usize);
    let mut lens: std::collections::HashMap<(u8, u8), usize> = Default::default(); // (main tag, report id) → bits
    let mut report_id = 0u8;
    let mut i = 0;
    while i < d.len() {
        let prefix = d[i];
        if prefix == 0xFE {
            // Long item: size in the next byte.
            let n = d.get(i + 1).copied().unwrap_or(0) as usize;
            i += 3 + n;
            continue;
        }
        let n = match prefix & 0x03 {
            3 => 4,
            k => k as usize,
        };
        let data = d.get(i + 1..i + 1 + n).unwrap_or(&[]);
        let value = data.iter().rev().fold(0u32, |v, b| (v << 8) | *b as u32);
        match prefix & 0xFC {
            0x04 if s.usage_page == 0 => s.usage_page = value as u16, // first Usage Page
            0x74 => size_bits = value as usize,
            0x94 => count = value as usize,
            0x84 => {
                report_id = value as u8;
                s.report_ids = true;
            }
            tag @ (0x80 | 0x90) => *lens.entry((tag, report_id)).or_default() += size_bits * count,
            _ => {}
        }
        i += 1 + n;
    }
    let max = |tag: u8| lens.iter().filter(|((t, _), _)| *t == tag).map(|(_, bits)| bits.div_ceil(8)).max().unwrap_or(0);
    s.input_len = max(0x80);
    s.output_len = max(0x90);
    s
}

pub fn hex(d: &[u8]) -> String {
    d.chunks(16).map(|c| c.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vendor interface with 64-byte reports, like Corsair's Bragi one.
    pub const VENDOR: &[u8] = &[
        0x06, 0x42, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x09, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40, 0x81, 0x02,
        0x09, 0x02, 0x95, 0x40, 0x91, 0x02, 0xC0,
    ];

    #[test]
    fn reads_vendor_interface() {
        let s = parse(VENDOR);
        assert_eq!((s.usage_page, s.input_len, s.output_len, s.report_ids), (0xFF42, 64, 64, false));
        assert!(s.is_vendor());
        assert_eq!(s.page_name(), "Vendor-defined (0xFF42)");
    }

    #[test]
    fn reads_boot_keyboard() {
        // Generic desktop / keyboard: 8 modifier bits + 1 reserved byte + 6 keys in, 5 LED bits + 3 padding out.
        let d = [
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x75, 0x08, 0x95, 0x01, 0x81,
            0x01, 0x95, 0x06, 0x81, 0x00, 0x05, 0x08, 0x75, 0x01, 0x95, 0x05, 0x91, 0x02, 0x75, 0x03, 0x95, 0x01, 0x91, 0x01,
            0xC0,
        ];
        let s = parse(&d);
        assert_eq!((s.usage_page, s.input_len, s.output_len), (0x01, 8, 1));
        assert!(!s.is_vendor());
    }

    #[test]
    fn report_ids_count_separately() {
        let d = [0x06, 0x00, 0xFF, 0x85, 0x01, 0x75, 0x08, 0x95, 0x10, 0x81, 0x02, 0x85, 0x02, 0x95, 0x20, 0x81, 0x02];
        let s = parse(&d);
        assert!(s.report_ids);
        assert_eq!(s.input_len, 32);
    }

    #[test]
    fn survives_truncation() {
        let _ = parse(&[0x06, 0x42]);
        let _ = parse(&[0xFE]);
    }
}
