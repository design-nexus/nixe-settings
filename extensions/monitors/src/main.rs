//! External monitors through DDC/CI (ddcutil): brightness, contrast, volume,
//! input, colour preset and power, on the monitor itself.
//!
//! One page per monitor that answers DDC; the laptop's own screen doesn't,
//! so it never gets one.

use anyhow::{Result, bail};
use nyx_ext::page::{self, Page, desc, refreshes};
use nyx_ext::{Extension, access, sys};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const SETUP: &str = "setup";

static RULE: access::Rule = access::Rule {
    file: "45-nyx-ddc.rules",
    text: "# Written by Settings: lets the signed-in user talk to monitors over DDC/CI.\nKERNEL==\"i2c-[0-9]*\", SUBSYSTEM==\"i2c-dev\", TAG+=\"uaccess\"\n",
    subsystems: &["i2c-dev"],
};

#[derive(Debug, Clone, PartialEq)]
struct Monitor {
    bus: u32,
    maker: String,
    model: String,
    serial: String,
}

impl Monitor {
    fn id(&self) -> String {
        format!("bus{}", self.bus)
    }
    fn title(&self) -> String {
        if self.model.is_empty() { format!("Monitor on bus {}", self.bus) } else { self.model.clone() }
    }
}

/// `ddcutil detect --terse`: the monitors that answer, skipping "Invalid display".
fn parse_detect(text: &str) -> Vec<Monitor> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let block = block.trim();
        if !block.starts_with("Display") {
            continue;
        }
        let field = |name: &str| block.lines().find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_string()));
        let Some(bus) = field("I2C bus:").and_then(|b| b.rsplit('-').next().and_then(|n| n.parse().ok())) else { continue };
        let mon = field("Monitor:").unwrap_or_default();
        let mut parts = mon.splitn(3, ':');
        out.push(Monitor {
            bus,
            maker: parts.next().unwrap_or("").to_string(),
            model: parts.next().unwrap_or("").to_string(),
            serial: parts.next().unwrap_or("").to_string(),
        });
    }
    out
}

/// `getvcp … --terse`: code → (current, max) for continuous values, or
/// (value, 0) for choices.
fn parse_values(text: &str) -> BTreeMap<String, (u32, u32)> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["VCP", code, "C", cur, max] => {
                if let (Ok(c), Ok(m)) = (cur.parse(), max.parse()) {
                    out.insert(code.to_uppercase(), (c, m));
                }
            }
            ["VCP", code, "SNC", v, ..] | ["VCP", code, "CNC", v, ..] => {
                if let Ok(n) = u32::from_str_radix(v.trim_start_matches('x').trim_start_matches("0x"), 16) {
                    out.insert(code.to_uppercase(), (n, 0));
                }
            }
            _ => {}
        }
    }
    out
}

/// `capabilities`: each feature's choices (code → [(value, label)]).
fn parse_capabilities(text: &str) -> BTreeMap<String, Vec<(u32, String)>> {
    let mut out: BTreeMap<String, Vec<(u32, String)>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Feature:") {
            current = rest.split_whitespace().next().map(|c| c.to_uppercase());
            if let Some(c) = &current {
                out.entry(c.clone()).or_default();
            }
            continue;
        }
        if let (Some(c), Some((v, label))) = (&current, t.split_once(": "))
            && v.len() <= 2
            && let Ok(n) = u32::from_str_radix(v, 16)
        {
            out.entry(c.clone()).or_default().push((n, label.trim().to_string()));
        }
    }
    out
}

fn monitors() -> Vec<Monitor> {
    sys::run("ddcutil", &["detect", "--terse"]).map(|t| parse_detect(&t)).unwrap_or_default()
}

/// Capabilities don't change: read once per monitor and keep them.
fn capabilities(m: &Monitor) -> BTreeMap<String, Vec<(u32, String)>> {
    let cache = sys::state_dir().join(format!("caps-{}-{}.txt", m.model.replace('/', "_"), m.serial.replace('/', "_")));
    let text = match std::fs::read_to_string(&cache) {
        Ok(t) => t,
        Err(_) => {
            let t = sys::run("ddcutil", &["--bus", &m.bus.to_string(), "capabilities"]).unwrap_or_default();
            if !t.is_empty() {
                let _ = std::fs::write(&cache, &t);
            }
            t
        }
    };
    parse_capabilities(&text)
}

