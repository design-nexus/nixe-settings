//! Theming: the shared Nixe theme engine, given this app's stylesheet.

pub use nixe_theme::*;

use crate::prefs;

/// Install the stylesheet and the current palette, and follow Omarchy live.
pub fn install() {
    if let Some(display) = gtk::gdk::Display::default() {
        install_icons(&display);
    }
    nixe_theme::install(Style {
        window_class: "settings-window",
        base_skip: &[
            ".settings-option",
            ".settings-dialog.narrow .settings-dialog-main .settings-page",
            ".settings-dialog-main .settings-page",
            ".settings-dialog-main .settings-page.narrow",
        ],
        css: include_str!("style.css"),
        no_glow_css: "\n.theme-card.selected, .toast { box-shadow: 0 0 0 1px alpha(@theme_accent, 0.6); }\n\
             window.settings-window scale > trough > slider, window.settings-window .fader-scale > trough > slider \
             { box-shadow: 0 0 0 1px alpha(@theme_accent, 0.72); }\n",
        extra_colors: None,
        prefs: || {
            let p = prefs::get();
            ThemePrefs { mode: p.mode, theme: p.theme, glow: p.glow, reduce_motion: p.reduce_motion }
        },
    });
}

/// Icons the icon themes don't have, shipped inside the program.
const ICONS: &[(&str, &str)] = &[("settings-shortcuts-symbolic", include_str!("../data/icons/settings-shortcuts-symbolic.svg"))];

/// Write the built-in icons where GTK's icon lookup finds them.
fn install_icons(display: &gtk::gdk::Display) {
    let dir = crate::paths::cache_dir().join("icons");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, svg) in ICONS {
        let path = dir.join(format!("{name}.svg"));
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*svg) {
            let _ = std::fs::write(&path, svg);
        }
    }
    gtk::IconTheme::for_display(display).add_search_path(&dir);
}
