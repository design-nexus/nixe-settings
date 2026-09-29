//! ASUS laptop controls through `asusctl` / asusd.
//!
//! Every feature is gated on what the machine reports it supports, so pages
//! only show controls that will actually work.

use crate::cmd;
use anyhow::{Result, bail};
use std::sync::OnceLock;

const BUS: &str = "xyz.ljones.Asusd";

// ----- What's supported -----

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Support {
    pub product: String,
    pub board: String,
    pub core: Vec<String>,
    pub platform: Vec<String>,
    pub brightness: Vec<String>,
    pub modes: Vec<String>,
    pub zones: Vec<String>,
    pub power_zones: Vec<String>,
}

impl Support {
    pub fn has_core(&self, name: &str) -> bool {
        self.core.iter().any(|c| c.to_lowercase().ends_with(&name.to_lowercase()))
    }

    pub fn has_platform(&self, name: &str) -> bool {
        self.platform.iter().any(|p| p == name)
    }
}

/// Parse `asusctl info --show-supported`.
pub fn parse_supported(text: &str) -> Support {
    let mut s = Support::default();
    let mut current: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Product family:") {
            s.product = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Board name:") {
            s.board = v.trim().to_string();
        } else if let Some(name) = t.strip_prefix("Supported ").and_then(|h| h.strip_suffix(':')) {
            current = Some(name);
        } else if t == "]" {
            current = None;
        } else if let Some(section) = current {
            let item = t.trim_end_matches(',').trim_matches('"');
            if item.is_empty() || item == "[" || item == "[]" {
                continue;
            }
            let list = match section {
                "Core Functions" => &mut s.core,
                "Platform Properties" => &mut s.platform,
                "Keyboard Brightness" => &mut s.brightness,
                "Aura Modes" => &mut s.modes,
                "Aura Zones" => &mut s.zones,
                "Aura Power Zones" => &mut s.power_zones,
                _ => continue,
            };
            list.push(item.to_string());
        }
    }
    s
}

static SUPPORT: OnceLock<Support> = OnceLock::new();

pub fn installed() -> bool {
    cmd::present("asusctl")
}

/// What this machine supports (asked once per run).
pub fn support() -> &'static Support {
    SUPPORT.get_or_init(|| {
        if !installed() {
            return Support::default();
        }
        cmd::output(&["asusctl", "info", "--show-supported"]).map(|t| parse_supported(&t)).unwrap_or_default()
    })
}

/// Whether the vendor string says this is an ASUS machine.
pub fn is_asus_hardware() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/sys_vendor").is_ok_and(|v| v.to_uppercase().contains("ASUS"))
}

/// The ASUS page: only on ASUS hardware with asusctl and something to control.
pub fn available() -> bool {
    installed() && is_asus_hardware() && !support().core.is_empty()
}

pub fn lighting_available() -> bool {
    available() && (has_aura() || has_slash() || has_anime() || has_xgm() || has_scsi() || kbd_backlight_only())
}

fn kbd_backlight_only() -> bool {
    !support().brightness.is_empty()
}

/// Effects worth offering, as kebab-case names. Rainbow wave travels across
/// lighting zones; keyboards that report none (lit as one piece) list it but
/// show a single colour or nothing, ignoring speed, direction and brightness.
pub fn usable_modes(s: &Support) -> Vec<String> {
    s.modes.iter().map(|m| kebab(m)).filter(|m| !(m == "rainbow-wave" && s.zones.is_empty())).collect()
}

pub fn has_aura() -> bool {
    !support().modes.is_empty() || support().has_core("Aura")
}

pub fn has_slash() -> bool {
    support().has_core("Slash") && cmd::output(&["asusctl", "slash", "get"]).is_some_and(|t| t.contains("Slash LED"))
}

pub fn has_anime() -> bool {
    support().has_core("Anime")
}

pub fn has_scsi() -> bool {
    support().has_core("Scsi")
}

pub fn has_xgm() -> bool {
    support().has_core("XgmLed") || support().has_core("Xgm")
}

pub fn has_fan_curves() -> bool {
    support().has_core("FanCurves")
}

pub fn has_armoury() -> bool {
    support().has_core("AsusArmoury")
}

