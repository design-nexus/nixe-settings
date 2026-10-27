//! Theming: the shared Nyx theme engine, given this app's stylesheet.

pub use nyx_theme::*;

use crate::prefs;

/// Install the stylesheet and the current palette, and follow Omarchy live.
pub fn install() {
    if let Some(display) = gtk::gdk::Display::default() {
        install_icons(&display);
    }
    nyx_theme::use_design_2();
    nyx_theme::install(Style {
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
        set_theme: Some(|mode, theme| {
            prefs::update(|p| {
                p.mode = mode;
                p.theme = theme;
            })
        }),
    });
}

/// The palette on screen as libadwaita colours, for the GOA sign-in dialogs
/// that `nyx-goa-helper` shows in its own process. Loaded above the user's
/// `gtk.css`, which carries Omarchy's colours.
pub fn adwaita_css(p: &Palette, look: &Look) -> String {
    let shade = |a: f64| {
        let s = Rgb::hex(&p.shadow);
        format!("rgba({}, {}, {}, {a})", (s.0 * 255.0).round(), (s.1 * 255.0).round(), (s.2 * 255.0).round())
    };
    let on_accent = if p.light { "#ffffff".to_string() } else { p.bg.clone() };
    let colors: Vec<(&str, String)> = vec![
        ("accent_bg_color", p.accent.clone()),
        ("accent_color", p.accent.clone()),
        ("accent_fg_color", on_accent.clone()),
        ("destructive_bg_color", p.danger.clone()),
        ("destructive_color", p.danger.clone()),
        ("destructive_fg_color", on_accent.clone()),
        ("error_bg_color", p.danger.clone()),
        ("error_color", p.danger.clone()),
        ("error_fg_color", on_accent.clone()),
        ("success_bg_color", look.success.clone()),
        ("success_color", look.success.clone()),
        ("success_fg_color", on_accent.clone()),
        ("warning_bg_color", look.warning.clone()),
        ("warning_color", look.warning.clone()),
        ("warning_fg_color", on_accent),
        ("window_bg_color", p.bg.clone()),
        ("window_fg_color", p.text.clone()),
        ("view_bg_color", look.card.clone()),
        ("view_fg_color", p.text.clone()),
        ("headerbar_bg_color", p.bg.clone()),
        ("headerbar_fg_color", p.text.clone()),
        ("headerbar_border_color", p.border.clone()),
        ("headerbar_backdrop_color", p.bg.clone()),
        ("headerbar_shade_color", shade(0.4)),
        ("headerbar_darker_shade_color", shade(0.8)),
        ("sidebar_bg_color", look.panel.clone()),
        ("sidebar_fg_color", p.text.clone()),
        ("sidebar_backdrop_color", look.panel.clone()),
        ("sidebar_border_color", shade(0.4)),
        ("sidebar_shade_color", shade(0.35)),
        ("card_bg_color", look.card.clone()),
        ("card_fg_color", p.text.clone()),
        ("card_shade_color", shade(0.4)),
        ("dialog_bg_color", p.bg.clone()),
        ("dialog_fg_color", p.text.clone()),
        ("popover_bg_color", look.popover.clone()),
        ("popover_fg_color", p.text.clone()),
        ("popover_shade_color", shade(0.4)),
        ("thumbnail_bg_color", look.card.clone()),
        ("thumbnail_fg_color", p.text.clone()),
        ("active_toggle_bg_color", look.raised.clone()),
        ("active_toggle_fg_color", p.text.clone()),
        ("shade_color", shade(0.35)),
        ("theme_selected_bg_color", p.highlight.clone()),
        ("theme_selected_fg_color", p.text.clone()),
        ("link_color", p.accent.clone()),
    ];
    let mut css = String::new();
    for (name, value) in &colors {
        css.push_str(&format!("@define-color {name} {value};\n"));
    }
    css.push_str(":root {\n");
    for (name, value) in &colors {
        css.push_str(&format!("  --{}: {value};\n", name.replace('_', "-")));
    }
    if let Some(font) = &look.font.ui {
        css.push_str(&format!("}}\nwindow, dialog {{ font-family: \"{}\"; }}\n", font.replace('"', "")));
    } else {
        css.push_str("}\n");
    }
    css
}

/// Icons the icon themes don't have, shipped inside the program.
const ICONS: &[(&str, &str)] = &[
    ("settings-shortcuts-symbolic", include_str!("../data/icons/settings-shortcuts-symbolic.svg")),
    // Adwaita has none; the Laptop extension uses it.
    ("computer-laptop-symbolic", include_str!("../data/icons/computer-laptop-symbolic.svg")),
];

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
