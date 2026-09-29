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

/// The Status and Time remaining lines, from `omarchy-battery-status --shell`
/// (tab-separated fields). Empty fields are left out.
fn battery_lines(shell: &str) -> (String, String) {
    let get = |k: &str| {
        shell.lines().find_map(|l| l.split_once('\t').filter(|(key, _)| *key == k).map(|(_, v)| v.trim().to_string())).unwrap_or_default()
    };
    let (state, time, rate, size) = (get("state"), get("time"), get("rate"), get("size"));
    let word = match state.as_str() {
        "charging" => "Charging".to_string(),
        "discharging" => "On battery".to_string(),
        "fully-charged" => "Fully charged".to_string(),
        "holding" => match get("threshold") {
            t if t.is_empty() => "Holding charge".to_string(),
            t => format!("Holding at {t}"),
        },
        "pending-charge" => "Plugged in, not charging".to_string(),
        _ => String::new(),
    };
    let drawing = rate.trim_end_matches('W').parse::<f64>().is_ok_and(|r| r > 0.0);
    let draw = match (drawing, size.is_empty()) {
        (true, false) => format!("{rate} / {size}"),
        (true, true) => rate,
        (false, _) => size,
    };
    let status = [get("percentage"), word, draw].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("  ·  ");
    let remaining = match state.as_str() {
        "discharging" | "charging" if time.is_empty() => "Estimating…".to_string(),
        "discharging" => format!("{time} left"),
        "charging" => format!("{time} to full"),
        "" => String::new(),
        _ => "Plugged in".to_string(),
    };
    (status, remaining)
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
        let read = || {
            let (status, time) = battery_lines(&cmd::output(&["omarchy-battery-status", "--shell"]).unwrap_or_default());
            let or_unknown = |s: String| if s.is_empty() { "Unknown".to_string() } else { s };
            (or_unknown(status), or_unknown(time))
        };
        let (status, time) = read();
        let (r, label) = widgets::info_row("Status", &status);
        label.set_wrap(false);
        g.add(&r);
        let (r, time_label) = widgets::info_row("Time remaining", &time);
        time_label.set_wrap(false);
        g.add(&r);
        // Keep it current while the page exists.
        let (weak, weak_time) = (label.downgrade(), time_label.downgrade());
        gtk::glib::timeout_add_seconds_local(10, move || match (weak.upgrade(), weak_time.upgrade()) {
            (Some(l), Some(t)) => {
                if l.is_mapped() {
                    let (status, time) = read();
                    l.set_text(&status);
                    t.set_text(&time);
                }
                gtk::glib::ControlFlow::Continue
            }
            _ => gtk::glib::ControlFlow::Break,
        });
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
    let now = widgets::hbox(8);
    now.append(&widgets::command_button("Lock", &["omarchy-system-lock"]));
    // Omarchy hides these when suspend is switched off or hibernation isn't set up.
    if cmd::run(&["omarchy-toggle-enabled", "suspend-off"]).is_err() {
        now.append(&widgets::command_button("Suspend", &["systemctl", "suspend"]));
    }
    if cmd::run(&["omarchy-hibernation-available"]).is_ok() {
        now.append(&widgets::command_button("Hibernate", &["systemctl", "hibernate"]));
    }
    g.add(&widgets::row("Lock or sleep", "", Some(now.upcast_ref())));
    let end = widgets::hbox(8);
    for (label, confirm, command) in [
        ("Log out", "Click again to log out", "omarchy-system-logout"),
        ("Restart", "Click again to restart", "omarchy-system-reboot"),
        ("Shut down", "Click again to shut down", "omarchy-system-shutdown"),
    ] {
        let b = widgets::confirm_button(label, confirm, move |_| cmd::spawn(&[command]));
        b.add_css_class("destructive-action");
        end.append(&b);
    }
    g.add(&widgets::row("End the session", "Open apps are closed. Each needs a second click.", Some(end.upcast_ref())));
    widgets::keywords("shut down shutdown power off restart reboot log out logout suspend sleep hibernate lock");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_lines_skip_empty_parts() {
        let full = "percentage\t100%\nstate\tfully-charged\nrate\t0W\nsize\t90Wh\ntime\t\ncycles\t0\nthreshold\t75-80%\n";
        assert_eq!(battery_lines(full), ("100%  ·  Fully charged  ·  90Wh".into(), "Plugged in".into()));
        let on_battery = "percentage\t64%\nstate\tdischarging\nrate\t12.3W\nsize\t90Wh\ntime\t3h 5m\n";
        assert_eq!(battery_lines(on_battery), ("64%  ·  On battery  ·  12.3W / 90Wh".into(), "3h 5m left".into()));
        let charging = "percentage\t40%\nstate\tcharging\nrate\t45W\nsize\t90Wh\ntime\t\n";
        assert_eq!(battery_lines(charging).1, "Estimating…");
        let holding = "percentage\t80%\nstate\tholding\nrate\t0W\nsize\t90Wh\ntime\t\nthreshold\t75-80%\n";
        assert_eq!(battery_lines(holding).0, "80%  ·  Holding at 75-80%  ·  90Wh");
        assert_eq!(battery_lines(""), (String::new(), String::new()));
    }

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
