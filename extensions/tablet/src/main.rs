//! Drawing tablets (Wacom, XP-Pen, Huion and others): which screen the pen
//! draws on, the part of the tablet that's used, left-handed and mouse-like
//! use. Hyprland does the work; these settings live in
//! `~/.config/hypr/nixe-tablet.lua`, loaded from `hyprland.lua`, and apply at once.

use anyhow::{Context, Result, bail};
use nixe_ext::page::{self, Page, desc, refreshes};
use nixe_ext::{Extension, sys};
use serde_json::{Value, json};
use std::path::PathBuf;

const PAGE: &str = "tablet";
const REQUIRE: &str = "require(\"hypr.nixe-tablet\") -- Drawing tablets (Settings)";

fn lua_file() -> PathBuf {
    sys::home().join(".config/hypr/nixe-tablet.lua")
}

fn state_file() -> PathBuf {
    sys::state_dir().join("tablet.json")
}

/// What's set here, kept as JSON beside the Lua it writes.
#[derive(Debug, Clone, PartialEq, Default)]
struct Settings {
    output: String,
    left_handed: bool,
    relative: bool,
    /// Percent of the tablet used (100: all), and whether it keeps the screen's shape.
    area: u32,
    match_shape: bool,
}

impl Settings {
    fn load() -> Settings {
        let v: Value = std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        Settings {
            output: v["output"].as_str().unwrap_or("").to_string(),
            left_handed: v["left_handed"].as_bool().unwrap_or(false),
            relative: v["relative"].as_bool().unwrap_or(false),
            area: v["area"].as_u64().unwrap_or(100) as u32,
            match_shape: v["match_shape"].as_bool().unwrap_or(false),
        }
    }

    fn save(&self) -> Result<()> {
        let v = json!({ "output": self.output, "left_handed": self.left_handed, "relative": self.relative, "area": self.area, "match_shape": self.match_shape });
        std::fs::write(state_file(), v.to_string())?;
        Ok(())
    }
}

/// The tablet's size in mm, from libwacom (it lists it in inches).
fn tablet_size() -> Option<(f64, f64)> {
    let text = sys::run("libwacom-list-local-devices", &["--format=datafile"]).ok()?;
    let field = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).and_then(|v| v.trim().parse::<f64>().ok()));
    Some((field("Width")? * 25.4, field("Height")? * 25.4))
}

/// libwacom's description of the tablet: name and button count.
fn tablet_info() -> Option<(String, usize)> {
    let text = sys::run("libwacom-list-local-devices", &["--format=datafile"]).ok()?;
    let name = text.lines().find_map(|l| l.strip_prefix("Name="))?.trim().to_string();
    let mut buttons = 0;
    let mut in_buttons = false;
    for l in text.lines() {
        if l.starts_with('[') {
            in_buttons = l.trim() == "[Buttons]";
            continue;
        }
        if in_buttons && let Some((k, v)) = l.split_once('=') && ["Left", "Right", "Top", "Bottom"].contains(&k.trim()) {
            buttons += v.split(';').filter(|b| !b.trim().is_empty()).count();
        }
    }
    Some((name, buttons))
}

/// The active area (mm, top-left) for `s`, centred on the tablet.
fn area(s: &Settings, size: (f64, f64), screen: Option<(f64, f64)>) -> Option<((f64, f64), (f64, f64))> {
    if s.area >= 100 && !s.match_shape {
        return None;
    }
    let k = s.area.clamp(20, 100) as f64 / 100.0;
    let (mut w, mut h) = (size.0 * k, size.1 * k);
    if s.match_shape && let Some((sw, sh)) = screen.filter(|(w, h)| *w > 0.0 && *h > 0.0) {
        // Narrow the longer side so the area has the screen's shape.
        let want = sw / sh;
        if w / h > want { w = h * want } else { h = w / want }
    }
    Some(((w.round(), h.round()), (((size.0 - w) / 2.0).round(), ((size.1 - h) / 2.0).round())))
}

