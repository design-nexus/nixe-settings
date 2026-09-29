use crate::backend::shell;
use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::prelude::*;
use serde_json::json;

fn minutes_label(secs: i64) -> String {
    if secs <= 0 { "Never".into() } else { format!("{} min", secs / 60) }
}

fn idle_row(key: &'static str, title: &str, desc: &str, default: i64) -> gtk::Box {
    let secs = shell::get(&["idle", key]).and_then(|v| v.as_i64()).unwrap_or(default);
    let (r, s) = widgets::slider_row(title, desc, (0.0, 60.0, 1.0), (secs / 60) as f64, 0, " min", move |v| {
        let secs = (v.round() as i64) * 60;
        if let Err(e) = shell::set(&["idle", key], json!(secs)) {
            window::toast(&format!("Couldn't save: {e}"));
        }
    });
    // Show "Never" at zero.
    let readout = s.readout.clone();
    readout.set_text(&minutes_label(secs));
    s.scale.connect_value_changed(move |sc| readout.set_text(&minutes_label(sc.value().round() as i64 * 60)));
    r
}

pub fn build(page: &Page) {
    let g = page.group("When idle");
    g.add(&idle_row("screensaver", "Start the screensaver after", "0 turns the screensaver off.", 150));
    g.add(&idle_row("lock", "Lock the screen after", "0 never locks on its own.", 300));
    widgets::keywords("timeout minutes");

    let g = page.group("Right now");
    let awake = cmd::output(&["omarchy-toggle-idle", "status"])
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("enabled").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    let (r, _) = widgets::switch_row(
        "Stay awake",
        "Keep the screen on and unlocked until you turn this off — for presentations and long downloads.",
        awake,
        |on| cmd::spawn(&["omarchy-toggle-idle", if on { "stay-awake" } else { "allow-idle" }]),
    );
    widgets::keywords("caffeine inhibit presentation");
    g.add(&r);

    let g = page.group("Lock & sleep");
    let (r, _) = widgets::switch_row(
        "Screensaver",
        "Show Omarchy's screensaver before locking.",
        !shell::toggle_on("screensaver-off"),
        |_| cmd::spawn(&["omarchy-toggle-screensaver"]),
    );
    g.add(&r);
    let (r, _) = widgets::switch_row("Suspend in the system menu", "", !shell::toggle_on("suspend-off"), |_| {
        cmd::spawn(&["omarchy-toggle-suspend"])
    });
    g.add(&r);
    let (r, _) = widgets::button_row("Lock now", "", "Lock", |_| cmd::spawn(&["omarchy-system-lock"]));
    g.add(&r);
}