fn can_reach() -> bool {
    std::fs::read_dir("/dev")
        .map(|rd| {
            let buses: Vec<_> = rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("i2c-")).collect();
            buses.is_empty() || buses.iter().any(|e| std::fs::OpenOptions::new().read(true).write(true).open(e.path()).is_ok())
        })
        .unwrap_or(true)
}

struct Monitors;

impl Extension for Monitors {
    fn pages(&self) -> Value {
        if !sys::present("ddcutil") {
            return json!([]);
        }
        if !can_reach() {
            return json!([page::entry(SETUP, "Monitors", "video-display-symbolic", "Allow your account to control external monitors.", "monitor ddc brightness")]);
        }
        Value::Array(
            monitors()
                .iter()
                .map(|m| {
                    page::entry(
                        &m.id(),
                        &m.title(),
                        "video-display-symbolic",
                        "Brightness, contrast, volume, input and power on the monitor itself.",
                        &format!("monitor display ddc brightness contrast input hdmi displayport volume {}", m.model.to_lowercase()),
                    )
                })
                .collect(),
        )
    }

    fn describe(&self, id: &str) -> Result<Value> {
        if id == SETUP {
            return Ok(Page::new("").banner(access::BANNER, true).group("Device access", "", RULE.rows("monitor control")).build());
        }
        let Some(m) = monitors().into_iter().find(|m| m.id() == id) else { bail!("that monitor isn't answering any more") };
        let bus = m.bus.to_string();
        let values = parse_values(&sys::run("ddcutil", &["--bus", &bus, "getvcp", "10", "12", "62", "60", "14", "D6", "--terse"]).unwrap_or_default());
        let caps = capabilities(&m);
        let mut picture = Vec::new();
        for (code, key, title) in [("10", "10", "Brightness"), ("12", "12", "Contrast")] {
            if let Some((cur, max)) = values.get(code).filter(|(_, max)| *max > 0) {
                picture.push(page::slider(key, title, *cur as f64, (0.0, *max as f64, 1.0), ""));
            }
        }
        if values.contains_key("10") && laptop_brightness().is_some() {
            picture.push(desc(page::button("match", "Match the laptop screen", "Match"), "Sets this monitor's brightness to the built-in screen's."));
        }
        let choice_row = |code: &str, title: &str| -> Option<Value> {
            let opts = caps.get(code).filter(|o| !o.is_empty())?;
            let cur = values.get(code).map(|v| v.0).unwrap_or(0);
            let opts: Vec<(String, String)> = opts.iter().map(|(v, l)| (format!("{v:02x}"), l.clone())).collect();
            Some(refreshes(page::choice_owned(code, title, &format!("{cur:02x}"), &opts)))
        };
        if let Some(r) = choice_row("14", "Colour preset") {
            picture.push(r);
        }
        let mut sound_input = Vec::new();
        if let Some(r) = choice_row("60", "Input") {
            sound_input.push(desc(r, "Switching away shows another computer's picture; switch back on the monitor."));
        }
        if let Some((cur, max)) = values.get("62").filter(|(_, max)| *max > 0) {
            sound_input.push(page::slider("62", "Speaker volume", *cur as f64, (0.0, *max as f64, 1.0), ""));
        }
        let mut power = Vec::new();
        if caps.contains_key("D6") {
            power.push(desc(page::button("standby", "Turn the monitor off", "Off"), "It wakes again with its power button or a new picture."));
        }
        let mut info = vec![page::info("model", "Model", &m.model), page::info("maker", "Maker code", &m.maker)];
        if !m.serial.is_empty() {
            info.push(page::info("serial", "Serial", &m.serial));
        }
        info.push(page::info("bus", "I2C bus", &format!("/dev/i2c-{}", m.bus)));
        Ok(Page::new(&format!("<b>{}</b> · DDC/CI", m.title().replace('&', "&amp;")))
            .group("Picture", "", picture)
            .group("Input and sound", "", sound_input)
            .group("Power", "", power)
            .group("Monitor", "", info)
            .build())
    }

