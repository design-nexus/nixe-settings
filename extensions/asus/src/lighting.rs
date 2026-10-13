//! The Aura Lighting page: keyboard effects and colours, when the lights are on,
//! the Slash lightbar, AniMe Matrix, XG Mobile and drive lights.

use crate::asus::{self, AuraChoice};
use crate::{bool_arg, cmd, num_arg, options, refresh};
use anyhow::{Result, bail};
use serde_json::{Value, json};

pub fn describe() -> Value {
    let mut groups = Vec::new();
    if asus::has_aura() {
        groups.push(keyboard());
        if let Some(g) = lighting_states() {
            groups.push(g);
        }
    } else if let Some(row) = brightness_row() {
        groups.push(json!({ "title": "Keyboard backlight", "rows": [row] }));
    }
    if asus::has_slash() {
        groups.push(slash());
    }
    if asus::has_anime() {
        groups.push(anime());
    }
    if asus::has_xgm()
        && let Some(on) = asus::xgm_state()
    {
        groups.push(json!({ "title": "XG Mobile", "rows": [
            { "kind": "switch", "key": "xgm", "title": "Light", "desc": "The light on a connected XG Mobile dock.", "value": on }
        ]}));
    }
    if asus::has_scsi() {
        groups.push(drive_leds());
    }
    json!({ "groups": groups })
}

// ----- Keyboard -----

fn brightness_row() -> Option<Value> {
    let levels: Vec<String> = asus::support().brightness.iter().map(|l| l.to_lowercase()).collect();
    if levels.is_empty() {
        return None;
    }
    let current = cmd::output(&["asusctl", "leds", "get"])
        .map(|t| t.rsplit(':').next().unwrap_or("").trim().to_lowercase())
        .unwrap_or_default();
    let opts: Vec<(String, String)> = levels
        .iter()
        .map(|l| {
            let label = match l.as_str() {
                "off" => "Off",
                "low" => "Low",
                "med" => "Medium",
                "high" => "High",
                other => other,
            };
            (l.clone(), label.to_string())
        })
        .collect();
    Some(json!({
        "kind": "segmented", "key": "brightness", "title": "Brightness", "desc": "Also on the keyboard's brightness keys.",
        "options": options(&opts), "value": current, "keywords": "keyboard backlight brightness",
    }))
}

/// What the keyboard should show: the saved choice, else what it's doing now.
fn current_choice() -> AuraChoice {
    let mut c = asus::load_aura().unwrap_or_else(|| {
        let mut c = AuraChoice::default();
        if let Some(l) = asus::aura_state() {
            c.mode = l.mode;
            c.colour1 = l.colour1;
            c.colour2 = l.colour2;
            if ["low", "med", "high"].contains(&l.speed.as_str()) {
                c.speed = l.speed;
            }
            if ["up", "down", "left", "right"].contains(&l.direction.as_str()) {
                c.direction = l.direction;
            }
        }
        c
    });
    let modes = asus::usable_modes(asus::support());
    if !modes.contains(&c.mode)
        && let Some(first) = modes.first()
    {
        c.mode = first.clone();
    }
    c
}

/// `rainbow-wave` → `Rainbow Wave`.
fn effect_label(kebab: &str) -> String {
    kebab.split('-').map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" ")
}

