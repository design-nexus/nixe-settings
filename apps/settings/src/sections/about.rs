use crate::backend::updates;
use crate::widgets::{self, Page};
use crate::{cmd, prefs, units, window};
use gtk::prelude::*;
use std::cell::RefCell;

thread_local! {
    /// The version just installed, until Settings restarts into it.
    static INSTALLED: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn checked_text(s: &updates::Status) -> String {
    if s.checked == 0 {
        return "Not checked yet.".into();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("Checked {}.", units::ago(now.saturating_sub(s.checked) as i64))
}

fn check_now(b: &gtk::Button) {
    b.set_sensitive(false);
    b.set_label("Checking…");
    cmd::background(
        || updates::check(true),
        |s| {
            if !s.settings_update() && s.extensions.is_empty() {
                window::toast("Everything is up to date");
            }
            window::after_update_check(s);
        },
    );
}

/// Settings' own version and updates, and extension updates.
fn settings_group(page: &Page) {
    let g = page.group("Settings");
    let s = updates::cached();
    let current = env!("CARGO_PKG_VERSION");
    if let Some(v) = INSTALLED.with(|i| i.borrow().clone()) {
        let (r, _) = widgets::button_row(
            &format!("Settings {v} is installed"),
            "Restart Settings to start using it.",
            "Restart Settings",
            |_| window::restart("about"),
        );
        g.add(&r);
    } else if s.settings_update() {
        let line = widgets::hbox(8);
        if !s.url.is_empty() {
            let notes = gtk::Button::with_label("What's new");
            let url = s.url.clone();
            notes.connect_clicked(move |_| cmd::spawn(&["xdg-open", &url]));
            line.append(&notes);
        }
        let update = gtk::Button::with_label("Update");
        update.add_css_class("suggested-action");
        let can = updates::self_updatable();
        update.set_sensitive(can);
        let latest = s.latest.clone();
        update.connect_clicked(move |b| {
            b.set_sensitive(false);
            b.set_label("Updating…");
            let (b, latest) = (b.clone(), latest.clone());
            cmd::background(
                || updates::update_settings().map_err(|e| format!("{e:#}")),
                move |r| match r {
                    Ok(()) => {
                        INSTALLED.with(|i| *i.borrow_mut() = Some(latest.clone()));
                        window::toast(&format!("Settings {latest} is installed. Restart to use it."));
                        window::rebuild("about");
                    }
                    Err(e) => {
                        b.set_sensitive(true);
                        b.set_label("Update");
                        window::toast(&format!("Couldn't update: {e}"));
                    }
                },
            );
        });
        line.append(&update);
        let desc = if can {
            format!("You have {current}. Updating takes a few seconds; then restart Settings.")
        } else {
            format!("You have {current}, a development build: update it from its source.")
        };
        g.add(&widgets::row(&format!("Settings {} is available", s.latest), &desc, Some(line.upcast_ref())));
    } else {
        let (r, _) = widgets::button_row(&format!("Settings {current}"), &format!("Up to date. {}", checked_text(&s)), "Check now", check_now);
        g.add(&r);
    }
    widgets::keywords("update upgrade version new release settings app check");

    // Only extensions still installed (the list may predate a removal).
    let names: Vec<String> = crate::ext::installed()
        .into_iter()
        .filter(|e| s.extensions.iter().any(|id| id == e.id()))
        .map(|e| e.manifest.name)
        .collect();
    if !names.is_empty() {
        let (r, _) = widgets::button_row(
            &format!("Extension updates ({})", names.len()),
            &glib_escape(&names.join(", ")),
            "Update all",
            |b| {
                b.set_sensitive(false);
                b.set_label("Updating…");
                let b = b.clone();
                cmd::background(
                    || updates::update_extensions().map_err(|e| format!("{e:#}")),
                    move |r| {
                        b.set_sensitive(true);
                        match r {
                            Ok(n) => window::toast(&format!("Updated {}", n.join(", "))),
                            Err(e) => window::toast(&format!("Couldn't update: {e}")),
                        }
                        window::reload_sections(false);
                        window::rebuild("about");
                        window::rebuild_if_built("extensions");
                    },
                );
            },
        );
        g.add(&r);
    }
    let (r, _) = widgets::switch_row(
        "Install extension updates automatically",
        "Settings looks for updates once a day when it opens. Settings itself always asks first.",
        prefs::get().auto_update_extensions,
        |on| prefs::update(|p| p.auto_update_extensions = on),
    );
    widgets::keywords("automatic auto update extensions");
    g.add(&r);
}

fn glib_escape(t: &str) -> String {
    gtk::glib::markup_escape_text(t).to_string()
}

fn cpu() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| {
            t.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
        })
        .unwrap_or_default()
}

fn memory() -> String {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| {
            let kb: f64 = t.lines().find(|l| l.starts_with("MemTotal"))?.split_whitespace().nth(1)?.parse().ok()?;
            Some(format!("{:.0} GB", kb / 1024.0 / 1024.0))
        })
        .unwrap_or_default()
}

fn model() -> String {
    let read = |f: &str| std::fs::read_to_string(format!("/sys/class/dmi/id/{f}")).unwrap_or_default().trim().to_string();
    format!("{} {}", read("sys_vendor"), read("product_name")).trim().to_string()
}

pub fn build(page: &Page) {
    settings_group(page);
    let g = page.group("Omarchy");
    let (r, _) = widgets::info_row("Version", &cmd::output(&["omarchy-version"]).unwrap_or_default());
    g.add(&r);
    if let Some(ch) = cmd::output(&["omarchy-version-channel"]) {
        let (r, _) = widgets::info_row("Channel", &ch);
        g.add(&r);
    }
    let (r, _) = widgets::button_row(
        "Update",
        "Update Omarchy and all system packages. Opens a terminal to show progress.",
        "Update now",
        |_| cmd::spawn(&["omarchy-launch-floating-terminal-with-presentation", "omarchy-update"]),
    );
    widgets::keywords("upgrade pacman packages");
    g.add(&r);
    let (r, _) = widgets::button_row(
        "Snapshot",
        "Save the system as it is now, so you can roll back after a bad update.",
        "Create snapshot",
        |b| {
            b.set_sensitive(false);
            let b = b.clone();
            cmd::run_async(&["omarchy-snapshot", "create"], move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(_) => window::toast("Snapshot created"),
                    Err(e) => window::toast(&format!("{e}")),
                }
            });
        },
    );
    widgets::keywords("backup restore snapper rollback");
    g.add(&r);

    let g = page.group("This computer");
    for (title, value) in [
        ("Model", model()),
        ("Processor", cpu()),
        ("Memory", memory()),
        ("Kernel", cmd::output(&["uname", "-r"]).unwrap_or_default()),
        (
            "Hyprland",
            cmd::output(&["hyprctl", "version", "-j"])
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("tag").and_then(|x| x.as_str()).map(String::from))
                .unwrap_or_default(),
        ),
    ] {
        if !value.is_empty() {
            let (r, _) = widgets::info_row(title, &value);
            g.add(&r);
        }
    }
}
