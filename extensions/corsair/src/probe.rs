//! `settings-corsair probe`: a read-only report about every Corsair device, to
//! send back so native lighting can be built from the real protocol.
//! It reads USB descriptors and asks a few Bragi GET questions; it never sends
//! anything that changes the device.

use crate::usb::{self, Access, Device};
use crate::{bragi, descriptor, hidraw, models};
use std::fmt::Write as _;
use std::process::Command;

fn lsusb() -> String {
    Command::new("lsusb")
        .args(["-v", "-d", "1b1c:"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_else(|e| format!("(lsusb unavailable: {e})"))
}

/// Ask each read-only property on the Bragi interface; record requests and raw replies.
fn query(dev: &Device, out: &mut String) {
    let Some(model) = models::by_keyboard(dev.product) else { return };
    let Some(iface) = dev.interface(model.bragi_interface) else {
        let _ = writeln!(out, "  Bragi: interface {} not found", model.bragi_interface);
        return;
    };
    let s = iface.summary();
    if !s.is_vendor() || !(32..=1024).contains(&s.output_len) {
        let _ = writeln!(out, "  Bragi: interface {} isn't a vendor interface with a usable report size; not queried", iface.number);
        return;
    }
    let Some(node) = iface.node() else { return };
    let mut h = match hidraw::Hidraw::open(&node) {
        Ok(h) => h,
        Err(e) => {
            let _ = writeln!(out, "  Bragi: couldn't open {}: {e}", node.display());
            return;
        }
    };
    let _ = writeln!(out, "  Bragi GET replies on interface {} ({}, {}-byte reports):", iface.number, node.display(), s.output_len);
    for (prop, name) in bragi::PROPERTIES {
        let Some(req) = bragi::get_request(*prop, s.output_len) else { continue };
        let _ = writeln!(out, "    0x{prop:02x} {name}");
        let _ = writeln!(out, "      sent: {}", descriptor::hex(&req[..8.min(req.len())]));
        if let Err(e) = h.write(&req) {
            let _ = writeln!(out, "      write failed: {e}");
            continue;
        }
        // Collect what comes back for a moment; unrelated input reports may arrive too.
        let mut got = false;
        for _ in 0..4 {
            match h.read(400) {
                Ok(Some(r)) => {
                    let decoded = bragi::reply_value(&r).map(|v| format!("  (value {v} / 0x{v:x})")).unwrap_or_default();
                    let _ = writeln!(out, "      recv: {}{decoded}", descriptor::hex(&r[..16.min(r.len())]));
                    got = true;
                    if bragi::is_get_reply(&r) {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = writeln!(out, "      read failed: {e}");
                    break;
                }
            }
        }
        if !got {
            let _ = writeln!(out, "      no reply");
        }
    }
}

pub fn report(ask: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "settings-corsair {} probe", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "kernel: {}", std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim());
    let devices = usb::scan();
    if devices.is_empty() {
        let _ = writeln!(out, "\nNo Corsair USB devices found.");
    }
    for d in &devices {
        let _ = writeln!(out, "\n== {:04x}:{:04x} {} ==", models::CORSAIR, d.product, d.name);
        let _ = writeln!(out, "  port {}, USB release {}, speed {} Mbit/s{}", d.port, d.release_text(), d.speed, if d.simulated { " (simulated)" } else { "" });
        let _ = writeln!(out, "  known model: {}", models::by_keyboard(d.product).map(|m| m.name).unwrap_or(if models::is_stream_deck(d.product) { "built-in Stream Deck" } else { "no" }));
        let _ = writeln!(out, "  access: {:?}", usb::access(d));
        for i in &d.interfaces {
            let s = i.summary();
            let _ = writeln!(
                out,
                "  interface {}: class {:02x} protocol {:02x}, {}, {}, input {} / output {} bytes{}",
                i.number,
                i.class,
                i.protocol,
                i.hidraw.as_deref().unwrap_or("no hidraw"),
                s.page_name(),
                s.input_len,
                s.output_len,
                if s.report_ids { ", numbered reports" } else { "" }
            );
            if !i.descriptor.is_empty() {
                for line in descriptor::hex(&i.descriptor).lines() {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
        if ask && !d.simulated && usb::access(d) == Access::Allowed {
            query(d, &mut out);
        }
    }
    let _ = writeln!(out, "\n== lsusb -v -d 1b1c: ==\n{}", lsusb());
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn simulated_report_lists_interfaces() {
        // SAFETY: tests in this crate only ever set this one value.
        unsafe { std::env::set_var("CORSAIR_MOCK", "galleon-100-sd") };
        let r = super::report(true);
        assert!(r.contains("== 1b1c:2b0c CORSAIR GALLEON 100 SD"), "{r}");
        assert!(r.contains("known model: Corsair Galleon 100 SD"));
        assert!(r.contains("interface 1: class 03 protocol 00, hidraw91, Vendor-defined (0xFF42), input 64 / output 64 bytes"));
        assert!(!r.contains("Bragi GET"), "simulated devices are never queried");
    }
}
