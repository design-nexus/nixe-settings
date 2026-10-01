//! Corsair's Bragi ("V2") protocol, read side only.
//!
//! There is deliberately no way to build anything but a GET here: writes with
//! the wrong layout can freeze the keyboard until it's unplugged, so nothing
//! that changes the device ships before its framing is confirmed on real hardware.
//!
//! Request (after the hidraw report-number byte): `08 02 <property> 00…`.
//! Expected reply: `00 02 <status> <value, little-endian>…`.

const HEADER: u8 = 0x08;
const GET: u8 = 0x02;

/// Properties worth reading, with what they're believed to hold.
pub const PROPERTIES: &[(u8, &str)] = &[
    (0x01, "Polling rate"),
    (0x02, "Brightness"),
    (0x03, "Mode (hardware or software)"),
    (0x11, "Vendor id"),
    (0x12, "Product id"),
    (0x13, "Firmware version"),
    (0x14, "Bootloader version"),
    (0x41, "Layout"),
];

/// A GET request for one property, as written to hidraw: a leading 0 (no report
/// number), then `out_len` bytes. Only properties from [`PROPERTIES`] are allowed.
pub fn get_request(property: u8, out_len: usize) -> Option<Vec<u8>> {
    if !PROPERTIES.iter().any(|(id, _)| *id == property) || !(4..=1024).contains(&out_len) {
        return None;
    }
    let mut b = vec![0u8; out_len + 1];
    b[1] = HEADER;
    b[2] = GET;
    b[3] = property;
    Some(b)
}

/// Whether a report looks like the answer to a GET.
pub fn is_get_reply(r: &[u8]) -> bool {
    r.len() >= 3 && r[1] == GET
}

/// The value of a successful GET reply (status 0), read little-endian from 4 bytes.
pub fn reply_value(r: &[u8]) -> Option<u32> {
    if !is_get_reply(r) || r[2] != 0 || r.len() < 7 {
        return None;
    }
    Some(u32::from_le_bytes([r[3], r[4], r[5], r[6]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_builds_gets() {
        for (p, _) in PROPERTIES {
            let r = get_request(*p, 64).unwrap();
            assert_eq!(r.len(), 65);
            assert_eq!(&r[..4], &[0x00, 0x08, 0x02, *p]);
            assert!(r[4..].iter().all(|b| *b == 0), "nothing after the property");
        }
        assert!(get_request(0x99, 64).is_none(), "unknown properties are refused");
        assert!(get_request(0x02, 2).is_none() && get_request(0x02, 4096).is_none());
    }

    #[test]
    fn reads_replies() {
        let ok = [0x00, 0x02, 0x00, 0xE8, 0x03, 0x00, 0x00, 0xAA];
        assert_eq!(reply_value(&ok), Some(1000));
        let failed = [0x00, 0x02, 0x05, 0, 0, 0, 0];
        assert!(is_get_reply(&failed) && reply_value(&failed).is_none());
        assert!(!is_get_reply(&[0x00, 0x01, 0x00]));
    }
}
