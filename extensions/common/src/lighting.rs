//! The lighting rows for one OpenRGB device, and what a change to them does.
//! Shared by the Corsair and RGB extensions.
//!
//! Row keys: `light/effect`, `light/colour`, `light/brightness`,
//! `light/speed`, `light/theme`, `light/keep`, plus `openrgb/install` and
//! `openrgb/start` while OpenRGB is missing or stopped.

use crate::openrgb::{self, COLOR_MODE_PER_LED, COLOR_MODE_SPECIFIC, Client, Device, HAS_BRIGHTNESS, HAS_SPEED, Rgb};
use crate::page::{self, desc, refreshes};
use crate::sys;
use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Whether OpenRGB is installed, and running.
pub fn status() -> (bool, bool) {
    let installed = sys::present("openrgb");
    let running = installed && Client::connect().is_ok();
    (installed, running)
}

/// Rows offering to install or start OpenRGB; empty when it's running.
pub fn setup_rows() -> Vec<Value> {
    match status() {
        (false, _) => vec![desc(
            page::button("openrgb/install", "Install OpenRGB", "Install…"),
            "OpenRGB runs the lighting. Installs the <tt>openrgb</tt> package in a terminal.",
        )],
        (true, false) => vec![desc(
            page::button("openrgb/start", "Start OpenRGB", "Start"),
            "Runs OpenRGB in the background (a user service), now and at every sign-in.",
        )],
        _ => Vec::new(),
    }
}

/// Every device OpenRGB sees, or nothing if it isn't running.
pub fn devices() -> Vec<Device> {
    Client::connect().and_then(|mut c| c.devices()).unwrap_or_default()
}

