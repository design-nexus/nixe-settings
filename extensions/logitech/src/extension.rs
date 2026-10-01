//! Logitech devices as pages in the Settings app.
//!
//! Speaks the Settings extension protocol (JSON on stdout): `pages`, `describe PAGE`
//! and `set PAGE KEY VALUE`. Changes are saved to `devices.toml`, so the `watch`
//! service puts them back on reconnect.

use crate::devices::{self, Found};
use crate::hidpp::device::{Info, Kind, feature as f};
use crate::hidpp::features::gaming::Effect;
use crate::hidpp::features::keyboard::DISABLE_KEY_NAMES;
use crate::hidpp::features::wheel::WheelMode;
use crate::hidpp::features::{battery, buttons, gaming, keyboard, pointer, wheel};
use crate::hidpp::link::Link;
use crate::state::{self, Saved, ZoneSaved};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// Every device on every node, and whether any node was refused for lack of permission.
fn scan_all() -> (Vec<(usize, Found)>, bool) {
    let mut out = Vec::new();
    let mut opened = false;
    let nodes = devices::scan();
    for (n, node) in nodes.iter().enumerate() {
        match devices::open(node) {
            Ok(mut link) => {
                opened = true;
                if let Ok(d) = devices::discover(&mut link, node) {
                    out.extend(d.devices.into_iter().map(|f| (n, f)));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(_) => opened = true,
        }
    }
    (out, !nodes.is_empty() && !opened)
}

/// Open the node a device is on and identify it again.
fn connect(key: &str) -> Result<(Link, Found)> {
    for node in devices::scan() {
        let Ok(mut link) = devices::open(&node) else { continue };
        let Ok(d) = devices::discover(&mut link, &node) else { continue };
        if let Some(f) = d.devices.into_iter().find(|f| f.info.key() == key) {
            return Ok((link, f));
        }
    }
    bail!("that device isn't here any more")
}

fn options<A: ToString, B: ToString>(pairs: impl IntoIterator<Item = (A, B)>) -> Value {
    Value::Array(pairs.into_iter().map(|(a, b)| json!([a.to_string(), b.to_string()])).collect())
}

fn accent() -> Option<String> {
    crate::files::theme_accent().map(|a| a.to_lowercase())
}

/// The page shown instead of devices while Logitech devices can't be opened.
const SETUP: &str = "setup";

pub fn pages() -> Value {
    let (found, denied) = scan_all();
    if denied {
        return json!([{
            "id": SETUP, "title": "Logitech", "icon": "input-mouse-symbolic",
            "description": "Logitech devices are plugged in, but this account can't talk to them yet.",
            "keywords": "logitech logi permission access udev",
        }]);
    }
    let mut seen = std::collections::HashSet::new();
    let pages: Vec<Value> = found
        .into_iter()
        .filter(|(_, f)| f.info.kind != Kind::Receiver && seen.insert(f.info.key()))
        .map(|(_, f)| {
            json!({
                "id": f.info.key(),
                "title": f.info.name,
                "icon": f.info.kind.icon(),
                "description": format!("Logitech {}: battery, buttons and everything the device lets you change.", f.info.kind.label().to_lowercase()),
                "keywords": format!("logitech logi {} {}", f.info.kind.label().to_lowercase(), f.info.name.to_lowercase()),
            })
        })
        .collect();
    Value::Array(pages)
}

fn row_err(title: &str, e: impl std::fmt::Display) -> Value {
    json!({ "kind": "info", "title": title, "value": format!("Couldn't read this: {e}") })
}

pub fn describe(key: &str) -> Result<Value> {
    if key == SETUP {
        return Ok(setup_page());
    }
    let (mut link, found) = connect(key)?;
    let mut page = describe_on(&mut link, &found)?;
    if let Some(groups) = page["groups"].as_array_mut() {
        groups.push(service_group());
    }
    Ok(page)
}

fn setup_page() -> Value {
    json!({
        "banners": [{ "text": "Linux only lets <b>root</b> talk to these devices until a small rule allows your account to.", "warning": true }],
        "groups": [{ "title": "Device access", "rows": [{
            "kind": "button", "key": "allow", "title": "Allow access to Logitech devices",
            "desc": format!("Installs <tt>{}</tt>. Asks for your password.", crate::paths::udev_rule().display()),
            "label": "Allow access",
        }]}],
    })
}

/// The background service that puts saved settings back when devices reconnect.
fn service_group() -> Value {
    let row = if crate::service::logi_watch_enabled() {
        json!({ "kind": "info", "title": "Keep settings applied", "value": "On (Logi's background service)" })
    } else {
        json!({
            "kind": "switch", "key": "keep-applied", "title": "Keep settings applied",
            "desc": "Many devices forget their settings when switched off. This puts them back whenever a device connects.",
            "value": crate::service::enabled(), "keywords": "background service watch reconnect restore",
        })
    };
    json!({ "title": "Logitech", "rows": [row] })
}

fn describe_on(link: &mut Link, found: &Found) -> Result<Value> {
    let info = found.info.clone();
    let saved = state::load().devices.get(&info.key()).cloned().unwrap_or_default();
    let mut groups: Vec<Value> = Vec::new();
    if !found.online {
        groups.push(json!({ "title": "Not connected", "rows": [{
            "kind": "info", "title": "This device isn't connected right now",
            "value": "Switched off, asleep or out of range",
        }]}));
        groups.push(device_group(&info));
        return Ok(json!({ "groups": groups, "poll": 15 }));
    }
    let l = link;
    if let Ok(Some(b)) = battery::read(l, &info) {
        let mut row = json!({ "kind": "info", "title": "Charge", "value": b.describe(), "keywords": "battery level power charging" });
        if b.is_low() {
            row["tag"] = json!("Charge soon");
        }
        groups.push(json!({ "title": "Battery", "rows": [row] }));
    }

    // Pointer
    let mut rows = Vec::new();
    if info.has(f::ADJUSTABLE_DPI) {
        match pointer::read_dpi(l, &info) {
            Ok(dpi) => {
                let (min, max, step) = dpi.choices.bounds();
                rows.push(json!({
                    "kind": "slider", "key": "dpi", "title": "Pointer speed",
                    "desc": "How far the pointer moves as you move the mouse. Higher is faster.",
                    "value": dpi.current, "min": min, "max": max, "step": step, "unit": " dpi", "reset": dpi.default,
                    "keywords": "dpi sensitivity cpi speed acceleration",
                }));
            }
            Err(e) => rows.push(row_err("Pointer speed", e)),
        }
    }
    if info.has(f::REPORT_RATE) || info.has(f::EXTENDED_REPORT_RATE) {
        match pointer::read_report_rate(l, &info) {
            Ok(rate) => {
                let kind = if rate.choices.len() <= 5 { "segmented" } else { "choice" };
                rows.push(json!({
                    "kind": kind, "key": "report-rate", "title": "Report rate",
                    "desc": "Position updates per second (Hz). Higher feels smoother and uses more battery.",
                    "options": options(rate.choices.iter().map(|hz| (hz, if kind == "choice" { format!("{hz} Hz") } else { hz.to_string() }))),
                    "value": rate.current.to_string(), "keywords": "polling rate hz latency",
                }));
            }
            Err(e) => rows.push(row_err("Report rate", e)),
        }
    }
    if !rows.is_empty() {
        groups.push(json!({ "title": "Pointer", "rows": rows }));
    }

    // Scrolling
    let mut rows = Vec::new();
    if info.has(f::SMART_SHIFT) || info.has(f::SMART_SHIFT_ENHANCED) {
        match wheel::read_mode(l, &info) {
            Ok(mode) => {
                rows.push(json!({
                    "kind": "segmented", "key": "wheel", "title": "Wheel mode",
                    "desc": "Ratchet clicks line by line. Free spin glides. SmartShift clicks until you flick it, then spins free.",
                    "options": options([("ratchet", "Ratchet"), ("free", "Free spin"), ("smart", "SmartShift")]),
                    "value": mode.id(), "refresh": true, "keywords": "smartshift ratchet free spin magspeed wheel",
                }));
                if let WheelMode::SmartShift(t) = mode {
                    rows.push(json!({
                        "kind": "slider", "key": "smartshift", "title": "Shift threshold",
                        "desc": "How hard a flick has to be before the wheel spins freely. Lower shifts sooner.",
                        "value": t.clamp(1, 50), "min": 1, "max": 50, "keywords": "smartshift sensitivity",
                    }));
                }
            }
            Err(e) => rows.push(row_err("Wheel mode", e)),
        }
    }
    if info.has(f::HIRES_WHEEL) {
        match wheel::read_hires(l, &info) {
            Ok(h) if h.can_invert => rows.push(json!({
                "kind": "switch", "key": "invert-wheel", "title": "Reverse wheel direction",
                "desc": "Content follows the wheel, like on a touchpad.", "value": h.inverted,
                "keywords": "natural scrolling invert reverse direction",
            })),
            Ok(_) => {}
            Err(e) => rows.push(row_err("Scroll wheel", e)),
        }
    }
    if info.has(f::THUMB_WHEEL) {
        match wheel::read_thumb_inverted(l, &info) {
            Ok(v) => rows.push(json!({
                "kind": "switch", "key": "invert-thumb", "title": "Reverse thumb wheel",
                "desc": "Swap which way the side wheel scrolls.", "value": v,
                "keywords": "horizontal scrolling side wheel invert",
            })),
            Err(e) => rows.push(row_err("Thumb wheel", e)),
        }
    }
    if !rows.is_empty() {
        groups.push(json!({ "title": "Scrolling", "rows": rows }));
    }

    // Keys
    let mut rows = Vec::new();
    if keyboard::has_fn_swap(&info) {
        match keyboard::read_fn_swap(l, &info) {
            Ok(v) => rows.push(json!({
                "kind": "switch", "key": "fn-swap", "title": "Media keys first",
                "desc": "The top row controls brightness, volume and playback. Hold <b>Fn</b> for F1–F12.", "value": v,
                "keywords": "fn lock function keys f1 f12 swap invert",
            })),
            Err(e) => rows.push(row_err("Fn keys", e)),
        }
    }
    if info.has(f::DISABLE_KEYS) {
        match keyboard::read_disabled_keys(l, &info) {
            Ok((caps, now)) => {
                let bits: Vec<u8> = (0..DISABLE_KEY_NAMES.len() as u8).filter(|b| caps & (1 << b) != 0).collect();
                rows.push(json!({
                    "kind": "chips", "key": "disabled-keys", "title": "Turn off keys",
                    "desc": "Keys selected here do nothing, so they can't be hit by accident.",
                    "labels": bits.iter().map(|b| DISABLE_KEY_NAMES[*b as usize]).collect::<Vec<_>>(),
                    "value": bits.iter().map(|b| now & (1 << b) != 0).collect::<Vec<_>>(),
                    "keywords": "disable caps lock insert windows super key",
                }));
            }
            Err(e) => rows.push(row_err("Turn off keys", e)),
        }
    }
    if !rows.is_empty() {
        groups.push(json!({ "title": "Keys", "rows": rows }));
    }

    if info.has(f::BACKLIGHT2) {
        let row = match keyboard::read_backlight(l, &info) {
            Ok(b) => {
                let mut more = Vec::new();
                if b.levels > 1 {
                    more.push(json!({
                        "kind": "slider", "key": "backlight-level", "title": "Brightness",
                        "desc": "Setting a level turns off automatic brightness.",
                        "value": b.level.min(b.levels - 1), "min": 0, "max": b.levels - 1,
                    }));
                }
                more.push(json!({
                    "kind": "slider", "key": "backlight-timeout", "title": "Turn off after",
                    "desc": "How long the light stays on after you stop typing.",
                    "value": b.timeout.clamp(5, 300), "min": 5, "max": 300, "step": 5, "unit": " s",
                }));
                json!({
                    "kind": "switch", "key": "backlight", "title": "Backlight",
                    "desc": "Light up the keys when your hands are near.", "value": b.enabled, "rows": more,
                    "keywords": "backlight illumination light keys",
                })
            }
            Err(e) => row_err("Backlight", e),
        };
        groups.push(json!({ "title": "Backlight", "rows": [row] }));
    }

    if info.has(f::REPROG_CONTROLS_V4) {
        let pointing = matches!(info.kind, Kind::Mouse | Kind::Trackball | Kind::Touchpad);
        let title = if pointing { "Buttons" } else { "Special keys" };
        match buttons::read(l, &info) {
            Ok(controls) => {
                let rows: Vec<Value> = controls
                    .iter()
                    .filter(|c| c.reprogrammable() && !c.is_virtual() && c.targets(&controls).len() > 1)
                    .map(|c| {
                        let targets = c.targets(&controls);
                        json!({
                            "kind": "choice", "key": format!("button:{}", c.cid), "title": buttons::name(c.cid),
                            "options": options(targets.iter().map(|t| {
                                (t.cid, if t.cid == c.cid { "Default".to_string() } else { buttons::name(t.cid) })
                            })),
                            "value": c.remap.to_string(), "keywords": "button remap reassign action",
                        })
                    })
                    .collect();
                if !rows.is_empty() {
                    let group = if pointing {
                        json!({ "title": title, "note": "Each one can act as another. Changes are put back whenever the device reconnects.", "rows": rows })
                    } else {
                        json!({ "title": title, "rows": [{
                            "kind": "disclosure", "title": "Remap special keys",
                            "desc": format!("Make any of {} keys act as another one.", rows.len()), "rows": rows,
                        }]})
                    };
                    groups.push(group);
                }
            }
            Err(e) => groups.push(json!({ "title": title, "rows": [row_err(title, e)] })),
        }
    }

    if info.has(f::CHANGE_HOST)
        && let Ok((count, current)) = keyboard::read_hosts(l, &info)
        && count > 1
    {
        groups.push(json!({ "title": "Easy-Switch", "rows": [{
            "kind": "segmented", "key": "host", "title": "Computer",
            "desc": format!("This computer is <b>{}</b>. Picking another sends the device there right away.", current + 1),
            "options": options((0..count).map(|h| (h, h + 1))), "value": current.to_string(), "refresh": true,
            "keywords": "easy-switch channel host computer flow",
        }]}));
    }

    if info.has(f::ONBOARD_PROFILES) {
        match gaming::read_onboard(l, &info) {
            Ok(o) => {
                let mut more = Vec::new();
                if o.profiles > 1 {
                    more.push(json!({
                        "kind": if o.profiles <= 5 { "segmented" } else { "choice" }, "key": "profile", "title": "Profile",
                        "desc": "Which stored profile is active.", "options": options((1..=o.profiles).map(|p| (p, p))),
                        "value": o.current.to_string(),
                    }));
                }
                groups.push(json!({ "title": "Onboard profiles", "rows": [{
                    "kind": "switch", "key": "onboard", "title": "Use onboard profiles",
                    "desc": "Run from the settings stored in the mouse, as set up in G HUB, instead of from this computer.",
                    "value": o.onboard, "rows": more, "keywords": "onboard memory profiles g hub host mode",
                }]}));
            }
            Err(e) => groups.push(json!({ "title": "Onboard profiles", "rows": [row_err("Onboard profiles", e)] })),
        }
    }

    if info.has(f::COLOR_LED_EFFECTS) {
        match gaming::read_zones(l, &info) {
            Ok(zones) => {
                let accent = accent();
                let mut rows = Vec::new();
                for z in &zones {
                    let following = saved.lighting.get(&z.index.to_string()).is_some_and(|s| s.colour == "theme");
                    let effects = Effect::ALL.iter().filter(|e| z.effects.iter().any(|(x, _)| x == *e)).map(|e| (e.key(), e.label()));
                    let name = z.name();
                    rows.push(json!({
                        "kind": "segmented", "key": format!("zone:{}:effect", z.index), "title": name,
                        "options": options(effects), "value": z.effect.key(), "refresh": true,
                        "keywords": "lighting rgb led colour color effect breathe cycle logo",
                    }));
                    if matches!(z.effect, Effect::Static | Effect::Breathe) {
                        let shown = if following { accent.clone().unwrap_or_else(|| state::hex_colour(z.colour)) } else { state::hex_colour(z.colour) };
                        let mut c = json!({ "kind": "colour", "key": format!("zone:{}:colour", z.index), "title": format!("{name} colour"), "value": shown });
                        if let Some(a) = &accent {
                            c["theme"] = json!({ "label": "Omarchy theme", "colour": a });
                        }
                        rows.push(c);
                    }
                    if matches!(z.effect, Effect::Breathe | Effect::Cycle) {
                        rows.push(json!({
                            "kind": "slider", "key": format!("zone:{}:period", z.index), "title": format!("{name} speed"),
                            "desc": "Seconds per cycle.", "value": z.period as f64 / 1000.0, "min": 1, "max": 10, "step": 0.5, "digits": 1, "unit": " s",
                        }));
                    }
                }
                groups.push(json!({ "title": "Lighting", "rows": rows }));
            }
            Err(e) => groups.push(json!({ "title": "Lighting", "rows": [row_err("Lighting", e)] })),
        }
    } else if info.has(f::RGB_EFFECTS) {
        groups.push(json!({ "title": "Lighting", "rows": [{
            "kind": "info", "title": "Lighting isn't supported for this device yet",
            "value": "Newer RGB effects protocol",
            "tooltip": "The lights keep running their stored effect.",
        }]}));
    }

    if saved != (Saved { name: saved.name.clone(), ..Default::default() }) {
        groups.push(json!({ "title": "Saved settings", "rows": [{
            "kind": "button", "key": "forget", "title": "Put back on connect",
            "desc": "Settings you change here are written back whenever the device connects, since many forget them when switched off.",
            "label": "Forget", "confirm": "Click again to forget", "refresh": true,
            "keywords": "forget reset saved restore defaults",
        }]}));
    }
    groups.push(device_group(&info));
    Ok(json!({ "poll": 60, "groups": groups }))
}

fn device_group(i: &Info) -> Value {
    let mut rows = vec![json!({ "kind": "info", "title": "Type", "value": i.kind.label() })];
    if i.wpid != 0 {
        rows.push(json!({ "kind": "info", "title": "Model", "value": format!("{:04X}", i.wpid) }));
    }
    if !i.serial.is_empty() {
        rows.push(json!({ "kind": "info", "title": "Serial", "value": i.serial }));
    }
    for fw in &i.firmware {
        let (kind, rest) = fw.split_once(' ').unwrap_or(("Firmware", fw));
        rows.push(json!({ "kind": "info", "title": kind, "value": rest }));
    }
    json!({ "title": "Device", "rows": rows })
}

fn save(info: &Info, change: impl FnOnce(&mut Saved)) -> Result<()> {
    let mut store = state::load();
    let entry = store.devices.entry(info.key()).or_default();
    entry.name = info.name.clone();
    change(entry);
    state::save(&store)
}

fn flag(v: &str) -> Result<bool> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => bail!("expected true or false"),
    }
}