fn keyboard() -> Value {
    let modes = asus::usable_modes(asus::support());
    let mut rows: Vec<Value> = brightness_row().into_iter().collect();
    if modes.is_empty() {
        return json!({ "title": "Keyboard", "rows": rows });
    }
    let c = current_choice();
    let shown = asus::resolved(&c);
    let p = asus::effect_params(&c.mode);
    let effects: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), effect_label(m))).collect();
    rows.push(json!({
        "kind": "segmented", "key": "effect", "title": "Effect", "options": options(&effects), "value": c.mode,
        "refresh": true, "keywords": "aura rgb lighting animation rainbow breathe static pulse effect",
    }));
    if p.colours >= 1 {
        rows.push(json!({
            "kind": "switch", "key": "follow-theme", "title": "Follow Omarchy theme",
            "desc": "Use the theme's keyboard colour, and update it whenever you change themes. Your effect is kept.",
            "value": c.follow_theme, "refresh": true,
        }));
        let mut colour = json!({
            "kind": "colour", "key": "colour1", "title": "Colour", "value": shown.colour1, "refresh": true,
            "keywords": "colour color rgb hex",
        });
        if let Some(t) = asus::theme_keyboard_colour() {
            colour["theme"] = json!({ "label": "Omarchy theme", "colour": t });
        }
        rows.push(colour);
    }
    if p.colours >= 2 {
        rows.push(json!({
            "kind": "colour", "key": "colour2", "title": "Second colour",
            "desc": "Breathing fades between the two colours.", "value": c.colour2,
        }));
    }
    // Breathe needs a speed on the command line, but the keyboard ignores it.
    if p.speed && c.mode != "breathe" {
        rows.push(json!({
            "kind": "segmented", "key": "speed", "title": "Speed",
            "options": options(&[("low", "Slow"), ("med", "Medium"), ("high", "Fast")]), "value": c.speed,
        }));
    }
    if p.direction {
        rows.push(json!({
            "kind": "segmented", "key": "direction", "title": "Direction",
            "options": options(&[("left", "←"), ("right", "→"), ("up", "↑"), ("down", "↓")]), "value": c.direction,
        }));
    }
    json!({
        "title": "Keyboard",
        "note": "The backlight timeout is under Power &amp; Battery.",
        "rows": rows,
    })
}

fn lighting_states() -> Option<Value> {
    let entries = asus::aura_power();
    let names = &asus::support().power_zones;
    if entries.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    for (i, (_zone, flags)) in entries.iter().enumerate() {
        let name = if names.len() == entries.len() {
            names[i].clone()
        } else if entries.len() == 1 {
            "Keyboard".to_string()
        } else {
            continue;
        };
        rows.push(json!({
            "kind": "chips", "key": format!("power:{}", asus::kebab(&name)), "title": asus::pretty(&name),
            "labels": ["Boot", "Awake", "Sleep", "Shutdown"], "value": flags,
            "keywords": "boot awake sleep shutdown power state startup",
        }));
    }
    Some(json!({
        "title": "When the lights are on",
        "note": "Pick the moments each zone lights up. For example, leave <b>Sleep</b> off so the keyboard is dark while the laptop sleeps.",
        "rows": rows,
    }))
}

// ----- Slash -----

fn slash() -> Value {
    let s = asus::slash();
    let mut inner = Vec::new();
    let modes = asus::slash_modes();
    if !modes.is_empty() {
        let opts: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), asus::pretty(m))).collect();
        inner.push(json!({ "kind": "choice", "key": "slash-mode", "title": "Animation", "options": options(&opts), "value": s.mode }));
    }
    inner.push(json!({
        "kind": "slider", "key": "slash-brightness", "title": "Brightness", "value": (s.brightness as f64 / 2.55).round(),
        "min": 0, "max": 100, "unit": "%",
    }));
    inner.push(json!({
        "kind": "slider", "key": "slash-interval", "title": "Pause between animations",
        "desc": "How long the lightbar waits before playing the animation again. 0 repeats straight away.",
        "value": s.interval, "min": 0, "max": 5, "keywords": "interval delay repeat pause gap",
    }));
    inner.push(json!({
        "kind": "chips", "key": "slash-when", "title": "Show the animation", "desc": "Besides while you're using the laptop.",
        "labels": ["Boot", "Shutdown", "Sleep", "On battery"],
        "value": [s.on_boot, s.on_shutdown, s.on_sleep, s.on_battery],
        "keywords": "boot shutdown sleep battery show animation",
    }));
    inner.push(json!({
        "kind": "switch", "key": "slash-warning", "title": "Low battery warning",
        "desc": "Flash a warning on the lightbar when the battery is low.", "value": s.battery_warning,
    }));
    json!({ "title": "Slash lightbar", "rows": [{
        "kind": "switch", "key": "slash", "title": "Slash lightbar",
        "desc": "The light strip on the back of the lid. Its LEDs are white only, so there's no colour to choose.",
        "value": s.enabled, "keywords": "slash led lid strip lightbar", "rows": inner,
    }]})
}

