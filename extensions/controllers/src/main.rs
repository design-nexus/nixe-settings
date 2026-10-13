//! Game controllers: Xbox, PlayStation, Switch Pro, 8BitDo and others the
//! kernel drives. Battery, a live tester (buttons, sticks and triggers, read
//! every second while the page shows), a rumble test, and the lightbar and
//! player lights on PlayStation controllers.

mod input;

use anyhow::{Context, Result, bail};
use input::Pad;
use nixe_ext::page::{self, Page};
use nixe_ext::{Extension, access, sys};
use serde_json::{Value, json};
use std::path::PathBuf;

/// PlayStation controllers' lights are root's until this rule lets the user write them.
static RULE: access::Rule = access::Rule {
    file: "47-nixe-controllers.rules",
    text: "# Written by Settings: lets the signed-in user set game controllers' lights.\n\
SUBSYSTEM==\"leds\", KERNEL==\"*:rgb:indicator\", RUN+=\"/bin/chmod 0666 /sys%p/brightness /sys%p/multi_intensity\"\n\
SUBSYSTEM==\"leds\", KERNEL==\"*:white:player-*\", RUN+=\"/bin/chmod 0666 /sys%p/brightness\"\n",
    subsystems: &["leds"],
};

/// The controller's LEDs: (lightbar folder, player LED folders).
fn leds(pad: &Pad) -> (Option<PathBuf>, Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir("/sys/class/leds") else { return (None, Vec::new()) };
    let mut bar = None;
    let mut players = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // The LEDs sit on the same HID device as the controller's input.
        let dev = std::fs::canonicalize(e.path().join("device")).ok();
        let same = dev.as_ref().is_some_and(|d| pad.hid.as_ref().is_some_and(|h| d.starts_with(h) || h.starts_with(d)));
        if !same {
            continue;
        }
        if name.ends_with(":rgb:indicator") {
            bar = Some(e.path());
        } else if name.contains(":white:player-") {
            players.push(e.path());
        }
    }
    players.sort();
    (bar, players)
}

fn battery(pad: &Pad) -> Option<(i64, String)> {
    let rd = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if sys::read(p.join("scope")).as_deref() != Some("Device") {
            continue;
        }
        let dev = std::fs::canonicalize(p.join("device")).ok()?;
        if pad.hid.as_ref().is_some_and(|h| dev.starts_with(h) || h.starts_with(&dev)) {
            let cap = sys::read_num(p.join("capacity")).or_else(|| {
                // Some report a level instead of a number.
                sys::read(p.join("capacity_level")).map(|l| match l.as_str() {
                    "Full" => 100,
                    "High" => 75,
                    "Normal" => 50,
                    "Low" => 20,
                    "Critical" => 5,
                    _ => -1,
                })
            })?;
            return Some((cap, sys::read(p.join("status")).unwrap_or_default()));
        }
    }
    None
}

fn bar(v: f64) -> String {
    let n = ((v.clamp(0.0, 1.0)) * 10.0).round() as usize;
    format!("{}{} {:>3.0}%", "█".repeat(n), "░".repeat(10 - n), v * 100.0)
}

fn stick(x: f64, y: f64) -> String {
    format!("x {:+.0}%  y {:+.0}%", x * 100.0, y * 100.0)
}

struct Controllers;

impl Extension for Controllers {
    fn pages(&self) -> Value {
        Value::Array(
            input::pads()
                .iter()
                .map(|p| {
                    page::entry(&p.id(), &p.title(), "input-gaming-symbolic", "Battery, a live button tester, rumble and lights.",
                        &format!("controller gamepad joystick xbox playstation dualsense dualshock switch 8bitdo rumble tester battery {}", p.name.to_lowercase()))
                })
                .collect(),
        )
    }