pub fn has_platform_profile() -> bool {
    support().has_platform("ThrottlePolicy")
}

pub fn has_charge_limit() -> bool {
    support().has_platform("ChargeControlEndThreshold")
}

pub fn has_screenpad() -> bool {
    busctl_get("/xyz/ljones", "xyz.ljones.Backlight", "ScreenpadBrightness").is_some()
}

// ----- Running asusctl -----

pub fn run(args: &[&str]) -> Result<String> {
    let mut full = vec!["asusctl"];
    full.extend_from_slice(args);
    cmd::run(&full)
}

pub fn run_owned(args: Vec<String>) -> Result<String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs)
}

/// Run in the background and toast a failure.
pub fn apply(args: Vec<String>) {
    let label = args.join(" ");
    cmd::background(
        move || run_owned(args),
        move |r| {
            if let Err(e) = r {
                crate::window::toast(&format!("asusctl {label}: {e}"));
            }
        },
    );
}

// ----- D-Bus reads -----

/// The object path of the keyboard's Aura device, e.g. `/xyz/ljones/aura/<device>`.
pub fn aura_path() -> Option<String> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let tree = cmd::output(&["busctl", "--system", "tree", BUS, "--list"])?;
        tree.lines().map(str::trim).find(|l| l.starts_with("/xyz/ljones/aura/")).map(String::from)
    })
    .clone()
}

/// `busctl get-property` output with the type prefix removed.
pub fn busctl_get(path: &str, iface: &str, prop: &str) -> Option<String> {
    let out = cmd::output(&["busctl", "--system", "get-property", BUS, path, iface, prop])?;
    let (_, value) = out.split_once(' ')?;
    Some(value.trim().to_string())
}

// ----- Aura -----

pub fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(c.to_lowercase());
    }
    out
}

pub fn pretty(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// What an effect needs: colours (0–2), a speed, a direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectParams {
    pub colours: u8,
    pub speed: bool,
    pub direction: bool,
}

pub fn effect_params(kebab_name: &str) -> EffectParams {
    let (colours, speed, direction) = match kebab_name {
        "static" | "pulse" | "comet" | "flash" => (1, false, false),
        "breathe" | "stars" => (2, true, false),
        "rainbow-cycle" | "rain" => (0, true, false),
        "rainbow-wave" => (0, true, true),
        "highlight" | "laser" | "ripple" => (1, true, false),
        _ => (1, false, false),
    };
    EffectParams { colours, speed, direction }
}

/// The `asusctl aura effect …` arguments for an effect.
pub fn effect_args(kebab_name: &str, c1: &str, c2: &str, speed: &str, direction: &str) -> Vec<String> {
    let p = effect_params(kebab_name);
    let mut a: Vec<String> = vec!["aura".into(), "effect".into(), kebab_name.into()];
    if p.colours >= 1 {
        a.extend(["--colour".into(), c1.into()]);
    }
    if p.colours >= 2 {
        a.extend(["--colour2".into(), c2.into()]);
    }
    if p.speed {
        a.extend(["--speed".into(), speed.into()]);
    }
    if p.direction {
        a.extend(["--direction".into(), direction.into()]);
    }
    a
}

/// `#rrggbb` → `rrggbb` (what asusctl wants).
pub fn hex6(colour: &str) -> String {
    colour.trim().trim_start_matches('#').to_lowercase()
}

/// The effect the keyboard is showing now.
#[derive(Debug, Clone, PartialEq)]
pub struct AuraState {
    pub mode: String,
    pub colour1: String,
    pub colour2: String,
    pub speed: String,
    pub direction: String,
}

/// asusd's numeric effect ids, as kebab-case names.
pub fn mode_name(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "static",
        1 => "breathe",
        2 => "rainbow-cycle",
        3 => "rainbow-wave",
        4 => "stars",
        5 => "rain",
        6 => "highlight",
        7 => "laser",
        8 => "ripple",
        10 => "pulse",
        11 => "comet",
        12 => "flash",
        _ => return None,
    })
}