fn monitors() -> Vec<(String, f64, f64)> {
    let Ok(text) = sys::run("hyprctl", &["monitors", "-j"]) else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some((m["name"].as_str()?.to_string(), m["width"].as_f64()?, m["height"].as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// The Lua for `s`.
fn lua(s: &Settings) -> String {
    let screens = monitors();
    let screen = screens.iter().find(|m| m.0 == s.output).or(screens.first()).map(|m| (m.1, m.2));
    let mut fields = vec![
        format!("output = {:?}", s.output),
        format!("left_handed = {}", s.left_handed),
        format!("relative_input = {}", s.relative),
    ];
    match tablet_size().and_then(|size| area(s, size, screen)) {
        Some(((w, h), (x, y))) => {
            fields.push(format!("active_area_size = {{ {w}, {h} }}"));
            fields.push(format!("active_area_position = {{ {x}, {y} }}"));
        }
        None => {
            fields.push("active_area_size = { 0, 0 }".into());
            fields.push("active_area_position = { 0, 0 }".into());
        }
    }
    format!("hl.config({{ input = {{ tablet = {{ {} }} }} }})", fields.join(", "))
}

/// Write the file, make sure hyprland.lua loads it, and apply it now.
fn apply(s: &Settings) -> Result<()> {
    let code = lua(s);
    std::fs::write(lua_file(), format!("-- Written by Settings (Drawing tablets). Change it there.\n{code}\n"))?;
    let main = sys::home().join(".config/hypr/hyprland.lua");
    if let Ok(text) = std::fs::read_to_string(&main)
        && !text.contains("hypr.nixe-tablet")
    {
        // Before Settings' own file, which is loaded last.
        let new = match text.find("require(\"hypr.settings\")") {
            Some(i) => format!("{}{REQUIRE}\n{}", &text[..i], &text[i..]),
            None => format!("{}\n{REQUIRE}\n", text.trim_end()),
        };
        std::fs::write(&main, new)?;
    }
    let out = sys::run("hyprctl", &["eval", &code])?;
    if out.to_lowercase().contains("error") {
        bail!("Hyprland: {}", out.trim());
    }
    s.save()
}

fn tablets() -> Vec<String> {
    let Ok(text) = sys::run("hyprctl", &["devices", "-j"]) else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    v["tablets"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect()).unwrap_or_default()
}

struct Tablet;

impl Extension for Tablet {
    fn pages(&self) -> Value {
        if tablets().is_empty() {
            return json!([]);
        }
        json!([page::entry(PAGE, "Drawing tablet", "input-tablet-symbolic", "Which screen the pen draws on, the area used, left-handed and mouse-like use.",
            "tablet wacom xp-pen huion pen stylus drawing area screen left-handed")])
    }

    fn describe(&self, _id: &str) -> Result<Value> {
        let s = Settings::load();
        let mut screens: Vec<(String, String)> = vec![(String::new(), "All screens".into())];
        screens.extend(monitors().into_iter().map(|(n, w, h)| (n.clone(), format!("{n} ({w:.0}×{h:.0})"))));
        let mut pen = vec![desc(page::choice_owned("output", "Draw on", &s.output, &screens), "The whole tablet maps to this screen.")];
        if tablet_size().is_some() {
            pen.push(refreshes(desc(page::slider("area", "Area used", s.area as f64, (20.0, 100.0, 5.0), " %"), "A smaller area, centred, means shorter pen moves.")));
            pen.push(desc(page::switch("shape", "Match the screen's shape", s.match_shape), "Keeps circles round: trims the tablet's area to the screen's proportions."));
        }
        pen.push(desc(page::switch("left", "Left-handed", s.left_handed), "Turns the tablet around, with the buttons on the right."));
        pen.push(desc(page::switch("relative", "Move like a mouse", s.relative), "The pen moves the pointer from where it is, instead of jumping to where it touches."));
        let mut about = Vec::new();
        for t in tablets() {
            about.push(page::info(&t, "Connected", &t));
        }
        if let Some((name, buttons)) = tablet_info() {
            about.push(page::info("model", "Model", &name));
            if buttons > 0 {
                about.push(desc(page::info("buttons", "Tablet buttons", &buttons.to_string()), "Drawing apps (Krita, GIMP) choose what they do."));
            }
        }
        if let Some((w, h)) = tablet_size() {
            about.push(page::info("size", "Drawing area", &format!("{w:.0} × {h:.0} mm")));
        }
        let mut extra = Vec::new();
        if sys::present("otd-gui") {
            extra.push(desc(page::button("otd", "Pressure curve and more", "Open OpenTabletDriver"), "OpenTabletDriver is installed: its window sets the pressure curve and button actions."));
        }
        Ok(Page::new("Settings for the pen, applied straight away.").group("Pen", "", pen).group("Tablet", "", about).group("More", "", extra).build())
    }

    fn set(&self, _page: &str, key: &str, value: &str) -> Result<Option<Value>> {
        let mut s = Settings::load();
        match key {
            "output" => {
                if !value.is_empty() && !monitors().iter().any(|m| m.0 == value) {
                    bail!("that screen isn't connected");
                }
                s.output = value.to_string();
            }
            "area" => s.area = value.parse::<f64>().context("not a number")?.round() as u32,
            "shape" => s.match_shape = value == "true",
            "left" => s.left_handed = value == "true",
            "relative" => s.relative = value == "true",
            "otd" => {
                std::process::Command::new("otd-gui").spawn()?;
                return Ok(None);
            }
            _ => bail!("unknown setting {key}"),
        }
        apply(&s)?;
        Ok(None)
    }
}

fn main() {
    nixe_ext::run(&Tablet);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_tablet_needs_no_area() {
        let s = Settings { area: 100, ..Default::default() };
        assert_eq!(area(&s, (152.0, 95.0), Some((2560.0, 1600.0))), None);
    }

    #[test]
    fn smaller_area_is_centred() {
        let s = Settings { area: 50, ..Default::default() };
        assert_eq!(area(&s, (200.0, 100.0), None), Some(((100.0, 50.0), (50.0, 25.0))));
    }

    #[test]
    fn matching_the_screen_keeps_its_shape() {
        let s = Settings { area: 100, match_shape: true, ..Default::default() };
        let ((w, h), (x, _)) = area(&s, (216.0, 135.0), Some((1920.0, 1080.0))).unwrap();
        assert!(((w / h) - 16.0 / 9.0).abs() < 0.02, "{w}×{h}");
        assert_eq!(w, 216.0);
        assert_eq!(x, 0.0);
    }
}
