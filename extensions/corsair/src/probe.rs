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

/// Processes that have a hidraw node open (by command name), e.g. a browser on Web Hub.
fn holders(node: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(procs) = std::fs::read_dir("/proc") else { return out };
    for p in procs.flatten() {
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
        if fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == node)) {
            let comm = std::fs::read_to_string(p.path().join("comm")).unwrap_or_default();
            out.push(format!("{} (pid {})", comm.trim(), p.file_name().to_string_lossy()));
        }
    }
    out
}

/// Ask each read-only property on the Bragi interface, listening on every vendor
/// interface for the reply; record requests and raw replies.
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
    // The control interface first, then any other vendor interface that sends input.
    let mut listen: Vec<(u8, hidraw::Hidraw)> = Vec::new();
    match hidraw::Hidraw::open(&node) {
        Ok(h) => listen.push((iface.number, h)),
        Err(e) => {
            let _ = writeln!(out, "  Bragi: couldn't open {}: {e}", node.display());
            return;
        }
    }
    for other in dev.interfaces.iter().filter(|i| i.number != iface.number && i.summary().is_vendor() && i.summary().input_len > 0) {
        if let Some(n) = other.node()
            && let Ok(h) = hidraw::Hidraw::open(&n)
        {
            listen.push((other.number, h));
        }
    }
    let numbers: Vec<String> = listen.iter().map(|(n, _)| n.to_string()).collect();
    let _ = writeln!(out, "  Bragi: asking on interface {} ({}, {}-byte reports), listening on {}", iface.number, node.display(), s.output_len, numbers.join(" and "));
    for i in &dev.interfaces {
        if let Some(n) = i.node() {
            let h = holders(&n);
            if !h.is_empty() {
                let _ = writeln!(out, "  {} (interface {}) is also open in: {}", n.display(), i.number, h.join(", "));
            }
        }
    }
    let mut nodes: Vec<hidraw::Hidraw> = listen.into_iter().map(|(_, h)| h).collect();
    let tag: Vec<String> = numbers;
    // Anything already arriving is background traffic, not an answer.
    let mut background = 0;
    while let Ok(Some((i, r))) = hidraw::read_any(&mut nodes, 300) {
        background += 1;
        if background <= 3 {
            let _ = writeln!(out, "    before asking, interface {}: {}", tag[i], descriptor::hex(&r[..16.min(r.len())]));
        }
        if background > 50 {
            break;
        }
    }
    let _ = writeln!(out, "    {background} report(s) arrived before asking");

    for header in bragi::HEADERS {
        let _ = writeln!(out, "    header 0x{header:02x}:");
        let mut answered = 0;
        for (prop, name) in bragi::PROPERTIES {
            let Some(req) = bragi::get_request(header, *prop, s.output_len) else { continue };
            let _ = write!(out, "      0x{prop:02x} {name}: ");
            if let Err(e) = nodes[0].write(&req) {
                let _ = writeln!(out, "write failed: {e}");
                continue;
            }
            let start = std::time::Instant::now();
            let mut lines = Vec::new();
            while start.elapsed().as_millis() < 1000 && lines.len() < 6 {
                let left = 1000 - start.elapsed().as_millis() as i32;
                match hidraw::read_any(&mut nodes, left.max(1)) {
                    Ok(Some((i, r))) => {
                        let decoded = bragi::reply_value(&r).map(|v| format!(" (value {v} / 0x{v:x})")).unwrap_or_default();
                        lines.push(format!("interface {}: {}{decoded}", tag[i], descriptor::hex(&r[..16.min(r.len())])));
                        if bragi::is_get_reply(&r) {
                            answered += 1;
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        lines.push(format!("read failed: {e}"));
                        break;
                    }
                }
            }
            if lines.is_empty() {
                let _ = writeln!(out, "no reply in 1 s");
            } else {
                let _ = writeln!(out);
                for l in lines {
                    let _ = writeln!(out, "        {l}");
                }
            }
        }
        // The wired header answered: the other one isn't needed.
        if answered > 0 {
            break;
        }
    }
}

/// The raw USB device and configuration descriptors (sysfs), when lsusb isn't installed.
fn usb_descriptors(dev: &Device) -> Option<String> {
    let raw = std::fs::read(std::path::Path::new("/sys/bus/usb/devices").join(&dev.port).join("descriptors")).ok()?;
    Some(descriptor::hex(&raw))
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
            if i.class == 9 {
                let _ = writeln!(out, "  interface {}: USB hub (inside the keyboard)", i.number);
                continue;
            }
            if i.descriptor.is_empty() {
                let _ = writeln!(out, "  interface {}: class {:02x}, no HID", i.number, i.class);
                continue;
            }
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
        if !d.simulated
            && let Some(raw) = usb_descriptors(d)
        {
            let _ = writeln!(out, "  USB descriptors:");
            for line in raw.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
        if ask && !d.simulated && usb::access(d) == Access::Allowed {
            query(d, &mut out);
        }
    }
    // Optional: the sysfs descriptors above cover the same ground.
    let ls = lsusb();
    if !ls.starts_with("(lsusb unavailable") {
        let _ = writeln!(out, "\n== lsusb -v -d 1b1c: ==\n{ls}");
    }
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