// ----- AniMe Matrix and drive lights: write-only, so buttons rather than switches -----

fn on_off(key: &str, title: &str) -> Value {
    json!({ "kind": "buttons", "key": key, "title": title, "options": options(&[("true", "On"), ("false", "Off")]) })
}

fn anime() -> Value {
    json!({
        "title": "AniMe Matrix",
        "note": "These can't be read back from the laptop, so they show buttons instead of switches.",
        "rows": [
            on_off("anime-display", "Display"),
            { "kind": "choice", "key": "anime-brightness", "title": "Brightness",
              "options": options(&[("off", "Off"), ("low", "Low"), ("med", "Medium"), ("high", "High")]) },
            on_off("anime-builtins", "Built-in animations"),
            on_off("anime-unplugged", "Off when unplugged"),
            on_off("anime-suspended", "Off when asleep"),
            on_off("anime-lid", "Off when the lid is closed"),
            { "kind": "button", "key": "anime-clear", "title": "Clear the display", "label": "Clear" },
        ]
    })
}

fn drive_leds() -> Value {
    let modes: Vec<(String, String)> = cmd::output(&["asusctl", "scsi", "--list"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(|l| (l.to_string(), l.to_string()))
        .collect();
    let mut rows = vec![on_off("scsi-enable", "Lights")];
    if !modes.is_empty() {
        rows.push(json!({ "kind": "choice", "key": "scsi-mode", "title": "Effect", "options": options(&modes) }));
    }
    rows.push(json!({ "kind": "choice", "key": "scsi-speed", "title": "Speed", "options": options(&[
        ("slowest", "Slowest"), ("slow", "Slow"), ("med", "Medium"), ("fast", "Fast"), ("fastest", "Fastest")]) }));
    rows.push(json!({ "kind": "choice", "key": "scsi-direction", "title": "Direction",
                      "options": options(&[("forward", "Forward"), ("reverse", "Reverse")]) }));
    rows.push(json!({ "kind": "colour", "key": "scsi-colour", "title": "Colour", "value": "#7aa2f7", "compact": true }));
    json!({
        "title": "Drive lights",
        "note": "Lights on external ASUS drives. These can't be read back from the laptop.",
        "rows": rows,
    })
}

// ----- Set -----

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// Save a keyboard change and show it.
fn keyboard_change(change: impl FnOnce(&mut AuraChoice)) -> Result<()> {
    let mut c = current_choice();
    change(&mut c);
    asus::save_aura(&c)?;
    asus::apply_aura(&c)
}

fn colour_arg(v: &str) -> Result<String> {
    let t = v.trim().to_lowercase();
    if t.len() == 7 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(t)
    } else {
        bail!("expected a colour like #7aa2f7, got \"{v}\"")
    }
}

pub fn set(key: &str, value: &str) -> Result<Option<Value>> {
    let v = value.to_string();
    match key {
        "brightness" => asus::apply(args(&["leds", "set", value])).map(|_| None),
        "effect" => keyboard_change(|c| c.mode = v).map(|_| refresh()),
        "speed" => keyboard_change(|c| c.speed = v).map(|_| None),
        "direction" => keyboard_change(|c| c.direction = v).map(|_| None),
        "colour2" => {
            let hex = colour_arg(value)?;
            keyboard_change(|c| c.colour2 = hex).map(|_| None)
        }
        "colour1" => {
            let hex = colour_arg(value)?;
            // Picking the theme swatch follows the theme; any other colour stops following.
            let follow = asus::theme_keyboard_colour().as_deref() == Some(hex.as_str());
            keyboard_change(|c| {
                c.colour1 = hex;
                c.follow_theme = follow;
            })
            .map(|_| refresh())
        }
        "follow-theme" => {
            let on = bool_arg(value)?;
            let theme = asus::theme_keyboard_colour();
            keyboard_change(|c| {
                c.follow_theme = on;
                if on && let Some(t) = theme {
                    c.colour1 = t;
                }
            })
            .map(|_| refresh())
        }
        k if k.starts_with("power:") => {
            let flags: Vec<bool> = value.split(',').map(|b| b.trim() == "true").collect();
            if flags.len() != 4 {
                bail!("expected four states");
            }
            asus::apply(asus::power_args(&k[6..], [flags[0], flags[1], flags[2], flags[3]])).map(|_| None)
        }
        "slash" => {
            let flag = if bool_arg(value)? { "--enable" } else { "--disable" };
            asus::apply(args(&["slash", "set", flag])).map(|_| None)
        }
        "slash-mode" => asus::apply(args(&["slash", "set", "--mode", value])).map(|_| None),
        "slash-brightness" => {
            let raw = ((num_arg(value)? as f64) * 2.55).round() as i64;
            asus::apply(vec!["slash".into(), "set".into(), "-l".into(), raw.to_string()]).map(|_| None)
        }
        "slash-interval" => asus::apply(vec!["slash".into(), "set".into(), "--interval".into(), num_arg(value)?.to_string()]).map(|_| None),
        "slash-when" => {
            let s = asus::slash();
            let was = [s.on_boot, s.on_shutdown, s.on_sleep, s.on_battery];
            let now: Vec<bool> = value.split(',').map(|b| b.trim() == "true").collect();
            // Send only the ones that changed.
            for (i, flag) in ["-B", "-S", "-s", "-b"].iter().enumerate() {
                if let Some(n) = now.get(i)
                    && *n != was[i]
                {
                    asus::apply(vec!["slash".into(), "set".into(), flag.to_string(), n.to_string()])?;
                }
            }
            Ok(None)
        }
        "slash-warning" => asus::apply(vec!["slash".into(), "set".into(), "-w".into(), bool_arg(value)?.to_string()]).map(|_| None),
        "xgm" => {
            let on = if bool_arg(value)? { "1" } else { "0" };
            asus::apply(args(&["xgmled", "set", on])).map(|_| None)
        }
        "anime-display" | "anime-builtins" | "anime-unplugged" | "anime-suspended" | "anime-lid" => {
            let flag = match key {
                "anime-display" => "--enable-display",
                "anime-builtins" => "--enable-powersave-anim",
                "anime-unplugged" => "--off-when-unplugged",
                "anime-suspended" => "--off-when-suspended",
                _ => "--off-when-lid-closed",
            };
            asus::apply(vec!["anime".into(), flag.into(), bool_arg(value)?.to_string()]).map(|_| None)
        }
        "anime-brightness" => asus::apply(args(&["anime", "--brightness", value])).map(|_| None),
        "anime-clear" => asus::apply(args(&["anime", "--clear"])).map(|_| None),
        "scsi-enable" => asus::apply(vec!["scsi".into(), "--enable".into(), bool_arg(value)?.to_string()]).map(|_| None),
        "scsi-mode" => asus::apply(args(&["scsi", "--mode", value])).map(|_| None),
        "scsi-speed" => asus::apply(args(&["scsi", "--speed", value])).map(|_| None),
        "scsi-direction" => asus::apply(args(&["scsi", "--direction", value])).map(|_| None),
        "scsi-colour" => asus::apply(vec!["scsi".into(), "--colours".into(), asus::hex6(&colour_arg(value)?)]).map(|_| None),
        _ => bail!("unknown setting {key}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_colours() {
        assert_eq!(effect_label("rainbow-wave"), "Rainbow Wave");
        assert_eq!(effect_label("static"), "Static");
        assert_eq!(colour_arg("#7AA2F7").unwrap(), "#7aa2f7");
        assert!(colour_arg("7aa2f7").is_err());
        assert!(colour_arg("#7aa2fz").is_err());
    }
}
