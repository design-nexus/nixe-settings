//! Nixe apps: the look every app in the suite shares, and installing, updating
//! and removing the apps themselves.

use crate::widgets::{self, Page};
use crate::{cmd, paths};
use gtk::prelude::*;
use nixe_theme::ThemeMode;

/// Each app: what `nixe` calls it, its name, and the program that shows it's installed.
const APPS: &[(&str, &str, &str, &str)] = &[
    ("settings", "Settings", "nixe-settings", "A control panel for Omarchy"),
    ("contacts", "Contacts", "nixe-contacts", "Contacts, favorites and lists"),
    ("tasks", "Tasks", "nixe-tasks", "Processes, usage and startup apps"),
    ("torrents", "Torrents", "nixe-torrents", "A BitTorrent client"),
    ("pdf", "PDF", "nixe-pdf", "Read, fill in and mark up PDFs"),
    ("notepad", "Notepad", "nixe-notepad", "Plain text, with tabs"),
    ("music", "Music", "nixe-music", "Your music library, radio and podcasts"),
    ("media-player", "Media Player", "nixe-media-player", "Movies and TV shows"),
    ("chat", "Chat", "nixe-chat", "Chat with models on Ollama"),
    ("fonts", "Fonts", "nixe-fonts", "Google Fonts and Nerd Fonts"),
    ("archive", "Archive", "nixe-archive", "Open and make archives"),
    ("office", "Office", "nixe-office", "Write, Sheets and Present"),
    ("photos", "Photos", "nixe-photos", "A photo gallery and editor"),
];

fn installed(program: &str) -> bool {
    paths::home().join(".local/bin").join(program).is_file()
}

/// Run `nixe …` in a terminal: installing can ask for a password and takes a while.
fn in_terminal(args: &str) {
    let script = format!("nixe {args}; echo; read -rp 'Done. Press Enter to close.'");
    cmd::spawn(&["xdg-terminal-exec", "bash", "-lc", &script]);
}

pub fn build(page: &Page) {
    let suite = nixe_theme::read_suite().unwrap_or_default();

    let g = page.group("Look");
    g.note("Every Nixe app uses these, and follows a change within a second.");
    let follow = suite.mode == ThemeMode::Omarchy;
    let themes: Vec<(String, String)> = nixe_theme::all().into_iter().map(|t| (t.id, t.name)).collect();
    let (theme_row, theme_dd) = widgets::choice_row(
        "Theme",
        "The colors to use when not following Omarchy.",
        themes,
        &suite.theme,
        |id| {
            nixe_theme::update_suite(|s| {
                s.mode = ThemeMode::Theme;
                s.theme = id;
            })
        },
    );
    theme_dd.set_sensitive(!follow || !nixe_theme::omarchy_available());
    if nixe_theme::omarchy_available() {
        let dd = theme_dd.clone();
        let (r, _) = widgets::switch_row(
            "Follow Omarchy theme",
            "Use the Omarchy theme's colors, and change with it.",
            follow,
            move |on| {
                dd.set_sensitive(!on);
                nixe_theme::update_suite(|s| s.mode = if on { ThemeMode::Omarchy } else { ThemeMode::Theme });
            },
        );
        g.add(&r);
    }
    g.add(&theme_row);
    let density = nixe_ui::widgets::segmented(
        &[("comfortable".into(), "Comfortable".into()), ("compact".into(), "Compact".into())],
        &suite.density,
        |d| nixe_theme::update_suite(|s| s.density = d),
    );
    g.add(&widgets::row("Density", "Compact fits more rows on screen.", Some(density.upcast_ref())));
    let (r, _) = widgets::slider_row(
        "Text size",
        "Larger or smaller text in every Nixe app.",
        (90.0, 125.0, 5.0),
        (suite.text_scale * 100.0).round(),
        0,
        " %",
        |v| nixe_theme::update_suite(|s| s.text_scale = (v / 5.0).round() * 5.0 / 100.0),
    );
    g.add(&r);
    widgets::keywords("nixe theme density compact text size font scale suite");
    g.add(&nixe_ui::widgets::start_row());
    widgets::keywords("open on start page last page home remember");

    let g = page.group("Apps");
    g.note("Installing builds the app from source, which takes a few minutes the first time.");
    for (id, name, program, desc) in APPS {
        let here = installed(program);
        let control = widgets::hbox(6);
        if here {
            control.append(&widgets::tag("Installed"));
            let remove = gtk::Button::with_label("Remove");
            remove.add_css_class("destructive-action");
            let id = *id;
            remove.connect_clicked(move |_| in_terminal(&format!("uninstall {id}")));
            control.append(&remove);
        } else {
            let add = gtk::Button::with_label("Install");
            let id = *id;
            add.connect_clicked(move |_| in_terminal(&format!("install {id}")));
            control.append(&add);
        }
        g.add(&widgets::row(name, desc, Some(control.upcast_ref())));
    }
    let (r, _) = widgets::button_row(
        "Update every app",
        "Rebuild the installed apps from the latest source.",
        "Update",
        |_| in_terminal("update"),
    );
    g.add(&r);
}