/// Parse `busctl get-property … LedModeData`: `0 0 137 180 250 0 0 0 "Med" "Right"`.
pub fn parse_mode_data(text: &str) -> Option<AuraState> {
    let mut parts = text.split_whitespace();
    let mode = mode_name(parts.next()?.parse().ok()?)?.to_string();
    let _zone = parts.next()?;
    let rgb = |it: &mut std::str::SplitWhitespace| -> Option<String> {
        let (r, g, b): (u8, u8, u8) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    };
    let colour1 = rgb(&mut parts)?;
    let colour2 = rgb(&mut parts)?;
    let speed = parts.next()?.trim_matches('"').to_lowercase();
    let direction = parts.next()?.trim_matches('"').to_lowercase();
    Some(AuraState { mode, colour1, colour2, speed, direction })
}

pub fn aura_state() -> Option<AuraState> {
    let path = aura_path()?;
    parse_mode_data(&busctl_get(&path, "xyz.ljones.Aura", "LedModeData")?)
}

/// Lighting on/off per power state: boot, awake, sleep, shutdown.
pub type PowerStates = [bool; 4];

/// Parse `busctl get-property … LedPower`: `1 1 true true true true` (a count, then zone + four flags each).
pub fn parse_led_power(text: &str) -> Vec<(u32, PowerStates)> {
    let mut it = text.split_whitespace();
    let Some(n) = it.next().and_then(|n| n.parse::<usize>().ok()) else { return vec![] };
    let mut out = Vec::new();
    for _ in 0..n {
        let Some(zone) = it.next().and_then(|z| z.parse::<u32>().ok()) else { break };
        let mut flags = [false; 4];
        for f in flags.iter_mut() {
            *f = it.next() == Some("true");
        }
        out.push((zone, flags));
    }
    out
}

pub fn aura_power() -> Vec<(u32, PowerStates)> {
    aura_path().and_then(|p| busctl_get(&p, "xyz.ljones.Aura", "LedPower")).map(|t| parse_led_power(&t)).unwrap_or_default()
}

/// `asusctl aura power <zone> --boot --awake …` (flags not given mean off).
pub fn power_args(zone_kebab: &str, s: PowerStates) -> Vec<String> {
    let mut a: Vec<String> = vec!["aura".into(), "power".into(), zone_kebab.into()];
    for (on, flag) in s.iter().zip(["--boot", "--awake", "--sleep", "--shutdown"]) {
        if *on {
            a.push(flag.into());
        }
    }
    a
}

// ----- Saved lighting choice, kept across Omarchy theme changes -----

/// Omarchy resets ASUS keyboards to a static theme colour on every theme
/// change. Settings saves what the user picked and re-applies it from a
/// theme-set hook: with the new theme colour when following the theme, or
/// exactly as chosen otherwise.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AuraChoice {
    pub mode: String,
    pub colour1: String,
    pub colour2: String,
    pub speed: String,
    pub direction: String,
    pub follow_theme: bool,
}

impl Default for AuraChoice {
    fn default() -> Self {
        Self {
            mode: "static".into(),
            colour1: "#7aa2f7".into(),
            colour2: "#000000".into(),
            speed: "med".into(),
            direction: "right".into(),
            follow_theme: true,
        }
    }
}

pub fn aura_file() -> std::path::PathBuf {
    crate::paths::app_dir().join("aura.toml")
}

pub fn load_aura() -> Option<AuraChoice> {
    std::fs::read_to_string(aura_file()).ok().and_then(|t| toml::from_str(&t).ok())
}

pub fn save_aura(c: &AuraChoice) -> Result<()> {
    cmd::atomic_write(&aura_file(), &toml::to_string_pretty(c)?)
}

/// The keyboard colour the current Omarchy theme asks for (its `keyboard.rgb`,
/// else its accent).
pub fn theme_keyboard_colour() -> Option<String> {
    let dir = crate::paths::omarchy_theme_dir();
    let from_file = std::fs::read_to_string(dir.join("keyboard.rgb")).ok().map(|t| t.trim().to_lowercase());
    let valid = |c: &String| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
    from_file.filter(valid).or_else(|| crate::theme::omarchy_palette().map(|p| p.accent.to_lowercase()).filter(valid))
}

