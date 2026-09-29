use crate::widgets::{self, Page};
use crate::{cmd, paths, window};
use gtk::prelude::*;

fn state_file(source: &str) -> std::path::PathBuf {
    std::env::var_os("OMARCHY_POWERPROFILES_STATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths::state_home().join("omarchy/powerprofiles"))
        .join(source)
}

/// What Omarchy has saved for "ac" or "battery", if anything.
fn read_saved(source: &str) -> Option<String> {
    std::fs::read_to_string(state_file(source)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// The profile Omarchy will use for a power source: what's saved, else its own default
/// (Performance on AC when available, otherwise Balanced).
fn effective_profile(saved: Option<&str>, source: &str, profiles: &[String]) -> String {
    if let Some(s) = saved.filter(|s| profiles.iter().any(|p| p == s)) {
        return s.to_string();
    }
    if source == "ac" && profiles.iter().any(|p| p == "performance") { "performance".into() } else { "balanced".into() }
}

fn on_battery() -> bool {
    cmd::output(&[
        "busctl",
        "get-property",
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
        "OnBattery",
    ])
    .is_some_and(|s| s.trim() == "b true")
}

/// Save the profile for a power source. Omarchy's own command also switches to it right away, which
/// is only wanted when that source is the one in use, so for the other source just record the choice.
fn remember(source: &str, profile: &str, in_use: bool) -> anyhow::Result<()> {
    if in_use {
        cmd::run(&["omarchy-powerprofiles-set", source, profile])?;
        return Ok(());
    }
    cmd::atomic_write(&state_file(source), &format!("{profile}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<String> {
        ["power-saver", "balanced", "performance"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn uses_the_saved_profile() {
        assert_eq!(effective_profile(Some("power-saver"), "battery", &all()), "power-saver");
        assert_eq!(effective_profile(Some("balanced"), "ac", &all()), "balanced");
    }

    #[test]
    fn falls_back_like_omarchy() {
        assert_eq!(effective_profile(None, "ac", &all()), "performance");
        assert_eq!(effective_profile(None, "battery", &all()), "balanced");
        let no_perf: Vec<String> = vec!["power-saver".into(), "balanced".into()];
        assert_eq!(effective_profile(None, "ac", &no_perf), "balanced");
        // A saved profile this machine doesn't offer is ignored.
        assert_eq!(effective_profile(Some("performance"), "ac", &no_perf), "balanced");
    }

    #[test]
    fn records_the_other_source_without_running_omarchy() {
        let dir = std::env::temp_dir().join(format!("settings-power-test-{}", std::process::id()));
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var("OMARCHY_POWERPROFILES_STATE_DIR", &dir) };
        remember("battery", "power-saver", false).unwrap();
        assert_eq!(read_saved("battery").as_deref(), Some("power-saver"));
        assert!(read_saved("ac").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::remove_var("OMARCHY_POWERPROFILES_STATE_DIR") };
    }
}

pub fn build(page: &Page) {
    // ----- Profile -----
    let g = page.group("Power profile");
    let profiles: Vec<String> = cmd::output(&["omarchy-powerprofiles-list"])
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let current = cmd::output(&["powerprofilesctl", "get"]).unwrap_or_default();
    let label = |p: &str| match p {
        "power-saver" => "Power saver".to_string(),
        "balanced" => "Balanced".to_string(),
        "performance" => "Performance".to_string(),
        other => other.to_string(),
    };
    let options: Vec<(String, String)> = profiles.iter().map(|p| (p.clone(), label(p))).collect();
    let (r, _) = widgets::choice_row("Right now", "", options.clone(), &current, |p| {
        cmd::run_async(&["powerprofilesctl", "set", &p], |r| {
            if let Err(e) = r {
                window::toast(&format!("{e}"));
            }
        });
    });
    g.add(&r);
    let on_battery = on_battery();
    for (when, title) in [("ac", "When plugged in"), ("battery", "On battery")] {
        let in_use = (when == "battery") == on_battery;
        let desc = if in_use {
            "Switched to automatically when power changes. This is the one in use now."
        } else {
            "Switched to automatically when power changes."
        };
        let saved = effective_profile(read_saved(when).as_deref(), when, &profiles);
        let (r, _) = widgets::choice_row(title, desc, options.clone(), &saved, move |p| {
            cmd::background(
                move || remember(when, &p, in_use).map_err(|e| format!("{e:#}")),
                move |r| match r {
                    // The profile in use changed, so "Right now" is stale.
                    Ok(()) if in_use => window::rebuild("power"),
                    Ok(()) => {}
                    Err(e) => window::toast(&e),
                },
            );
        });
        g.add(&r);
    }

    // ----- Battery -----
    if cmd::output(&["omarchy-battery-present"]).is_some() || std::path::Path::new("/sys/class/power_supply/BAT0").exists() {
        let g = page.group("Battery");
        let status = cmd::output(&["omarchy-battery-status"]).unwrap_or_else(|| "Unknown".into());
        let (r, _) = widgets::info_row("Status", status.lines().next().unwrap_or(""));
        g.add(&r);
        let (r, _) = widgets::button_row("Time remaining", "", "Show", |_| cmd::spawn(&["omarchy-notification-battery"]));
        g.add(&r);
    }

    // ----- Brightness -----
    let g = page.group("Brightness");
    if let Some(b) = cmd::output(&["omarchy-brightness-display", "--no-osd"]).and_then(|s| s.trim().parse::<f64>().ok()) {
        let (r, _) = widgets::slider_row("Display", "The focused screen.", (1.0, 100.0, 1.0), b, 0, "%", |v| {
            cmd::spawn(&["omarchy-brightness-display", "--no-osd", &format!("{}%", v.round() as i64)]);
        });
        g.add(&r);
    }
    if cmd::present("brightnessctl") {
        let kbd = cmd::output(&["brightnessctl", "-l", "-c", "leds", "-m"]).unwrap_or_default();
        if let Some(line) = kbd.lines().find(|l| l.contains("kbd_backlight")) {
            let parts: Vec<&str> = line.split(',').collect();
            let dev = parts.first().unwrap_or(&"").to_string();
            let max: f64 = parts.get(4).and_then(|m| m.parse().ok()).unwrap_or(3.0);
            let cur: f64 = parts.get(2).and_then(|m| m.parse().ok()).unwrap_or(0.0);
            let (r, _) = widgets::slider_row("Keyboard backlight", "", (0.0, max, 1.0), cur, 0, "", move |v| {
                cmd::spawn(&["brightnessctl", "-d", &dev, "set", &(v.round() as i64).to_string()]);
            });
            widgets::keywords("keyboard light backlit");
            g.add(&r);
            if let Some(t) = crate::sections::aura::timeout_row() {
                g.add(&t);
            }
        }
    }

    // ----- Lid & buttons -----
    let g = page.group("Session");
    let buttons = widgets::hbox(8);
    buttons.append(&widgets::command_button("Power menu", &["omarchy-shell", "shell", "toggle", "omarchy.power"]));
    g.add(&widgets::row("Shut down, restart, sleep", "", Some(buttons.upcast_ref())));
}
