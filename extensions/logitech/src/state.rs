//! Saved device settings (`~/.config/logi/devices.toml`), keyed by model and
//! serial. Many devices forget their settings when switched off, so these are
//! written back every time a device connects.

use crate::hidpp::device::{Info, feature as f};
use crate::hidpp::features::{buttons, gaming, keyboard, pointer, wheel};
use crate::hidpp::link::Link;
use crate::{files, paths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneSaved {
    pub effect: String,
    pub colour: String,
    pub period: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// For people reading the file.
    pub name: String,
    pub dpi: Option<u16>,
    pub report_rate: Option<u32>,
    /// "ratchet", "free" or "smart:<threshold>".
    pub wheel: Option<String>,
    pub hires: Option<bool>,
    pub invert_wheel: Option<bool>,
    pub invert_thumb_wheel: Option<bool>,
    pub media_keys_first: Option<bool>,
    pub disabled_keys: Option<u8>,
    pub backlight: Option<bool>,
    pub backlight_level: Option<u8>,
    pub backlight_timeout: Option<u16>,
    pub onboard: Option<bool>,
    pub profile: Option<u8>,
    /// Control id (hex) → the control it acts as.
    pub buttons: BTreeMap<String, u16>,
    /// Zone index → effect.
    pub lighting: BTreeMap<String, ZoneSaved>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub devices: BTreeMap<String, Saved>,
}

pub fn load() -> Store {
    std::fs::read_to_string(paths::devices_file()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(store: &Store) -> anyhow::Result<()> {
    let header = "# Saved by Logi. Re-applied whenever a device connects.\n\n";
    files::atomic_write(&paths::devices_file(), &format!("{header}{}", toml::to_string_pretty(store)?))
}

pub fn wheel_to_string(mode: wheel::WheelMode) -> String {
    match mode {
        wheel::WheelMode::Ratchet => "ratchet".into(),
        wheel::WheelMode::FreeSpin => "free".into(),
        wheel::WheelMode::SmartShift(t) => format!("smart:{t}"),
    }
}

pub fn wheel_from_string(s: &str) -> Option<wheel::WheelMode> {
    match s {
        "ratchet" => Some(wheel::WheelMode::Ratchet),
        "free" => Some(wheel::WheelMode::FreeSpin),
        _ => s.strip_prefix("smart:").and_then(|t| t.parse().ok()).map(wheel::WheelMode::SmartShift),
    }
}

pub fn parse_hex_colour(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A saved zone colour: `#rrggbb`, or `theme` for the Omarchy accent.
pub fn resolve_colour(saved: &str, fallback: [u8; 3]) -> [u8; 3] {
    if saved == "theme" {
        return files::theme_accent().and_then(|a| parse_hex_colour(&a)).unwrap_or(fallback);
    }
    parse_hex_colour(saved).unwrap_or(fallback)
}

pub fn hex_colour(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Write saved settings to a device. Returns what failed, for logging.
pub fn apply(link: &mut Link, info: &Info, s: &Saved) -> Vec<String> {
    let mut errors = Vec::new();
    let mut check = |what: &str, r: crate::hidpp::link::Result<()>| {
        if let Err(e) = r {
            errors.push(format!("{what}: {e}"));
        }
    };
    if let Some(v) = s.dpi.filter(|_| info.has(f::ADJUSTABLE_DPI)) {
        check("pointer speed", pointer::write_dpi(link, info, v));
    }
    if let Some(v) = s.report_rate.filter(|_| info.has(f::REPORT_RATE) || info.has(f::EXTENDED_REPORT_RATE)) {
        check("report rate", pointer::write_report_rate(link, info, v));
    }
    if let Some(m) = s.wheel.as_deref().and_then(wheel_from_string).filter(|_| info.has(f::SMART_SHIFT) || info.has(f::SMART_SHIFT_ENHANCED)) {
        check("wheel mode", wheel::write_mode(link, info, m));
    }
    if (s.hires.is_some() || s.invert_wheel.is_some()) && info.has(f::HIRES_WHEEL) {
        let r = wheel::read_hires(link, info).and_then(|now| {
            wheel::write_hires(link, info, s.hires.unwrap_or(now.hires), s.invert_wheel.unwrap_or(now.inverted))
        });
        check("scroll wheel", r);
    }
    if let Some(v) = s.invert_thumb_wheel.filter(|_| info.has(f::THUMB_WHEEL)) {
        check("thumb wheel", wheel::write_thumb_inverted(link, info, v));
    }
    if let Some(v) = s.media_keys_first.filter(|_| keyboard::has_fn_swap(info)) {
        check("Fn keys", keyboard::write_fn_swap(link, info, v));
    }
    if let Some(v) = s.disabled_keys.filter(|_| info.has(f::DISABLE_KEYS)) {
        check("disabled keys", keyboard::write_disabled_keys(link, info, v));
    }
    if (s.backlight.is_some() || s.backlight_level.is_some() || s.backlight_timeout.is_some()) && info.has(f::BACKLIGHT2) {
        let r = keyboard::read_backlight(link, info).and_then(|mut b| {
            b.enabled = s.backlight.unwrap_or(b.enabled);
            b.level = s.backlight_level.unwrap_or(b.level);
            b.timeout = s.backlight_timeout.unwrap_or(b.timeout);
            keyboard::write_backlight(link, info, &b, s.backlight_level.is_some())
        });
        check("backlight", r);
    }
    if info.has(f::ONBOARD_PROFILES) {
        if let Some(v) = s.onboard {
            check("onboard profiles", gaming::write_onboard_mode(link, info, v));
        }
        if let Some(p) = s.profile.filter(|_| s.onboard == Some(true)) {
            check("profile", gaming::write_profile(link, info, p));
        }
    }
    if !s.buttons.is_empty() && info.has(f::REPROG_CONTROLS_V4) {
        for (cid, target) in &s.buttons {
            if let Ok(cid) = u16::from_str_radix(cid.trim_start_matches("0x"), 16) {
                check("buttons", buttons::remap(link, info, cid, *target));
            }
        }
    }
    if !s.lighting.is_empty() && info.has(f::COLOR_LED_EFFECTS) {
        match gaming::read_zones(link, info) {
            Ok(zones) => {
                for z in &zones {
                    if let Some(saved) = s.lighting.get(&z.index.to_string())
                        && let Some(effect) = gaming::Effect::from_key(&saved.effect)
                    {
                        let colour = resolve_colour(&saved.colour, z.colour);
                        let period = if saved.period > 0 { saved.period } else { z.period };
                        check("lighting", gaming::write_zone(link, info, z, effect, colour, period));
                    }
                }
            }
            Err(e) => errors.push(format!("lighting: {e}")),
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut store = Store::default();
        let mut s = Saved { name: "MX Master 3S".into(), dpi: Some(1600), wheel: Some("smart:12".into()), ..Default::default() };
        s.buttons.insert("0x0053".into(), 0x52);
        s.lighting.insert("0".into(), ZoneSaved { effect: "static".into(), colour: "#00ff00".into(), period: 0 });
        store.devices.insert("b034-8a215c03".into(), s);
        let text = toml::to_string_pretty(&store).unwrap();
        assert_eq!(toml::from_str::<Store>(&text).unwrap(), store);
    }

    #[test]
    fn wheel_strings() {
        for m in [wheel::WheelMode::Ratchet, wheel::WheelMode::FreeSpin, wheel::WheelMode::SmartShift(20)] {
            assert_eq!(wheel_from_string(&wheel_to_string(m)), Some(m));
        }
        assert_eq!(parse_hex_colour("#8e5cf7"), Some([0x8e, 0x5c, 0xf7]));
    }
}