/// The colours the keyboard should actually show for a choice.
pub fn resolved(c: &AuraChoice) -> AuraChoice {
    let mut c = c.clone();
    if c.follow_theme
        && let Some(t) = theme_keyboard_colour()
    {
        c.colour1 = t;
    }
    c
}

pub fn apply_aura(c: &AuraChoice) -> Result<()> {
    let r = resolved(c);
    run_owned(effect_args(&r.mode, &hex6(&r.colour1), &hex6(&r.colour2), &r.speed, &r.direction))?;
    Ok(())
}

/// Re-apply the saved lighting; part of `settings --theme-sync`.
pub fn sync_after_theme() -> Result<()> {
    if !installed() || !has_aura() {
        return Ok(());
    }
    // Nothing saved yet: follow the theme with the default static effect,
    // which is what Omarchy itself just did.
    let mut c = load_aura().unwrap_or_default();
    // A saved effect this keyboard can't show properly falls back to static.
    if !usable_modes(support()).contains(&c.mode) {
        c.mode = "static".into();
    }
    apply_aura(&c)
}

// ----- Slash -----

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slash {
    pub enabled: bool,
    pub brightness: u32,
    pub interval: u32,
    pub mode: String,
    pub on_boot: bool,
    pub on_shutdown: bool,
    pub on_sleep: bool,
    pub on_battery: bool,
    pub battery_warning: bool,
}

pub fn parse_slash(text: &str) -> Slash {
    let mut s = Slash::default();
    let b = |v: &str| v.trim() == "true";
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "Slash LED" => s.enabled = v == "enabled",
            "Brightness" => s.brightness = v.parse().unwrap_or(0),
            "Interval" => s.interval = v.parse().unwrap_or(0),
            "Mode" => s.mode = v.to_string(),
            "Show on boot" => s.on_boot = b(v),
            "Show on shutdown" => s.on_shutdown = b(v),
            "Show on sleep" => s.on_sleep = b(v),
            "Show on battery" => s.on_battery = b(v),
            "Show battery warning" => s.battery_warning = b(v),
            _ => {}
        }
    }
    s
}

pub fn slash() -> Slash {
    cmd::output(&["asusctl", "slash", "get"]).map(|t| parse_slash(&t)).unwrap_or_default()
}