    fn set(&self, page: &str, key: &str, value: &str) -> Result<Option<Value>> {
        if key == "access" {
            RULE.install()?;
            return Ok(nyx_ext::reload("Monitor control allowed"));
        }
        let bus = page.trim_start_matches("bus").to_string();
        if bus.parse::<u32>().is_err() {
            bail!("no such monitor");
        }
        match key {
            "10" | "12" | "62" => {
                let v: f64 = value.parse()?;
                sys::run("ddcutil", &["--bus", &bus, "--noverify", "setvcp", key, &(v.round() as u32).to_string()])?;
                Ok(None)
            }
            "60" | "14" => {
                if !value.chars().all(|c| c.is_ascii_hexdigit()) || value.len() > 2 {
                    bail!("not a value this monitor offers");
                }
                sys::run("ddcutil", &["--bus", &bus, "setvcp", key, &format!("x{value}")])?;
                Ok(nyx_ext::refresh())
            }
            "standby" => {
                sys::run("ddcutil", &["--bus", &bus, "--noverify", "setvcp", "D6", "04"])?;
                Ok(nyx_ext::toast("Monitor off"))
            }
            "match" => {
                let pct = laptop_brightness().unwrap_or(100);
                let max = parse_values(&sys::run("ddcutil", &["--bus", &bus, "getvcp", "10", "--terse"])?).get("10").map(|v| v.1).unwrap_or(100);
                sys::run("ddcutil", &["--bus", &bus, "--noverify", "setvcp", "10", &(pct * max / 100).to_string()])?;
                Ok(nyx_ext::refresh())
            }
            _ => bail!("unknown setting {key}"),
        }
    }
}

/// The built-in screen's brightness in percent.
fn laptop_brightness() -> Option<u32> {
    let dir = std::fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
    let cur = sys::read_num(dir.join("brightness"))? as f64;
    let max = sys::read_num(dir.join("max_brightness"))? as f64;
    (max > 0.0).then(|| (cur / max * 100.0).round() as u32)
}

fn main() {
    nyx_ext::run(&Monitors);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_monitors_that_answer() {
        let t = "Display 1\n   I2C bus:  /dev/i2c-4\n   DRM connector:  card1-DP-1\n   Monitor:  DEL:DELL U2720Q:7ABC\n\n\
                 Invalid display\n   I2C bus:  /dev/i2c-15\n   Monitor:  SDC:ATNA60DL01-0:\n";
        let m = parse_detect(t);
        assert_eq!(m, vec![Monitor { bus: 4, maker: "DEL".into(), model: "DELL U2720Q".into(), serial: "7ABC".into() }]);
        assert_eq!(m[0].id(), "bus4");
    }

    #[test]
    fn reads_values() {
        let v = parse_values("VCP 10 C 50 100\nVCP 12 C 75 100\nVCP 60 SNC x0f\nVCP D6 SNC x01\nVCP 62 ERR\n");
        assert_eq!(v["10"], (50, 100));
        assert_eq!(v["60"], (0x0f, 0));
        assert_eq!(v["D6"], (1, 0));
        assert!(!v.contains_key("62"));
    }

    #[test]
    fn reads_choices() {
        let t = "VCP Features:\n   Feature: 10 (Brightness)\n   Feature: 60 (Input Source)\n      Values:\n         0f: DisplayPort-1\n         11: HDMI-1\n         1b: USB-C\n   Feature: 14 (Select color preset)\n      Values:\n         05: 6500 K\n";
        let c = parse_capabilities(t);
        assert_eq!(c["60"], vec![(0x0f, "DisplayPort-1".into()), (0x11, "HDMI-1".into()), (0x1b, "USB-C".into())]);
        assert_eq!(c["14"], vec![(5, "6500 K".into())]);
        assert!(c["10"].is_empty());
    }
}
