//! Razer mice, keyboards and headsets through OpenRazer's daemon (D-Bus,
//! `org.razer` on the session bus): DPI, polling rate, battery, idle time,
//! lighting effects and brightness.

use anyhow::{Context, Result, bail};
use nyx_ext::page::{self, Page, desc, refreshes};
use nyx_ext::{Extension, sys};
use serde_json::{Value, json};

const SETUP: &str = "setup";

/// A `busctl --user call` to the daemon; its reply as text.
fn call(path: &str, iface: &str, method: &str, args: &[&str]) -> Result<String> {
    let mut a = vec!["--user", "call", "org.razer", path, iface, method];
    a.extend(args);
    sys::run("busctl", &a)
}

/// busctl's reply: its type, then the values ("s \"Basilisk\"", "aq 800 800", "d 87.5").
fn values(reply: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = reply.trim();
    // Skip the signature.
    if let Some(i) = rest.find(' ') {
        rest = &rest[i + 1..];
    } else {
        return out;
    }
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            s.push(n);
                        }
                    }
                    '"' => break,
                    _ => s.push(c),
                }
            }
            out.push(s);
        } else if !c.is_whitespace() {
            let mut s = c.to_string();
            while let Some(&n) = chars.peek() {
                if n.is_whitespace() {
                    break;
                }
                s.push(n);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

fn get(path: &str, iface: &str, method: &str) -> Option<Vec<String>> {
    call(path, iface, method, &[]).ok().map(|r| values(&r))
}

fn first(path: &str, iface: &str, method: &str) -> Option<String> {
    get(path, iface, method)?.into_iter().last()
}

#[derive(Debug)]
struct Device {
    serial: String,
    path: String,
    name: String,
    kind: String,
}

fn devices() -> Vec<Device> {
    let Some(serials) = get("/org/razer", "razer.devices", "getDevices") else { return Vec::new() };
    // "as N serial…": the first value is the count.
    serials
        .into_iter()
        .skip(1)
        .map(|serial| {
            let path = format!("/org/razer/device/{serial}");
            let name = first(&path, "razer.device.misc", "getDeviceName").unwrap_or_else(|| "Razer device".into());
            let kind = first(&path, "razer.device.misc", "getDeviceType").unwrap_or_default();
            Device { serial, path, name, kind }
        })
        .collect()
}

fn icon(kind: &str) -> &'static str {
    match kind {
        "mouse" => "input-mouse-symbolic",
        "keyboard" | "keypad" => "input-keyboard-symbolic",
        "headset" => "audio-headset-symbolic",
        _ => "input-gaming-symbolic",
    }
}

/// The lighting effects offered, by the method each uses.
const EFFECTS: &[(&str, &str)] = &[
    ("setStatic", "Still colour"),
    ("setBreathSingle", "Breathing"),
    ("setSpectrum", "Colour cycle"),
    ("setWave", "Wave"),
    ("setReactive", "React to presses"),
    ("setNone", "Off"),
];

struct Razer;

impl Extension for Razer {
    fn pages(&self) -> Value {
        if !sys::present("openrazer-daemon") && !std::path::Path::new("/usr/bin/openrazer-daemon").exists() {
            return json!([]);
        }
        let devs = devices();
        if devs.is_empty() {
            // The daemon may not be running, or the user not in its group yet.
            return if razer_plugged_in() {
                json!([page::entry(SETUP, "Razer", "input-mouse-symbolic", "Set up OpenRazer for your Razer device.", "razer openrazer")])
            } else {
                json!([])
            };
        }
        Value::Array(
            devs.iter()
                .map(|d| page::entry(&d.serial, &d.name, icon(&d.kind), "DPI, polling rate, battery, lighting.", &format!("razer {} dpi polling battery lighting chroma {}", d.kind, d.name.to_lowercase())))
                .collect(),
        )
    }

    fn describe(&self, id: &str) -> Result<Value> {
        if id == SETUP {
            return Ok(Page::new("")
                .banner("OpenRazer is installed, but its daemon isn't answering. Your account needs to be in the <tt>plugdev</tt> group, and the daemon running.", true)
                .group("Set up", "", vec![desc(page::button("setup", "Set up OpenRazer", "Set up…"), "Adds you to <tt>plugdev</tt> and starts the daemon. Asks for your password; sign out and in afterwards.")])
                .build());
        }
        let d = devices().into_iter().find(|d| d.serial == id).context("that device isn't connected any more")?;
        let p = &d.path;
        let mut perf = Vec::new();
        if let Some(dpi) = get(p, "razer.device.dpi", "getDPI") {
            let max = first(p, "razer.device.dpi", "maxDPI").and_then(|m| m.parse::<f64>().ok()).unwrap_or(16000.0);
            if let Some(x) = dpi.get(1).and_then(|v| v.parse::<f64>().ok()) {
                perf.push(desc(page::slider("dpi", "Sensitivity (DPI)", x, (100.0, max, 50.0), ""), "Higher moves the pointer further for the same hand movement."));
            }
        }
        if let Some(rate) = first(p, "razer.device.misc", "getPollRate") {
            let opts: Vec<(String, String)> = ["125", "500", "1000"].iter().map(|r| (r.to_string(), format!("{r} Hz"))).collect();
            let mut row = page::choice_owned("poll", "Polling rate", &rate, &opts);
            row["kind"] = json!("segmented");
            perf.push(desc(row, "How often it reports its position. 1000 Hz is smoothest; lower saves a wireless battery."));
        }
        let mut power = Vec::new();
        if let Some(b) = first(p, "razer.device.power", "getBattery").and_then(|v| v.parse::<f64>().ok()) {
            let charging = first(p, "razer.device.power", "isCharging").is_some_and(|c| c == "true");
            power.push(page::info("battery", "Battery", &format!("{b:.0}%{}", if charging { " · charging" } else { "" })));
            if let Some(idle) = first(p, "razer.device.power", "getIdleTime").and_then(|v| v.parse::<f64>().ok()) {
                power.push(desc(page::slider("idle", "Sleep after", (idle / 60.0).round(), (1.0, 15.0, 1.0), " min"), "Idle time before it sleeps to save the battery."));
            }
        }
        let mut light = Vec::new();
        if call(p, "razer.device.lighting.chroma", "getEffect", &[]).is_ok() || call(p, "razer.device.lighting.brightness", "getBrightness", &[]).is_ok() {
            let opts: Vec<(String, String)> = EFFECTS.iter().map(|(m, l)| (m.to_string(), l.to_string())).collect();
            light.push(refreshes(page::choice_owned("effect", "Effect", "", &opts)));
            light.push(page::colour_with_theme("colour", "Colour", &sys::read(sys::state_dir().join(format!("{}.colour", d.serial))).unwrap_or_else(|| "#00ff00".into()), sys::theme_accent().as_deref()));
            if let Some(b) = first(p, "razer.device.lighting.brightness", "getBrightness").and_then(|v| v.parse::<f64>().ok()) {
                light.push(page::slider("brightness", "Brightness", b.round(), (0.0, 100.0, 5.0), " %"));
            }
        }
        let info = vec![page::info("kind", "Kind", &d.kind), page::info("serial", "Serial", &d.serial)];
        Ok(Page::new(&format!("<b>{}</b>", d.name.replace('&', "&amp;")))
            .group("Pointer", "", perf)
            .group("Power", "", power)
            .group("Lighting", "", light)
            .group("Device", "", info)
            .build())
    }

    fn set(&self, id: &str, key: &str, value: &str) -> Result<Option<Value>> {
        if key == "setup" {
            let user = std::env::var("USER").context("who am I?")?;
            sys::run_root("gpasswd", &["-a", &user, "plugdev"])?;
            let _ = sys::run("systemctl", &["--user", "enable", "--now", "openrazer-daemon.service"]);
            return Ok(nyx_ext::reload("Done: sign out and back in to finish"));
        }
        let p = format!("/org/razer/device/{id}");
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            bail!("no such device");
        }
        let colour = || sys::read(sys::state_dir().join(format!("{id}.colour"))).and_then(|c| sys::parse_hex(&c)).unwrap_or((0, 255, 0));
        match key {
            "dpi" => {
                let v = (value.parse::<f64>()?.round() as u32).to_string();
                call(&p, "razer.device.dpi", "setDPI", &["qq", &v, &v])?;
            }
            "poll" => {
                call(&p, "razer.device.misc", "setPollRate", &["q", value])?;
            }
            "idle" => {
                let secs = (value.parse::<f64>()? * 60.0).round() as u32;
                call(&p, "razer.device.power", "setIdleTime", &["q", &secs.to_string()])?;
            }
            "brightness" => {
                call(&p, "razer.device.lighting.brightness", "setBrightness", &["d", value])?;
            }
            "colour" => {
                let (r, g, b) = sys::parse_hex(value).context("not a colour")?;
                std::fs::write(sys::state_dir().join(format!("{id}.colour")), value)?;
                call(&p, "razer.device.lighting.chroma", "setStatic", &["yyy", &r.to_string(), &g.to_string(), &b.to_string()])?;
            }
            "effect" => {
                let (r, g, b) = colour();
                let (r, g, b) = (r.to_string(), g.to_string(), b.to_string());
                let args: Vec<&str> = match value {
                    "setStatic" | "setBreathSingle" => vec!["yyy", &r, &g, &b],
                    "setWave" => vec!["i", "1"],
                    "setReactive" => vec!["yyyy", &r, &g, &b, "2"],
                    "setSpectrum" | "setNone" => vec![],
                    _ => bail!("not an effect this offers"),
                };
                call(&p, "razer.device.lighting.chroma", value, &args)?;
            }
            _ => bail!("unknown setting {key}"),
        }
        Ok(None)
    }
}

/// Any Razer USB device (vendor 1532) plugged in.
fn razer_plugged_in() -> bool {
    std::fs::read_dir("/sys/bus/usb/devices")
        .map(|rd| rd.flatten().any(|e| sys::read(e.path().join("idVendor")).as_deref() == Some("1532")))
        .unwrap_or(false)
}

fn main() {
    nyx_ext::run(&Razer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_busctl_replies() {
        assert_eq!(values("s \"Razer Basilisk V3\""), vec!["Razer Basilisk V3"]);
        assert_eq!(values("aq 2 800 800"), vec!["2", "800", "800"]);
        assert_eq!(values("as 2 \"PM1234\" \"XX9\""), vec!["2", "PM1234", "XX9"]);
        assert_eq!(values("d 87.5"), vec!["87.5"]);
        assert_eq!(values("s \"say \\\"hi\\\"\""), vec!["say \"hi\""]);
        assert!(values("").is_empty());
    }
}
