//! The ASUS page: performance profiles, battery care, fan curves, firmware
//! attributes, Screenpad, and power limits.

use crate::asus::{self, Attribute, FanCurve, Kind};
use crate::{bool_arg, cmd, num_arg, options, paths, refresh, toast};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

fn after_colon(text: &str) -> String {
    text.rsplit(':').next().unwrap_or("").trim().to_string()
}

fn profiles() -> Vec<String> {
    cmd::output(&["asusctl", "profile", "list"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn profile_info() -> String {
    cmd::output(&["asusctl", "profile", "get"]).unwrap_or_default()
}

fn find(info: &str, prefix: &str) -> String {
    info.lines()
        .find(|l| l.trim_start().starts_with(prefix))
        .map(|l| l.trim_start()[prefix.len()..].trim().trim_start_matches(':').trim().to_string())
        .unwrap_or_default()
}

// ----- Which profile the fan curve editor shows -----

fn fan_profile_file() -> std::path::PathBuf {
    paths::state_dir().join("fan-profile")
}

/// The profile being edited: the one picked last, else the active one.
fn fan_profile(profiles: &[String], active: &str) -> String {
    let saved = std::fs::read_to_string(fan_profile_file()).unwrap_or_default().trim().to_string();
    if profiles.contains(&saved) { saved } else { active.to_string() }
}

fn fan_id(fan: &str) -> String {
    fan.to_lowercase()
}

// ----- Describe -----

pub fn describe() -> Value {
    let sup = asus::support();
    let subtitle = if sup.product.is_empty() { String::new() } else { format!("<b>{}</b> · {}", esc(&sup.product), esc(&sup.board)) };
    let profiles = profiles();
    let mut groups = Vec::new();
    if asus::has_platform_profile() {
        groups.push(performance(&profiles));
    }
    if asus::has_charge_limit() {
        groups.push(battery());
    }
    if asus::has_fan_curves() && !profiles.is_empty() {
        groups.push(fan_curves(&profiles));
    }
    let attrs = if asus::has_armoury() { asus::armoury() } else { vec![] };
    let basic: Vec<Value> = attrs.iter().filter(|a| !meta(&a.name).advanced).map(attribute_row).collect();
    if !basic.is_empty() {
        groups.push(json!({ "title": "Firmware", "rows": basic }));
    }
    if asus::has_screenpad() {
        groups.push(screenpad());
    }
    if let Some(g) = advanced(&attrs) {
        groups.push(g);
    }
    json!({ "subtitle": subtitle, "groups": groups })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn performance(profiles: &[String]) -> Value {
    let info = profile_info();
    let opts: Vec<(&str, &str)> = profiles.iter().map(|p| (p.as_str(), p.as_str())).collect();
    json!({
        "title": "Performance",
        "note": "Sets fan behaviour and power limits. Each profile also has its own fan curves, below.",
        "rows": [
            { "kind": "segmented", "key": "profile", "title": "Right now", "options": options(&opts),
              "value": find(&info, "Active profile"), "refresh": true, "keywords": "quiet balanced performance profile mode" },
            { "kind": "choice", "key": "profile-ac", "title": "When plugged in", "options": options(&opts), "value": find(&info, "AC profile") },
            { "kind": "choice", "key": "profile-battery", "title": "On battery", "options": options(&opts), "value": find(&info, "Battery profile") },
        ]
    })
}

fn battery() -> Value {
    let limit = cmd::output(&["asusctl", "battery", "info"])
        .map(|s| after_colon(&s).trim_end_matches('%').parse::<f64>().unwrap_or(100.0))
        .unwrap_or(100.0);
    json!({
        "title": "Battery care",
        "rows": [
            { "kind": "slider", "key": "charge-limit", "title": "Charge limit",
              "desc": "Stop charging at this level. 80% keeps the battery healthy if you're usually plugged in.",
              "value": limit, "min": 20, "max": 100, "step": 5, "unit": "%", "marks": [60, 80, 100], "on_release": true,
              "keywords": "battery charge limit 80 health" },
            { "kind": "button", "key": "charge-once", "title": "Charge to full once",
              "desc": "Ignore the limit until the next full charge.", "label": "Charge to 100%" },
        ]
    })
}

fn fan_curves(profiles: &[String]) -> Value {
    let active = find(&profile_info(), "Active profile");
    let active = if active.is_empty() { profiles[0].clone() } else { active };
    let profile = fan_profile(profiles, &active);
    let opts: Vec<(&str, &str)> = profiles.iter().map(|p| (p.as_str(), p.as_str())).collect();
    let mut rows = vec![json!({
        "kind": "choice", "key": "fan-profile", "title": "Profile to edit",
        "desc": "Each performance profile has its own curves.", "options": options(&opts), "value": profile, "refresh": true,
    })];
    let curves = asus::fan_curves(&profile);
    if curves.is_empty() {
        rows.push(json!({ "kind": "info", "title": "No fan curves for this profile" }));
        return json!({ "title": "Fan curves", "rows": rows });
    }
    let series: Vec<Value> = curves
        .iter()
        .map(|c| {
            let points: Vec<Value> =
                c.temp.iter().zip(&c.pwm).map(|(t, p)| json!([t, asus::pwm_to_percent(*p)])).collect();
            json!({ "id": fan_id(&c.fan), "label": format!("{} fan", c.fan), "points": points })
        })
        .collect();
    let mut inner = vec![json!({
        "kind": "curve", "key": "fan", "series": series, "presets": true,
        "keywords": "fan curve speed temperature cooling cpu gpu graph quiet",
        "hint": "Drag a point up or down for fan speed, left or right for temperature. Speeds can't drop as it gets hotter.",
    })];
    if profile == active {
        inner.push(json!({
            "kind": "button", "key": "fan-reset", "title": "Reset to the firmware's curves",
            "desc": "For the active profile.", "label": "Reset",
        }));
    }
    rows.push(json!({
        "kind": "switch", "key": "fan-custom", "title": "Use custom fan curves",
        "desc": "Off uses the firmware's own curves for this profile.",
        "value": curves.iter().any(|c| c.enabled), "rows": inner,
    }));
    json!({ "title": "Fan curves", "rows": rows })
}

// ----- Firmware & power limits -----

struct Meta {
    title: &'static str,
    desc: &'static str,
    unit: &'static str,
    restart: bool,
    advanced: bool,
}

fn meta(name: &str) -> Meta {
    let m = |title, desc, unit, restart, advanced| Meta { title, desc, unit, restart, advanced };
    match name {
        "boot_sound" => m("Boot sound", "Play the ASUS startup sound when the laptop powers on.", "", false, false),
        "dgpu_disable" => m(
            "Turn off the dedicated GPU",
            "Use only the integrated GPU: cooler and longer battery life, but no NVIDIA graphics.",
            "",
            true,
            false,
        ),
        "gpu_mux_mode" => m(
            "GPU mode",
            "Hybrid lets the integrated GPU drive the screen and saves battery. Dedicated sends everything through the NVIDIA \
             GPU for lower latency.",
            "",
            true,
            false,
        ),
        "panel_overdrive" => m("Panel overdrive", "Faster pixel response on the built-in display.", "", false, false),
        "charge_mode" => m(
            "Charge mode",
            "Charging behaviour chosen by the firmware. Leave it unless you know what a value does.",
            "",
            false,
            true,
        ),
        "nv_base_tgp" => m("GPU base power", "The NVIDIA GPU's guaranteed power.", " W", false, true),
        "nv_dynamic_boost" => {
            m("GPU dynamic boost", "Extra power the GPU may borrow from the CPU when it needs it.", " W", false, true)
        }
        "nv_temp_target" => m("GPU temperature target", "The GPU slows down to stay under this temperature.", " °C", false, true),
        "nv_tgp" => m("GPU power limit", "Total power the NVIDIA GPU may use.", " W", false, true),
        "ppt_pl1_spl" => m("CPU sustained power limit", "Power the CPU may use for long workloads (PL1).", " W", false, true),
        "ppt_pl2_sppt" => m("CPU boost power limit", "Power the CPU may use in short bursts (PL2).", " W", false, true),
        "ppt_pl3_fppt" => {
            m("CPU fast boost power limit", "Power the CPU may use for the shortest bursts (PL3).", " W", false, true)
        }
        "ppt_apu_sppt" => {
            m("APU sustained power limit", "Power the whole processor may use for long workloads.", " W", false, true)
        }
        "ppt_platform_sppt" => {
            m("Platform sustained power limit", "Power the whole system may use for long workloads.", " W", false, true)
        }
        _ => m("", "", "", false, true),
    }
}

fn title_for(name: &str, m: &Meta) -> String {
    if !m.title.is_empty() {
        return m.title.to_string();
    }
    let mut s = name.replace('_', " ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    s
}

fn is_switch(a: &Attribute) -> bool {
    matches!(&a.kind, Kind::Choice { choices, .. }
        if choices.len() == 2 && choices.contains(&0) && choices.contains(&1) && a.name != "gpu_mux_mode")
}

fn attribute_row(a: &Attribute) -> Value {
    let m = meta(&a.name);
    let title = title_for(&a.name, &m);
    let key = format!("attr:{}", a.name);
    let tag = if m.restart { "Restart needed" } else { "" };
    let is_temp = m.unit == " °C";
    match &a.kind {
        Kind::Fixed(v) => {
            let text = if is_temp {
                let (n, sym) =
                    if paths::fahrenheit() { ((*v as f64 * 9.0 / 5.0 + 32.0).round() as i64, "°F") } else { (*v, "°C") };
                format!("{n}\u{a0}{sym}")
            } else {
                format!("{v}{}", m.unit.replace(' ', "\u{a0}"))
            };
            json!({ "kind": "info", "title": title, "value": text, "tooltip": m.desc, "tag": tag })
        }
        Kind::Choice { current, .. } if is_switch(a) => {
            json!({ "kind": "switch", "key": key, "title": title, "desc": m.desc, "value": *current == 1, "tag": tag })
        }
        Kind::Choice { choices, current } => {
            let opts: Vec<(String, String)> = choices
                .iter()
                .map(|c| {
                    let label = match (a.name.as_str(), c) {
                        ("gpu_mux_mode", 0) => "Dedicated GPU (MUX)".to_string(),
                        ("gpu_mux_mode", 1) => "Hybrid".to_string(),
                        (_, n) => format!("Mode {n}"),
                    };
                    (c.to_string(), label)
                })
                .collect();
            json!({ "kind": "choice", "key": key, "title": title, "desc": m.desc, "options": options(&opts),
                    "value": current.to_string(), "tag": tag })
        }
        Kind::Range { min, max, current, default } => {
            let step = if is_temp || *max - *min <= 60 { 1 } else { 5 };
            let mut row = json!({
                "kind": "slider", "key": key, "title": title, "desc": m.desc, "value": current,
                "min": min, "max": max, "step": step, "tag": tag,
            });
            if is_temp {
                row["temperature"] = json!(true);
            } else {
                row["unit"] = json!(m.unit);
            }
            if let Some(d) = default {
                row["reset"] = json!(d);
            }
            row
        }
    }
}

/// Power limits and profile tuning, folded away: most people never need them.
fn advanced(attrs: &[Attribute]) -> Option<Value> {
    let mut rows: Vec<Value> = Vec::new();
    let tuning = asus::has_platform_profile()
        .then(|| cmd::output(&["asusctl", "profile", "tuning"]))
        .flatten()
        .filter(|t| t.contains("Profile tuning:"));
    if let Some(t) = tuning {
        rows.push(json!({ "kind": "switch", "key": "tuning", "title": "Profile tuning",
                          "desc": "Apply per-profile power tuning.", "value": after_colon(&t) == "true" }));
    }
    rows.extend(attrs.iter().filter(|a| meta(&a.name).advanced).map(attribute_row));
    if rows.is_empty() {
        return None;
    }
    Some(json!({
        "title": "Advanced",
        "rows": [{
            "kind": "disclosure",
            "title": "Power limits and tuning",
            "desc": "Higher limits make the laptop hotter and louder and drain the battery faster. Each value can be reset to the firmware default.",
            "keywords": "power limit tgp ppt pl1 pl2 boost watts gpu cpu tuning advanced",
            "rows": rows,
        }]
    }))
}

fn screenpad() -> Value {
    let get = |prop: &str| asus::busctl_get("/xyz/ljones", "xyz.ljones.Backlight", prop);
    let brightness = get("ScreenpadBrightness").and_then(|v| v.parse::<f64>().ok()).unwrap_or(50.0).clamp(0.0, 100.0);
    let sync = get("ScreenpadSyncWithPrimary").is_some_and(|v| v == "true");
    let gamma = get("ScreenpadGamma").and_then(|v| v.trim_matches('"').parse::<f64>().ok()).unwrap_or(1.0).clamp(0.5, 2.2);
    json!({
        "title": "Screenpad",
        "rows": [
            { "kind": "slider", "key": "screenpad-brightness", "title": "Brightness", "value": brightness, "min": 0, "max": 100, "unit": "%" },
            { "kind": "switch", "key": "screenpad-sync", "title": "Match the main display's brightness", "value": sync },
            { "kind": "slider", "key": "screenpad-gamma", "title": "Gamma", "desc": "1.0 is linear.", "value": gamma,
              "min": 0.5, "max": 2.2, "step": 0.1, "digits": 1 },
        ]
    })
}

// ----- Set -----

/// Speeds from the page (percent) back to PWM, keeping the exact PWM of points
/// whose percentage didn't change, so untouched points aren't nudged by rounding.
pub fn curve_from_page(old: &FanCurve, value: &str) -> Result<(Vec<u32>, Vec<u32>)> {
    let mut temp = Vec::new();
    let mut pwm = Vec::new();
    for (i, part) in value.split(',').filter(|p| !p.is_empty()).enumerate() {
        let (t, p) = part.split_once(':').context("points are temp:percent")?;
        let (t, p): (u32, u32) = (t.trim().parse()?, p.trim().parse()?);
        temp.push(t);
        pwm.push(match old.pwm.get(i) {
            Some(o) if asus::pwm_to_percent(*o) == p => *o,
            _ => asus::percent_to_pwm(p),
        });
    }
    if temp.len() != old.temp.len() {
        bail!("the {} fan curve has {} points, not {}", old.fan, old.temp.len(), temp.len());
    }
    Ok((temp, pwm))
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub fn set(key: &str, value: &str) -> Result<Option<Value>> {
    let v = value.to_string();
    match key {
        "profile" => asus::apply(args(&["profile", "set", value])).map(|_| refresh()),
        "profile-ac" => asus::apply(args(&["profile", "set", "-a", value])).map(|_| None),
        "profile-battery" => asus::apply(args(&["profile", "set", "-b", value])).map(|_| None),
        "charge-limit" => asus::apply(vec!["battery".into(), "limit".into(), num_arg(value)?.to_string()]).map(|_| None),
        "charge-once" => asus::apply(args(&["battery", "oneshot"])).map(|_| toast("Charging to 100% once")),
        "fan-profile" => {
            cmd::atomic_write(&fan_profile_file(), &v)?;
            Ok(refresh())
        }
        "fan-custom" => {
            let profile = current_fan_profile()?;
            let on = bool_arg(value)?.to_string();
            asus::apply(vec!["fan-curve".into(), "--mod-profile".into(), profile, "--enable-fan-curves".into(), on]).map(|_| None)
        }
        "fan-reset" => asus::apply(args(&["fan-curve", "--default"])).map(|_| refresh()),
        "tuning" => asus::apply(vec!["profile".into(), "tuning".into(), bool_arg(value)?.to_string()]).map(|_| None),
        "screenpad-brightness" => {
            asus::apply(vec!["backlight".into(), "--screenpad-brightness".into(), num_arg(value)?.to_string()]).map(|_| None)
        }
        "screenpad-sync" => {
            asus::apply(vec!["backlight".into(), "--sync-screenpad-brightness".into(), bool_arg(value)?.to_string()]).map(|_| None)
        }
        "screenpad-gamma" => {
            let g: f64 = value.parse().context("gamma is a number")?;
            asus::apply(vec!["backlight".into(), "--screenpad-gamma".into(), format!("{g:.1}")]).map(|_| None)
        }
        k if k.starts_with("fan/") => {
            let fan = &k[4..];
            let profile = current_fan_profile()?;
            let curves = asus::fan_curves(&profile);
            let old = curves.iter().find(|c| fan_id(&c.fan) == fan).with_context(|| format!("no {fan} fan"))?;
            let (temp, pwm) = curve_from_page(old, value)?;
            let data = asus::fan_data(&temp, &pwm).with_context(|| format!("{} curve not applied", old.fan))?;
            asus::apply(vec![
                "fan-curve".into(),
                "--mod-profile".into(),
                profile,
                "--fan".into(),
                fan.into(),
                "--data".into(),
                data,
            ])
            .map(|_| None)
        }
        k if k.starts_with("attr:") => {
            let name = &k[5..];
            let n = match value {
                "true" => 1,
                "false" => 0,
                v => num_arg(v)?,
            };
            asus::armoury_set(name, n)?;
            Ok(if meta(name).restart { toast("Restart to apply this change") } else { None })
        }
        _ => bail!("unknown setting {key}"),
    }
}

fn current_fan_profile() -> Result<String> {
    let profiles = profiles();
    if profiles.is_empty() {
        bail!("asusctl lists no profiles");
    }
    let active = find(&profile_info(), "Active profile");
    Ok(fan_profile(&profiles, if active.is_empty() { &profiles[0] } else { &active }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_keeps_untouched_pwm() {
        let old = FanCurve { fan: "CPU".into(), pwm: vec![25, 43, 61], temp: vec![0, 57, 61], enabled: true };
        let page: Vec<String> =
            old.temp.iter().zip(&old.pwm).map(|(t, p)| format!("{t}:{}", asus::pwm_to_percent(*p))).collect();
        let (t, p) = curve_from_page(&old, &page.join(",")).unwrap();
        assert_eq!((t, p), (vec![0, 57, 61], vec![25, 43, 61]));
        // A moved point is converted; the others stay exact.
        let (_, p) = curve_from_page(&old, "0:12,57:17,70:50").unwrap();
        assert_eq!(p, vec![asus::percent_to_pwm(12), 43, asus::percent_to_pwm(50)]);
        assert!(curve_from_page(&old, "0:10,57:17").is_err());
        assert!(curve_from_page(&old, "0-10").is_err());
    }

    #[test]
    fn firmware_rows() {
        let sw = attribute_row(&Attribute { name: "panel_overdrive".into(), kind: Kind::Choice { choices: vec![0, 1], current: 1 } });
        assert_eq!(sw["kind"], "switch");
        assert_eq!(sw["value"], true);
        let mux = attribute_row(&Attribute { name: "gpu_mux_mode".into(), kind: Kind::Choice { choices: vec![0, 1], current: 1 } });
        assert_eq!(mux["kind"], "choice");
        assert_eq!(mux["tag"], "Restart needed");
        assert_eq!(mux["options"][1], json!(["1", "Hybrid"]));
        let temp = attribute_row(&Attribute {
            name: "nv_temp_target".into(),
            kind: Kind::Range { min: 75, max: 87, current: 87, default: Some(87) },
        });
        assert_eq!(temp["temperature"], true);
        assert_eq!(temp["reset"], 87);
        let tgp = attribute_row(&Attribute { name: "nv_tgp".into(), kind: Kind::Range { min: 30, max: 140, current: 110, default: None } });
        assert_eq!((tgp["step"].as_i64(), tgp["unit"].as_str()), (Some(5), Some(" W")));
        assert_eq!(attribute_row(&Attribute { name: "x_y".into(), kind: Kind::Fixed(3) })["title"], "X y");
    }
}
