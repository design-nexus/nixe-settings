//! Laptops of any brand, through the kernel's standard controls: battery
//! health and charge limits, power profile, keyboard backlight, and switches
//! for the microphone and camera. ThinkPad, Dell, Framework, HP, LG, Lenovo,
//! Huawei, Samsung and others expose these the same way.
//!
//! On an ASUS with asusctl the ASUS extension does all this, so this one
//! stays out of the way.

use anyhow::{Context, Result, bail};
use nixe_ext::page::{self, Page, desc, refreshes};
use nixe_ext::{Extension, sys};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const PAGE: &str = "laptop";
const RULE: &str = "/etc/udev/rules.d/46-nixe-battery.rules";

fn battery() -> Option<PathBuf> {
    let rd = std::fs::read_dir("/sys/class/power_supply").ok()?;
    let mut bats: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| sys::read(p.join("type")).as_deref() == Some("Battery") && p.join("capacity").exists()).collect();
    bats.sort();
    // The laptop's own battery, not a mouse's or a controller's.
    bats.into_iter().find(|p| sys::read(p.join("scope")).as_deref() != Some("Device"))
}

fn kbd_backlight() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/leds").ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains("kbd_backlight")))
}

/// An ASUS that the ASUS extension looks after.
fn asus_handled() -> bool {
    if std::env::var_os("NIXE_LAPTOP_SHOW").is_some() {
        return false;
    }
    sys::read("/sys/class/dmi/id/sys_vendor").is_some_and(|v| v.to_uppercase().contains("ASUS")) && sys::present("asusctl")
}

fn laptop() -> bool {
    let chassis = sys::read_num("/sys/class/dmi/id/chassis_type").unwrap_or(0);
    // 8 portable, 9 laptop, 10 notebook, 14 sub-notebook, 31 convertible, 32 detachable.
    battery().is_some() && ([8, 9, 10, 14, 31, 32].contains(&chassis) || chassis == 0)
}

/// The rule that sets the charge limits at every start (the firmware forgets
/// them on many laptops), from the values now set.
fn rule_text(name: &str, start: Option<i64>, end: Option<i64>) -> String {
    let mut attrs = String::new();
    if let Some(s) = start {
        attrs.push_str(&format!(", ATTR{{charge_control_start_threshold}}=\"{s}\""));
    }
    if let Some(e) = end {
        attrs.push_str(&format!(", ATTR{{charge_control_end_threshold}}=\"{e}\""));
    }
    format!("# Written by Settings: battery charge limits, set again at every start.\nACTION==\"add\", SUBSYSTEM==\"power_supply\", KERNEL==\"{name}\"{attrs}\n")
}

/// Write the limits now and keep them, as root (asks for a password).
fn set_limits(bat: &Path, start: Option<i64>, end: Option<i64>) -> Result<()> {
    let name = bat.file_name().context("no battery")?.to_string_lossy().to_string();
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let tmp = dir.join("46-nixe-battery.rules");
    std::fs::write(&tmp, rule_text(&name, start, end))?;
    // Lowering the stop limit below the start limit is refused: change start first.
    let mut script = String::new();
    for (file, v) in [("charge_control_start_threshold", start), ("charge_control_end_threshold", end)] {
        if let Some(v) = v {
            script.push_str(&format!("echo {v} > '{}' 2>/dev/null; ", bat.join(file).display()));
        }
    }
    for (file, v) in [("charge_control_end_threshold", end), ("charge_control_start_threshold", start)] {
        if let Some(v) = v {
            script.push_str(&format!("echo {v} > '{}'; ", bat.join(file).display()));
        }
    }
    script.push_str(&format!("install -Dm644 '{}' '{RULE}'", tmp.display()));
    let r = sys::run("pkexec", &["sh", "-c", &script]);
    let _ = std::fs::remove_file(&tmp);
    r.map(|_| ())
}