    fn describe(&self, id: &str) -> Result<Value> {
        let pad = input::pads().into_iter().find(|p| p.id() == id).context("that controller isn't connected any more")?;
        let mut about = Vec::new();
        if let Some((cap, status)) = battery(&pad) {
            let v = if cap < 0 { status } else { format!("{cap}% · {status}") };
            about.push(page::info("battery", "Battery", &v));
        }
        about.push(page::info("connection", "Connected by", if pad.bluetooth { "Bluetooth" } else { "USB" }));
        about.push(page::info("ids", "USB id", &format!("{:04x}:{:04x}", pad.vendor, pad.product)));

        let mut tester = Vec::new();
        match input::state(&pad) {
            Ok(s) => {
                let held: Vec<&str> = s.buttons.iter().map(String::as_str).collect();
                tester.push(page::info("buttons", "Held", if held.is_empty() { "Nothing: press and hold a button" } else { "" }));
                if !held.is_empty() {
                    tester.last_mut().unwrap()["value"] = json!(held.join("  "));
                }
                if let Some((x, y)) = s.left {
                    tester.push(page::info("left", "Left stick", &stick(x, y)));
                }
                if let Some((x, y)) = s.right {
                    tester.push(page::info("right", "Right stick", &stick(x, y)));
                }
                if let Some(t) = s.lt {
                    tester.push(page::info("lt", "Left trigger", &bar(t)));
                }
                if let Some(t) = s.rt {
                    tester.push(page::info("rt", "Right trigger", &bar(t)));
                }
                if let Some((x, y)) = s.dpad {
                    tester.push(page::info("dpad", "D-pad", &stick(x, y)));
                }
            }
            Err(e) => tester.push(page::info("tester", "Tester", &format!("Can't read it: {e}"))),
        }
        let mut feel = Vec::new();
        if pad.rumble {
            feel.push(page::buttons("rumble", "Rumble", &[("light", "Light"), ("strong", "Strong"), ("both", "Both")]));
        }
        let (lightbar, players) = leds(&pad);
        let mut lights = Vec::new();
        let writable = |p: &PathBuf, f: &str| std::fs::OpenOptions::new().write(true).open(p.join(f)).is_ok();
        if let Some(b) = &lightbar {
            let rgb: Vec<u8> = sys::read(b.join("multi_intensity")).unwrap_or_default().split_whitespace().filter_map(|x| x.parse().ok()).collect();
            let hex = if rgb.len() == 3 { sys::hex(rgb[0], rgb[1], rgb[2]) } else { "#0000ff".into() };
            if writable(b, "multi_intensity") {
                lights.push(page::colour_with_theme("lightbar", "Lightbar", &hex, sys::theme_accent().as_deref()));
            }
        }
        if !players.is_empty() && players.iter().all(|p| writable(p, "brightness")) {
            let lit = players.iter().filter(|p| sys::read_num(p.join("brightness")).unwrap_or(0) > 0).count();
            let opts: Vec<(String, String)> = (0..=players.len()).map(|n| (n.to_string(), if n == 0 { "Off".into() } else { n.to_string() })).collect();
            let mut row = page::choice_owned("players", "Player lights", &lit.to_string(), &opts);
            row["kind"] = json!("segmented");
            lights.push(row);
        }
        // Lights it has but this account can't set yet.
        let has_lights = lightbar.is_some() || !players.is_empty();
        let need_rule = has_lights && lights.is_empty();
        let mut page = Page::new(&format!("<b>{}</b>", pad.title().replace('&', "&amp;"))).poll(1).group("Controller", "", about).group(
            "Tester",
            "Hold buttons and move the sticks: this updates every second.",
            tester,
        );
        page = page.group("Rumble", "", feel).group("Lights", "", lights);
        if need_rule {
            page = page.group("Lights", "", RULE.rows("setting the controller's lights"));
        }
        Ok(page.build())
    }

    fn set(&self, id: &str, key: &str, value: &str) -> Result<Option<Value>> {
        if key == "access" {
            RULE.install()?;
            return Ok(nixe_ext::refresh());
        }
        let pad = input::pads().into_iter().find(|p| p.id() == id).context("that controller isn't connected any more")?;
        match key {
            "rumble" => {
                let (strong, weak) = match value {
                    "light" => (0, 0x8000),
                    "strong" => (0xc000, 0),
                    _ => (0xffff, 0xffff),
                };
                input::rumble(&pad, strong, weak, 600)?;
                Ok(None)
            }
            "lightbar" => {
                let (r, g, b) = sys::parse_hex(value).context("not a colour")?;
                let (Some(bar), _) = leds(&pad) else { bail!("this controller has no lightbar") };
                std::fs::write(bar.join("multi_intensity"), format!("{r} {g} {b}"))?;
                std::fs::write(bar.join("brightness"), "255")?;
                Ok(None)
            }
            "players" => {
                let n: usize = value.parse()?;
                let (_, players) = leds(&pad);
                for (i, p) in players.iter().enumerate() {
                    std::fs::write(p.join("brightness"), if i < n { "1" } else { "0" })?;
                }
                Ok(None)
            }
            _ => bail!("unknown setting {key}"),
        }
    }
}

fn main() {
    nixe_ext::run(&Controllers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readouts() {
        assert_eq!(bar(0.5), "█████░░░░░  50%");
        assert_eq!(stick(-1.0, 0.25), "x -100%  y +25%");
    }
}