fn number<T: std::str::FromStr>(v: &str) -> Result<T> {
    let n: f64 = v.parse().context("expected a number")?;
    format!("{}", n.round() as i64).parse().ok().context("number out of range")
}

pub fn set(key: &str, what: &str, value: &str) -> Result<Option<Value>> {
    match what {
        "allow" => {
            crate::access::install_udev_rule()?;
            // Devices show up as their own pages now.
            return Ok(Some(json!({ "toast": "Access allowed", "reload": true })));
        }
        "keep-applied" => {
            crate::service::set(flag(value)?)?;
            return Ok(None);
        }
        _ => {}
    }
    let (mut link, found) = connect(key)?;
    if !found.online {
        bail!("{} isn't connected", found.info.name);
    }
    set_on(&mut link, &found.info, what, value)
}

fn set_on(l: &mut Link, info: &Info, what: &str, value: &str) -> Result<Option<Value>> {
    let info = info.clone();
    let hw = |r: crate::hidpp::link::Result<()>| r.map_err(|e| anyhow::anyhow!("{e}"));
    match what {
        "dpi" => {
            let d = pointer::read_dpi(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            let v = d.choices.snap(number(value)?);
            hw(pointer::write_dpi(l, &info, v))?;
            save(&info, |s| s.dpi = Some(v).filter(|v| *v != d.default))?;
        }
        "report-rate" => {
            let hz: u32 = number(value)?;
            hw(pointer::write_report_rate(l, &info, hz))?;
            save(&info, |s| s.report_rate = Some(hz))?;
        }
        "wheel" | "smartshift" => {
            let current = wheel::read_mode(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            let threshold = match current {
                WheelMode::SmartShift(t) => t.clamp(1, 50),
                _ => 12,
            };
            let m = match (what, value) {
                ("wheel", "ratchet") => WheelMode::Ratchet,
                ("wheel", "free") => WheelMode::FreeSpin,
                ("wheel", _) => WheelMode::SmartShift(threshold),
                _ => WheelMode::SmartShift(number::<u8>(value)?.clamp(1, 50)),
            };
            hw(wheel::write_mode(l, &info, m))?;
            save(&info, |s| s.wheel = Some(state::wheel_to_string(m)))?;
        }
        "invert-wheel" => {
            let on = flag(value)?;
            let now = wheel::read_hires(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            hw(wheel::write_hires(l, &info, now.hires, on))?;
            save(&info, |s| s.invert_wheel = Some(on))?;
        }
        "invert-thumb" => {
            let on = flag(value)?;
            hw(wheel::write_thumb_inverted(l, &info, on))?;
            save(&info, |s| s.invert_thumb_wheel = Some(on))?;
        }
        "fn-swap" => {
            let on = flag(value)?;
            hw(keyboard::write_fn_swap(l, &info, on))?;
            save(&info, |s| s.media_keys_first = Some(on))?;
        }
        "disabled-keys" => {
            let (caps, _) = keyboard::read_disabled_keys(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            let bits: Vec<u8> = (0..DISABLE_KEY_NAMES.len() as u8).filter(|b| caps & (1 << b) != 0).collect();
            let on: Vec<bool> = value.split(',').map(|b| b.trim() == "true").collect();
            let mask = bits.iter().zip(on).filter(|(_, on)| *on).fold(0u8, |m, (b, _)| m | (1 << b));
            hw(keyboard::write_disabled_keys(l, &info, mask))?;
            save(&info, |s| s.disabled_keys = Some(mask))?;
        }
        "backlight" | "backlight-level" | "backlight-timeout" => {
            let mut b = keyboard::read_backlight(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            let manual = what == "backlight-level";
            match what {
                "backlight" => b.enabled = flag(value)?,
                "backlight-level" => b.level = number(value)?,
                _ => b.timeout = number(value)?,
            }
            hw(keyboard::write_backlight(l, &info, &b, manual))?;
            save(&info, |s| {
                s.backlight = Some(b.enabled);
                s.backlight_timeout = Some(b.timeout);
                if manual {
                    s.backlight_level = Some(b.level);
                }
            })?;
        }
        "host" => {
            let host: u8 = number(value)?;
            hw(keyboard::switch_host(l, &info, host))?;
            return Ok(Some(json!({ "toast": format!("Sent to computer {}", host + 1) })));
        }
        "onboard" => {
            let on = flag(value)?;
            hw(gaming::write_onboard_mode(l, &info, on))?;
            save(&info, |s| s.onboard = Some(on))?;
        }
        "profile" => {
            let p: u8 = number(value)?;
            hw(gaming::write_profile(l, &info, p))?;
            save(&info, |s| s.profile = Some(p))?;
        }
        "forget" => {
            let mut store = state::load();
            store.devices.remove(&info.key());
            state::save(&store)?;
            return Ok(Some(json!({ "toast": "Saved settings forgotten. The device keeps what it has now." })));
        }
        w if w.starts_with("button:") => {
            let cid: u16 = w[7..].parse().context("bad button")?;
            let target: u16 = number(value)?;
            hw(buttons::remap(l, &info, cid, target))?;
            save(&info, |s| {
                let k = format!("0x{cid:04x}");
                if target == cid {
                    s.buttons.remove(&k);
                } else {
                    s.buttons.insert(k, target);
                }
            })?;
        }
        w if w.starts_with("zone:") => {
            let mut parts = w[5..].splitn(2, ':');
            let index: u8 = parts.next().unwrap_or("").parse().context("bad zone")?;
            let field = parts.next().unwrap_or("");
            let zones = gaming::read_zones(l, &info).map_err(|e| anyhow::anyhow!("{e}"))?;
            let z = zones.iter().find(|z| z.index == index).context("no such zone")?;
            let saved = state::load().devices.get(&info.key()).and_then(|s| s.lighting.get(&index.to_string()).cloned());
            let mut colour_key = saved.map(|s| s.colour).unwrap_or_else(|| state::hex_colour(z.colour));
            let (mut effect, mut colour, mut period) = (z.effect, z.colour, z.period);
            match field {
                "effect" => effect = Effect::from_key(value).context("unknown effect")?,
                "colour" => {
                    colour = state::parse_hex_colour(value).context("expected #rrggbb")?;
                    // Picking the theme swatch follows the theme from now on.
                    colour_key = if accent().as_deref() == Some(&value.to_lowercase()) { "theme".into() } else { value.to_lowercase() };
                }
                "period" => period = ((value.parse::<f64>().context("expected seconds")?) * 1000.0).round() as u16,
                _ => bail!("unknown lighting setting"),
            }
            hw(gaming::write_zone(l, &info, z, effect, colour, period))?;
            save(&info, |s| {
                s.lighting.insert(index.to_string(), ZoneSaved { effect: effect.key().into(), colour: colour_key, period });
            })?;
        }
        _ => bail!("unknown setting {what}"),
    }
    Ok(None)
}

/// `settings-logitech …`
pub fn run(args: &[String]) -> Result<()> {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let out = match arg(0) {
        "pages" => Some(pages()),
        "describe" => Some(describe(arg(1))?),
        "set" => set(arg(1), arg(2), arg(3))?,
        "theme-changed" => None,
        _ => bail!("usage: settings-logitech pages | describe PAGE | set PAGE KEY VALUE | watch"),
    };
    if let Some(v) = out {
        println!("{v}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::hidpp::sim;

    /// The first device behind simulated node `which`, with a link to it.
    fn sim_device(spec: &str, which: usize) -> (Link, Found) {
        let (node, io) = sim::nodes(spec).into_iter().nth(which).unwrap();
        let mut link = Link::new(Box::new(io), node.short, node.long);
        let d = devices::discover(&mut link, &node).unwrap();
        (link, d.devices.into_iter().next().unwrap())
    }

    fn group<'a>(d: &'a Value, title: &str) -> &'a Value {
        d["groups"].as_array().unwrap().iter().find(|g| g["title"] == title).unwrap_or_else(|| panic!("no {title} group"))
    }

    #[test]
    fn describes_a_mouse() {
        let (mut link, found) = sim_device("mx-master-3s", 0);
        let d = describe_on(&mut link, &found).unwrap();
        let dpi = &group(&d, "Pointer")["rows"][0];
        assert_eq!(dpi["kind"], "slider");
        assert_eq!((dpi["min"].as_u64(), dpi["max"].as_u64()), (Some(200), Some(8000)));
        assert_eq!(group(&d, "Scrolling")["rows"][0]["key"], "wheel");
        assert!(!group(&d, "Buttons")["rows"].as_array().unwrap().is_empty());
    }

    #[test]
    fn describes_gaming_lighting() {
        let (mut link, found) = sim_device("mx-master-3s,mx-keys,g502x", 1);
        let d = describe_on(&mut link, &found).unwrap();
        let rows = group(&d, "Lighting")["rows"].as_array().unwrap();
        assert_eq!(rows[0]["key"], "zone:0:effect");
        assert!(rows.iter().any(|r| r["key"] == "zone:1:period"), "breathing zone has a speed");
    }

    #[test]
    fn sets_without_touching_saved_settings_on_error() {
        let (mut link, found) = sim_device("mx-master-3s", 0);
        assert!(set_on(&mut link, &found.info, "wheel-of-fortune", "1").is_err());
        assert!(set_on(&mut link, &found.info, "invert-thumb", "maybe").is_err());
    }

    #[test]
    fn numbers_and_flags() {
        assert_eq!(number::<u16>("2400.0").unwrap(), 2400);
        assert!(number::<u8>("300").is_err());
        assert!(flag("true").unwrap() && !flag("false").unwrap());
        assert!(flag("yes").is_err());
    }
}