fn profiles() -> (String, Vec<String>) {
    let text = sys::run("powerprofilesctl", &["list"]).unwrap_or_default();
    let mut current = String::new();
    let mut all = Vec::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.trim().is_empty() || !line.trim_end().ends_with(':') {
            continue;
        }
        let name = line.trim().trim_start_matches("* ").trim_end_matches(':').to_string();
        if line.trim_start().starts_with('*') {
            current = name.clone();
        }
        all.push(name);
    }
    (current, all)
}

fn profile_label(p: &str) -> &str {
    match p {
        "power-saver" => "Battery saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

fn mic_muted() -> Option<bool> {
    sys::run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).ok().map(|t| t.contains("[MUTED]"))
}

fn camera_on() -> bool {
    sys::read("/proc/modules").is_some_and(|m| m.lines().any(|l| l.starts_with("uvcvideo ")))
}

struct Laptop;

impl Extension for Laptop {
    fn pages(&self) -> Value {
        if !laptop() || asus_handled() {
            return json!([]);
        }
        json!([page::entry(PAGE, "Laptop", "computer-laptop-symbolic", "Battery health and charge limit, power profile, keyboard light, mic and camera.",
            "laptop battery charge limit threshold health cycles power profile keyboard backlight camera microphone")])
    }

    fn describe(&self, _id: &str) -> Result<Value> {
        let bat = battery().context("there's no battery")?;
        let mut battery_rows = Vec::new();
        let cap = sys::read_num(bat.join("capacity")).unwrap_or(0);
        let status = sys::read(bat.join("status")).unwrap_or_default();
        battery_rows.push(page::info("charge", "Charge", &format!("{cap}% · {status}")));
        let full = sys::read_num(bat.join("charge_full")).or_else(|| sys::read_num(bat.join("energy_full")));
        let design = sys::read_num(bat.join("charge_full_design")).or_else(|| sys::read_num(bat.join("energy_full_design")));
        if let (Some(f), Some(d)) = (full, design)
            && d > 0
        {
            battery_rows.push(desc(
                page::info("health", "Health", &format!("{}% of its original capacity", f * 100 / d)),
                "Batteries hold less as they age; below about 80% you'll notice shorter runs.",
            ));
        }
        if let Some(c) = sys::read_num(bat.join("cycle_count")).filter(|c| *c > 0) {
            battery_rows.push(page::info("cycles", "Charge cycles", &c.to_string()));
        }
        let mut limit_rows = Vec::new();
        if let Some(end) = sys::read_num(bat.join("charge_control_end_threshold")) {
            limit_rows.push(desc(
                page::slider("end", "Stop charging at", end as f64, (50.0, 100.0, 5.0), " %"),
                "Keeping it below full makes the battery last more years. 80% is a good everyday limit.",
            ));
            if end < 100 {
                limit_rows.push(page::button("full", "Charge to 100% once", "Charge fully"));
            }
        }
        if let Some(start) = sys::read_num(bat.join("charge_control_start_threshold")) {
            limit_rows.push(desc(
                page::slider("start", "Start charging below", start as f64, (20.0, 95.0, 5.0), " %"),
                "Plugged in, it waits until the charge drops this low before charging again.",
            ));
        }
        let (current, all) = profiles();
        let mut power = Vec::new();
        if !all.is_empty() {
            let opts: Vec<(String, String)> = all.iter().map(|p| (p.clone(), profile_label(p).to_string())).collect();
            let mut row = page::choice_owned("profile", "Power profile", &current, &opts);
            row["kind"] = json!("segmented");
            power.push(desc(row, "Battery saver runs cooler and longer; Performance is faster and louder."));
        }
        if let Some(led) = kbd_backlight() {
            let max = sys::read_num(led.join("max_brightness")).unwrap_or(0);
            if max > 0 {
                power.push(page::slider("kbd", "Keyboard light", sys::read_num(led.join("brightness")).unwrap_or(0) as f64, (0.0, max as f64, 1.0), ""));
            }
        }
        let mut privacy = Vec::new();
        if let Some(muted) = mic_muted() {
            privacy.push(desc(page::switch("mic", "Microphone", !muted), "Off mutes the default microphone for every app."));
        }
        if std::path::Path::new("/sys/module/uvcvideo").exists() || camera_on() || has_camera() {
            privacy.push(refreshes(desc(page::switch("camera", "Camera", camera_on()), "Off unloads the camera's driver, so no app can use it until it's back on. Asks for your password.")));
        }
        let vendor = sys::read("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
        let product = sys::read("/sys/class/dmi/id/product_name").unwrap_or_default();
        Ok(Page::new(&format!("<b>{}</b> · {}", product.replace('&', "&amp;"), vendor.replace('&', "&amp;")))
            .poll(30)
            .group("Battery", "", battery_rows)
            .group("Charge limit", "", limit_rows)
            .group("Power", "", power)
            .group("Privacy", "", privacy)
            .build())
    }

    fn set(&self, _page: &str, key: &str, value: &str) -> Result<Option<Value>> {
        let bat = battery().context("there's no battery")?;
        let read = |f: &str| sys::read_num(bat.join(f));
        match key {
            "end" | "start" | "full" => {
                let mut start = read("charge_control_start_threshold");
                let mut end = read("charge_control_end_threshold");
                let v = if key == "full" { 100 } else { value.parse::<f64>()?.round() as i64 };
                if key == "start" {
                    start = Some(v.clamp(0, 99));
                } else {
                    end = Some(v.clamp(50, 100));
                }
                // Starting has to be below stopping.
                if let (Some(s), Some(e)) = (start, end)
                    && s >= e
                {
                    start = Some((e - 5).max(0));
                }
                set_limits(&bat, start, end)?;
                Ok(if key == "full" { nixe_ext::toast("Charging to 100% until you set a limit again") } else { nixe_ext::refresh() })
            }
            "profile" => {
                sys::run("powerprofilesctl", &["set", value])?;
                Ok(None)
            }
            "kbd" => {
                let led = kbd_backlight().context("no keyboard light")?;
                let name = led.file_name().context("no keyboard light")?.to_string_lossy().to_string();
                let v = value.parse::<f64>()?.round() as u32;
                sys::run(
                    "busctl",
                    &["call", "org.freedesktop.login1", "/org/freedesktop/login1/session/auto", "org.freedesktop.login1.Session", "SetBrightness", "ssu", "leds", &name, &v.to_string()],
                )?;
                Ok(None)
            }
            "mic" => {
                sys::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", if value == "true" { "0" } else { "1" }])?;
                Ok(None)
            }
            "camera" => {
                let on = value == "true";
                sys::run_root("modprobe", if on { &["uvcvideo"][..] } else { &["-r", "uvcvideo"][..] })
                    .map_err(|e| if on { e } else { e.context("an app is probably using the camera; close it first") })?;
                Ok(nixe_ext::refresh())
            }
            _ => bail!("unknown setting {key}"),
        }
    }
}

fn has_camera() -> bool {
    std::fs::read_dir("/sys/bus/usb/devices")
        .map(|rd| rd.flatten().any(|e| sys::read(e.path().join("bInterfaceClass")).as_deref() == Some("0e")))
        .unwrap_or(false)
}

fn main() {
    nixe_ext::run(&Laptop);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_rule() {
        let r = rule_text("BAT0", Some(75), Some(80));
        assert!(r.contains("KERNEL==\"BAT0\", ATTR{charge_control_start_threshold}=\"75\", ATTR{charge_control_end_threshold}=\"80\""));
        let r = rule_text("BAT1", None, Some(80));
        assert!(!r.contains("start_threshold") && r.contains("end_threshold}=\"80\""));
    }

    #[test]
    fn profile_names() {
        assert_eq!(profile_label("power-saver"), "Battery saver");
        assert_eq!(profile_label("custom"), "custom");
    }
}
