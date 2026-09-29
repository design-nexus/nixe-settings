use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::prelude::*;

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
    for (when, title) in [("ac", "When plugged in"), ("battery", "On battery")] {
        let (r, _) = widgets::choice_row(title, "Switched to automatically when power changes.", options.clone(), "", move |p| {
            cmd::run_async(&["omarchy-powerprofiles-set", when, &p], |r| {
                if let Err(e) = r {
                    window::toast(&format!("{e}"));
                }
            });
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
        }
    }

    // ----- Lid & buttons -----
    let g = page.group("Session");
    let buttons = widgets::hbox(8);
    buttons.append(&widgets::command_button("Power menu", &["omarchy-shell", "shell", "toggle", "omarchy.power"]));
    g.add(&widgets::row("Shut down, restart, sleep", "", Some(buttons.upcast_ref())));
}