/// The devices "follow the theme colour" is on for, by id.
fn following() -> Vec<String> {
    sys::read(sys::state_dir().join("follow-theme")).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

fn set_following(id: &str, on: bool) -> Result<()> {
    let mut list = following();
    list.retain(|x| x != id);
    if on {
        list.push(id.to_string());
    }
    std::fs::write(sys::state_dir().join("follow-theme"), list.join("\n"))?;
    Ok(())
}

/// The lighting rows for `d`.
pub fn rows(d: &Device) -> Vec<Value> {
    let mut rows = Vec::new();
    if d.modes.is_empty() {
        return rows;
    }
    let opts: Vec<(String, String)> = d.modes.iter().enumerate().map(|(i, m)| (i.to_string(), m.name.clone())).collect();
    rows.push(refreshes(page::choice_owned("light/effect", "Effect", &d.active_mode.to_string(), &opts)));
    let theme = sys::theme_accent();
    let follow = following().contains(&d.id());
    if let Some(m) = d.mode() {
        let current = if m.color_mode == COLOR_MODE_SPECIFIC {
            m.colors.first().copied()
        } else if m.color_mode == COLOR_MODE_PER_LED {
            d.colors.first().copied()
        } else {
            None
        };
        if let Some(c) = current {
            rows.push(page::colour_with_theme("light/colour", "Colour", &c.hex(), theme.as_deref()));
        }
        if m.has(HAS_BRIGHTNESS) && m.brightness_max > m.brightness_min {
            rows.push(page::slider("light/brightness", "Brightness", m.brightness as f64, (m.brightness_min as f64, m.brightness_max as f64, 1.0), ""));
        }
        if m.has(HAS_SPEED) && m.speed_max != m.speed_min {
            let (lo, hi) = (m.speed_min.min(m.speed_max) as f64, m.speed_min.max(m.speed_max) as f64);
            rows.push(page::slider("light/speed", "Speed", m.speed as f64, (lo, hi, 1.0), ""));
        }
    }
    if theme.is_some() {
        rows.push(desc(page::switch("light/theme", "Follow the theme colour", follow), "Takes the Omarchy theme's accent colour, and changes with the theme."));
    }
    rows.push(desc(
        page::button("light/keep", "Keep after restarting", "Save to the device"),
        "Stores this effect on the device itself, where it supports that, so it stays without OpenRGB.",
    ));
    rows
}

fn open_terminal(command: &str) -> Result<()> {
    std::process::Command::new("xdg-terminal-exec")
        .args(["bash", "-c", &format!("{command}; echo; read -rp 'Press Enter to close'")])
        .spawn()
        .context("couldn't open a terminal")?;
    Ok(())
}

/// Handle a change to one of the rows above (or the setup buttons) for the
/// device with id `device`. `None` when the key isn't a lighting one.
pub fn set(device: &str, key: &str, value: &str) -> Result<Option<Option<Value>>> {
    match key {
        "openrgb/install" => {
            open_terminal("sudo pacman -S --needed openrgb")?;
            return Ok(Some(crate::toast("Installing OpenRGB in a terminal. Come back when it's done.")));
        }
        "openrgb/start" => {
            openrgb::start_server()?;
            // The server takes a moment to find devices.
            std::thread::sleep(std::time::Duration::from_secs(2));
            return Ok(Some(crate::reload("OpenRGB started")));
        }
        k if !k.starts_with("light/") => return Ok(None),
        _ => {}
    }
    let mut c = Client::connect()?;
    let Some(d) = c.devices()?.into_iter().find(|d| d.id() == device) else { bail!("OpenRGB doesn't see that device any more") };
    let idx = d.active_mode;
    let mode = d.mode().cloned();
    match key {
        "light/effect" => {
            let i: usize = value.parse()?;
            let m = d.modes.get(i).context("no such effect")?;
            c.set_mode(d.index, i as i32, m)?;
            Ok(Some(crate::refresh()))
        }
        "light/colour" => {
            let colour = Rgb::parse(value).context("not a colour")?;
            set_following(&d.id(), false)?;
            apply_colour(&mut c, &d, colour)?;
            Ok(Some(None))
        }
        "light/brightness" | "light/speed" => {
            let mut m = mode.context("no effect is on")?;
            let v: f64 = value.parse()?;
            if key == "light/brightness" {
                m.brightness = v.round() as u32;
            } else {
                m.speed = v.round() as u32;
            }
            c.set_mode(d.index, idx, &m)?;
            Ok(Some(None))
        }
        "light/theme" => {
            let on = value == "true";
            set_following(&d.id(), on)?;
            if on && let Some(colour) = sys::theme_accent().and_then(|t| Rgb::parse(&t)) {
                apply_colour(&mut c, &d, colour)?;
            }
            Ok(Some(crate::refresh()))
        }
        "light/keep" => {
            let m = mode.context("no effect is on")?;
            c.save_mode(d.index, idx, &m)?;
            Ok(Some(crate::toast("Saved on the device")))
        }
        _ => Ok(None),
    }
}

/// A colour for the current effect if it takes one, else a still colour.
fn apply_colour(c: &mut Client, d: &Device, colour: Rgb) -> Result<()> {
    match d.mode() {
        Some(m) if m.color_mode == COLOR_MODE_SPECIFIC => {
            let mut m = m.clone();
            let n = m.colors.len().max(m.colors_min as usize).max(1);
            m.colors = vec![colour; n];
            c.set_mode(d.index, d.active_mode, &m)
        }
        Some(m) if m.color_mode == COLOR_MODE_PER_LED => c.set_leds(d.index, &vec![colour; d.leds.len().max(1)]),
        _ => openrgb::set_colour(c, d, colour),
    }
}

/// After a theme change: every device following the theme takes its colour.
pub fn theme_changed() -> Result<()> {
    let ids = following();
    if ids.is_empty() {
        return Ok(());
    }
    let Some(colour) = sys::theme_accent().and_then(|t| Rgb::parse(&t)) else { return Ok(()) };
    let mut c = Client::connect()?;
    for d in c.devices()? {
        if ids.contains(&d.id()) {
            apply_colour(&mut c, &d, colour)?;
        }
    }
    Ok(())
}