pub fn slash_modes() -> Vec<String> {
    cmd::output(&["asusctl", "slash", "list"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

// ----- Armoury (firmware attributes) -----

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// One of a few fixed values.
    Choice { choices: Vec<i64>, current: i64 },
    /// A number in a range, with the firmware default.
    Range { min: i64, max: i64, current: i64, default: Option<i64> },
    /// Shown but not changeable.
    Fixed(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub kind: Kind,
}

/// Parse `asusctl armoury list`.
pub fn parse_armoury(text: &str) -> Vec<Attribute> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut current: Option<String> = None;
    let mut default: Option<i64> = None;
    let flush = |name: &mut Option<String>, current: &mut Option<String>, default: &mut Option<i64>, out: &mut Vec<Attribute>| {
        if let (Some(n), Some(c)) = (name.take(), current.take())
            && let Some(kind) = parse_current(&c, *default)
        {
            out.push(Attribute { name: n, kind });
        }
        *default = None;
    };
    for line in text.lines() {
        if line.starts_with('[') {
            continue; // log lines such as "[WARN  rog_dbus] …"
        }
        if !line.starts_with(' ') && line.trim_end().ends_with(':') {
            flush(&mut name, &mut current, &mut default, &mut out);
            name = Some(line.trim().trim_end_matches(':').to_string());
        } else if let Some(v) = line.trim().strip_prefix("current:") {
            current = Some(v.trim().to_string());
        } else if let Some(v) = line.trim().strip_prefix("default:") {
            default = v.trim().parse().ok();
        }
    }
    flush(&mut name, &mut current, &mut default, &mut out);
    out
}

fn parse_current(c: &str, default: Option<i64>) -> Option<Kind> {
    if let Some(inner) = c.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let mut choices = Vec::new();
        let mut cur = None;
        for part in inner.split(',') {
            let p = part.trim();
            let selected = p.starts_with('(') && p.ends_with(')');
            let n: i64 = p.trim_matches(|c| c == '(' || c == ')').parse().ok()?;
            if selected {
                cur = Some(n);
            }
            choices.push(n);
        }
        return Some(Kind::Choice { current: cur.or(choices.first().copied())?, choices });
    }
    if let Some((min, rest)) = c.split_once("..[") {
        let (cur, max) = rest.split_once("]..")?;
        return Some(Kind::Range {
            min: min.trim().parse().ok()?,
            max: max.trim().parse().ok()?,
            current: cur.trim().parse().ok()?,
            default,
        });
    }
    Some(Kind::Fixed(c.trim().parse().ok()?))
}

pub fn armoury() -> Vec<Attribute> {
    cmd::output(&["asusctl", "armoury", "list"]).map(|t| parse_armoury(&t)).unwrap_or_default()
}

pub fn armoury_set(name: &str, value: i64) {
    apply(vec!["armoury".into(), "set".into(), name.into(), value.to_string()]);
}

// ----- Fan curves -----

#[derive(Debug, Clone, PartialEq)]
pub struct FanCurve {
    pub fan: String,
    pub pwm: Vec<u32>,
    pub temp: Vec<u32>,
    pub enabled: bool,
}

/// Parse `asusctl fan-curve --mod-profile <p>` (RON-like text).
pub fn parse_fan_curves(text: &str) -> Vec<FanCurve> {
    let nums = |line: &str| -> Vec<u32> {
        line.split_once('(')
            .map(|(_, r)| r.trim_end_matches([')', ',', ' ']).split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let mut cur: Option<FanCurve> = None;
    for line in text.lines().map(str::trim) {
        if let Some(f) = line.strip_prefix("fan:") {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            cur = Some(FanCurve { fan: f.trim().trim_end_matches(',').to_string(), pwm: vec![], temp: vec![], enabled: false });
        } else if let Some(c) = cur.as_mut() {
            if line.starts_with("pwm:") {
                c.pwm = nums(line);
            } else if line.starts_with("temp:") {
                c.temp = nums(line);
            } else if let Some(e) = line.strip_prefix("enabled:") {
                c.enabled = e.trim().trim_end_matches(',') == "true";
            }
        }
    }
    out.extend(cur);
    out
}

pub fn fan_curves(profile: &str) -> Vec<FanCurve> {
    cmd::output(&["asusctl", "fan-curve", "--mod-profile", profile]).map(|t| parse_fan_curves(&t)).unwrap_or_default()
}

/// The `--data` string with raw fan values (0–255, no `%`), so points the user
/// didn't touch are written back exactly as they were: `30c:25,57c:43,…`.
/// Fails if the curve isn't rising.
pub fn fan_data(temp: &[u32], pwm: &[u32]) -> Result<String> {
    if temp.len() != pwm.len() || temp.is_empty() {
        bail!("a fan curve needs matching temperatures and speeds");
    }
    if temp.windows(2).any(|w| w[0] >= w[1]) {
        bail!("temperatures must rise from left to right");
    }
    if pwm.windows(2).any(|w| w[0] > w[1]) {
        bail!("fan speed can't drop as the temperature rises");
    }
    if pwm.iter().any(|p| *p > 255) {
        bail!("fan speed is at most 100%");
    }
    Ok(temp.iter().zip(pwm).map(|(t, p)| format!("{t}c:{p}")).collect::<Vec<_>>().join(","))
}

/// PWM (0–255) to a percentage.
pub fn pwm_to_percent(pwm: u32) -> u32 {
    ((pwm as f64 / 255.0) * 100.0).round() as u32
}

/// A percentage to PWM (0–255).
pub fn percent_to_pwm(percent: u32) -> u32 {
    ((percent.min(100) as f64 / 100.0) * 255.0).round() as u32
}

// ----- Anime matrix, XG Mobile, drive LEDs -----

pub fn xgm_state() -> Option<bool> {
    let out = cmd::run(&["asusctl", "xgmled", "get"]).ok()?;
    if out.contains("Did not find") {
        return None;
    }
    let l = out.to_lowercase();
    Some(l.contains("on") || l.contains('1') || l.contains("true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUPPORTED: &str = r#"asusctl v6.5.0

Software version: 6.5.0
  Product family: ROG Zephyrus G16
      Board name: GU605CW

Supported Core Functions:
[
    "xyz.ljones.AsusArmoury",
    "xyz.ljones.Aura",
    "xyz.ljones.Backlight",
    "xyz.ljones.FanCurves",
    "xyz.ljones.Platform",
    "xyz.ljones.Slash",
]
Supported Platform Properties:
[
    ChargeControlEndThreshold,
    ThrottlePolicy,
]
Supported Keyboard Brightness:
[
    Off,
    Low,
    Med,
    High,
]
Supported Aura Modes:
[
    Static,
    Breathe,
    RainbowCycle,
    RainbowWave,
    Pulse,
]
Supported Aura Zones:
[]
Supported Aura Power Zones:
[
    Keyboard,
]
"#;

    #[test]
    fn parses_supported() {
        let s = parse_supported(SUPPORTED);
        assert_eq!(s.product, "ROG Zephyrus G16");
        assert_eq!(s.board, "GU605CW");
        assert!(s.has_core("Slash") && s.has_core("FanCurves") && s.has_core("AsusArmoury"));
        assert!(!s.has_core("Anime") && !s.has_core("Scsi"));
        assert!(s.has_platform("ThrottlePolicy") && s.has_platform("ChargeControlEndThreshold"));
        assert_eq!(s.brightness, ["Off", "Low", "Med", "High"]);
        assert_eq!(s.modes, ["Static", "Breathe", "RainbowCycle", "RainbowWave", "Pulse"]);
        assert!(s.zones.is_empty());
        assert_eq!(s.power_zones, ["Keyboard"]);
    }

    #[test]
    fn hides_rainbow_wave_without_zones() {
        let mut s = parse_supported(SUPPORTED);
        assert_eq!(usable_modes(&s), ["static", "breathe", "rainbow-cycle", "pulse"]);
        s.zones = vec!["Key1".into(), "Key2".into()];
        assert!(usable_modes(&s).contains(&"rainbow-wave".to_string()));
    }

    #[test]
    fn unsupported_machine_has_nothing() {
        let s = parse_supported("asusctl v6.5.0\n\nSupported Core Functions:\n[]\n");
        assert!(s.core.is_empty() && s.modes.is_empty());
    }

    #[test]
    fn names() {
        assert_eq!(kebab("RainbowWave"), "rainbow-wave");
        assert_eq!(kebab("Keyboard"), "keyboard");
        assert_eq!(pretty("RainbowCycle"), "Rainbow Cycle");
    }

    #[test]
    fn effect_commands() {
        assert_eq!(effect_args("static", "89b4fa", "", "", ""), ["aura", "effect", "static", "--colour", "89b4fa"]);
        assert_eq!(
            effect_args("breathe", "ff0000", "0000ff", "med", ""),
            ["aura", "effect", "breathe", "--colour", "ff0000", "--colour2", "0000ff", "--speed", "med"]
        );
        assert_eq!(
            effect_args("rainbow-wave", "", "", "high", "left"),
            ["aura", "effect", "rainbow-wave", "--speed", "high", "--direction", "left"]
        );
        assert_eq!(effect_args("rain", "", "", "low", ""), ["aura", "effect", "rain", "--speed", "low"]);
        assert_eq!(hex6("#89B4FA"), "89b4fa");
    }

    #[test]
    fn parses_mode_data() {
        let s = parse_mode_data("0 0 137 180 250 0 0 0 \"Med\" \"Right\"").unwrap();
        assert_eq!(
            s,
            AuraState {
                mode: "static".into(),
                colour1: "#89b4fa".into(),
                colour2: "#000000".into(),
                speed: "med".into(),
                direction: "right".into()
            }
        );
        assert!(parse_mode_data("99 0 0 0 0 0 0 0 \"Med\" \"Right\"").is_none());
    }

    #[test]
    fn parses_power() {
        assert_eq!(parse_led_power("1 1 true false true true"), vec![(1, [true, false, true, true])]);
        assert!(parse_led_power("").is_empty());
        assert_eq!(
            power_args("keyboard", [true, true, false, true]),
            ["aura", "power", "keyboard", "--boot", "--awake", "--shutdown"]
        );
        assert_eq!(power_args("logo", [false; 4]), ["aura", "power", "logo"]);
    }

    #[test]
    fn aura_choice_defaults_follow_theme() {
        let c: AuraChoice = toml::from_str("mode = \"breathe\"").unwrap();
        assert_eq!(c.mode, "breathe");
        assert!(c.follow_theme);
        let round: AuraChoice = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(round, c);
    }

    #[test]
    fn not_following_keeps_colours() {
        let c = AuraChoice { follow_theme: false, colour1: "#123456".into(), ..AuraChoice::default() };
        assert_eq!(resolved(&c).colour1, "#123456");
    }

    #[test]
    fn parses_slash() {
        let s = parse_slash(
            "Slash LED: disabled\nBrightness: 133\nInterval: 0\nMode: Bounce\nShow on boot: true\nShow on shutdown: true\nShow on sleep: false\nShow on battery: false\nShow battery warning: true\n",
        );
        assert!(!s.enabled && s.on_boot && s.on_shutdown && s.battery_warning && !s.on_sleep && !s.on_battery);
        assert_eq!((s.brightness, s.interval, s.mode.as_str()), (133, 0, "Bounce"));
        assert!(parse_slash("Slash LED: enabled").enabled);
    }

    #[test]
    fn parses_armoury() {
        let text = "[WARN  rog_dbus] Multiple asusd interfaces devices found\nboot_sound:\n  current: [0,(1)]\n\nnv_base_tgp:\n  current: 80\n\nnv_tgp:\n  current: 80..[110]..110\n  default: 90\n\ncharge_mode:\n  current: [0,(1),2]\n";
        let a = parse_armoury(text);
        assert_eq!(a.len(), 4);
        assert_eq!(a[0], Attribute { name: "boot_sound".into(), kind: Kind::Choice { choices: vec![0, 1], current: 1 } });
        assert_eq!(a[1].kind, Kind::Fixed(80));
        assert_eq!(a[2].kind, Kind::Range { min: 80, max: 110, current: 110, default: Some(90) });
        assert_eq!(a[3].kind, Kind::Choice { choices: vec![0, 1, 2], current: 1 });
    }

    #[test]
    fn parses_fan_curves() {
        let text = "Fan curves for Performance\n\n[\n    (\n        fan: CPU,\n        pwm: (25, 43, 61, 84, 107, 130, 163, 186),\n        temp: (0, 57, 61, 65, 69, 73, 78, 83),\n        enabled: false,\n    ),\n    (\n        fan: GPU,\n        pwm: (20, 30, 48, 66, 84, 122, 158, 181),\n        temp: (0, 47, 52, 57, 62, 67, 72, 77),\n        enabled: true,\n    ),\n]\n";
        let c = parse_fan_curves(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].fan, "CPU");
        assert_eq!(c[0].pwm.len(), 8);
        assert_eq!(c[0].temp[1], 57);
        assert!(!c[0].enabled && c[1].enabled);
    }

    #[test]
    fn fan_data_is_validated() {
        assert_eq!(fan_data(&[0, 57, 61], &[25, 43, 61]).unwrap(), "0c:25,57c:43,61c:61");
        assert!(fan_data(&[30, 30], &[10, 20]).is_err());
        assert!(fan_data(&[30, 40], &[20, 10]).is_err());
        assert!(fan_data(&[30, 40], &[10, 300]).is_err());
        assert!(fan_data(&[], &[]).is_err());
        assert_eq!(pwm_to_percent(255), 100);
        assert_eq!(pwm_to_percent(0), 0);
        assert_eq!(percent_to_pwm(100), 255);
        // Untouched points survive a round trip through the UI's percentage exactly.
        let original = [25u32, 43, 61, 84, 107, 130, 163, 186];
        let data = fan_data(&[0, 57, 61, 65, 69, 73, 78, 83], &original).unwrap();
        assert_eq!(data, "0c:25,57c:43,61c:61,65c:84,69c:107,73c:130,78c:163,83c:186");
    }
}
