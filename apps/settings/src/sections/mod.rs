//! Every page in the sidebar, in order.

use crate::paths;
use crate::widgets::Page;
use std::path::PathBuf;

pub mod about;
pub mod apps;
pub mod asus;
pub mod audio;
pub mod aura;
pub mod bar;
pub mod barlayout;
pub mod connectivity;
pub mod displays;
pub mod idle;
pub mod keybindings;
pub mod keyboard;
pub mod look;
pub mod mouse;
pub mod nightlight;
pub mod plugins;
pub mod power;
pub mod system;
pub mod theme;
pub mod trackpad;
pub mod workspaces;

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    /// Extra words the search should find this page by.
    pub keywords: &'static str,
    /// Files the "Open config" button offers.
    pub files: fn() -> Vec<PathBuf>,
    pub build: fn(&Page),
    pub visible: fn() -> bool,
}

fn always() -> bool {
    true
}

fn hypr(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|n| paths::hypr_dir().join(n)).collect()
}

pub fn all() -> Vec<Section> {
    vec![
        // ----- Appearance -----
        Section {
            id: "theme",
            title: "Theme",
            icon: "preferences-desktop-appearance-symbolic",
            group: "Appearance",
            description: "Omarchy theme, wallpaper, fonts, and how this window looks.",
            keywords: "colors colours palette wallpaper background font dracula catppuccin tokyo night one dark nord gruvbox follow omarchy",
            files: || vec![paths::omarchy_colors(), paths::app_dir().join("settings.toml")],
            build: theme::build,
            visible: always,
        },
        Section {
            id: "look",
            title: "Look & Feel",
            icon: "applications-graphics-symbolic",
            group: "Appearance",
            description: "Gaps, borders, rounding, transparency, blur, shadows and animations.",
            keywords: "gaps border rounding radius opacity transparency blur shadow animation cursor layout dwindle master scrolling",
            files: || hypr(&["looknfeel.lua", "settings.lua"]),
            build: look::build,
            visible: always,
        },
        Section {
            id: "bar",
            title: "Bar & Notifications",
            icon: "preferences-system-details-symbolic",
            group: "Appearance",
            description: "Where the bar sits, how it looks, and how notifications behave.",
            keywords: "waybar panel top bottom transparent notifications do not disturb silence shell text size",
            files: || vec![paths::shell_json(), paths::omarchy_config().join("shell.toml")],
            build: bar::build,
            visible: always,
        },
        // ----- Desktop -----
        Section {
            id: "workspaces",
            title: "Windows & Workspaces",
            icon: "view-grid-symbolic",
            group: "Desktop",
            description: "How windows are placed, focused and grouped, and how workspaces behave.",
            keywords: "focus follows mouse layout dwindle master scrolling split workspace resize",
            files: || hypr(&["looknfeel.lua", "settings.lua"]),
            build: workspaces::build,
            visible: always,
        },
        Section {
            id: "keybindings",
            title: "Keybindings",
            icon: "preferences-desktop-keyboard-symbolic",
            group: "Desktop",
            description: "Every shortcut you have, plus your own. Turn any of them off.",
            keywords: "shortcuts keys hotkeys bind unbind super",
            files: || hypr(&["bindings.lua", "settings.lua"]),
            build: keybindings::build,
            visible: always,
        },
        Section {
            id: "idle",
            title: "Idle & Lock",
            icon: "system-lock-screen-symbolic",
            group: "Desktop",
            description: "When the screensaver starts and the screen locks.",
            keywords: "screensaver lock sleep suspend timeout stay awake idle inhibit",
            files: || vec![paths::shell_json()],
            build: idle::build,
            visible: always,
        },
        Section {
            id: "nightlight",
            title: "Night Light",
            icon: "night-light-symbolic",
            group: "Desktop",
            description: "Warmer colours in the evening with hyprsunset.",
            keywords: "hyprsunset blue light temperature warm evening schedule",
            files: || hypr(&["hyprsunset.conf"]),
            build: nightlight::build,
            visible: always,
        },
        // ----- Input -----
        Section {
            id: "keyboard",
            title: "Keyboard",
            icon: "input-keyboard-symbolic",
            group: "Input",
            description: "Layouts, compose and caps lock, and key repeat.",
            keywords: "layout language us variant compose caps lock repeat rate delay numlock xkb",
            files: || hypr(&["input.lua", "settings.lua"]),
            build: keyboard::build,
            visible: always,
        },
        Section {
            id: "mouse",
            title: "Mouse",
            icon: "input-mouse-symbolic",
            group: "Input",
            description: "Pointer speed, acceleration, scrolling and per-device overrides.",
            keywords: "pointer sensitivity speed acceleration accel flat adaptive scroll natural left handed cursor",
            files: || hypr(&["input.lua", "settings.lua"]),
            build: mouse::build,
            visible: always,
        },
        Section {
            id: "trackpad",
            title: "Trackpad",
            icon: "input-touchpad-symbolic",
            group: "Input",
            description: "Tapping, scrolling, and two-, three- and four-finger gestures.",
            keywords: "touchpad gestures swipe pinch tap click fingers natural scrolling reverse invert drag",
            files: || hypr(&["input.lua", "settings.lua"]),
            build: trackpad::build,
            visible: always,
        },
        // ----- Devices -----
        Section {
            id: "displays",
            title: "Displays",
            icon: "video-display-symbolic",
            group: "Devices",
            description: "Resolution, refresh rate, scale and arrangement of each monitor.",
            keywords: "monitor screen resolution refresh hz scale hidpi position rotate brightness",
            files: || hypr(&["monitors.lua", "settings.lua"]),
            build: displays::build,
            visible: always,
        },
        Section {
            id: "audio",
            title: "Sound",
            icon: "audio-speakers-symbolic",
            group: "Devices",
            description: "Volume, devices, a 9-band equalizer and a preamp to make everything louder.",
            keywords: "audio volume loud quiet boost preamp equalizer eq bass treble speakers headphones microphone output input",
            files: crate::backend::audio::config_files,
            build: audio::build,
            visible: always,
        },
        Section {
            id: "connectivity",
            title: "Wi-Fi & Bluetooth",
            icon: "network-wireless-symbolic",
            group: "Devices",
            description: "Wireless networks and Bluetooth devices.",
            keywords: "wifi network wireless ethernet bluetooth pair airplane",
            files: Vec::new,
            build: connectivity::build,
            visible: always,
        },
        Section {
            id: "power",
            title: "Power & Battery",
            icon: "battery-full-charged-symbolic",
            group: "Devices",
            description: "Power profiles, brightness and keyboard backlight.",
            keywords: "battery power profile performance balanced saver brightness backlight charge",
            files: Vec::new,
            build: power::build,
            visible: always,
        },
        Section {
            id: "asus",
            title: "ASUS",
            icon: "input-gaming-symbolic",
            group: "Devices",
            description: "Performance profile, fan curves, battery limit and firmware settings via asusctl.",
            keywords: "asus rog zephyrus fan curve profile performance charge limit battery gpu mux dgpu overdrive power limit tgp asusctl screenpad",
            files: Vec::new,
            build: asus::build,
            visible: asus::available,
        },
        Section {
            id: "aura",
            title: "Aura Lighting",
            icon: "keyboard-brightness-symbolic",
            group: "Devices",
            description: "Keyboard backlight and effects, the Slash lightbar and other ASUS lights.",
            keywords: "aura rgb keyboard backlight lighting effects slash lightbar timeout idle led anime matrix rainbow colour",
            files: || vec![crate::backend::kbdidle::config_file()],
            build: aura::build,
            visible: crate::backend::asus::lighting_available,
        },
        // ----- System -----
        Section {
            id: "apps",
            title: "Default Apps",
            icon: "system-run-symbolic",
            group: "System",
            description: "Which apps open links, files, folders, media, documents and mail.",
            keywords: "default browser terminal editor file manager image video music pdf mail xdg mime open with",
            files: || vec![paths::config_home().join("mimeapps.list")],
            build: apps::build,
            visible: always,
        },
        Section {
            id: "plugins",
            title: "Plugins",
            icon: "application-x-addon-symbolic",
            group: "System",
            description: "Omarchy shell plugins: turn them on or off and keep them updated.",
            keywords: "plugins extensions widgets bar update enable disable remove",
            files: || vec![paths::shell_json()],
            build: plugins::build,
            visible: always,
        },
        Section {
            id: "system",
            title: "Settings App",
            icon: "preferences-system-symbolic",
            group: "System",
            description: "What this app manages, and removing the old settings panels.",
            keywords: "managed reset restore old panels cleanup omasettings control panel",
            files: || vec![paths::managed_lua(), paths::state_file(), paths::prefs_file()],
            build: system::build,
            visible: always,
        },
        Section {
            id: "about",
            title: "Updates & About",
            icon: "help-about-symbolic",
            group: "System",
            description: "Omarchy version, updates, snapshots and system information.",
            keywords: "update upgrade version snapshot about system info kernel cpu memory",
            files: Vec::new,
            build: about::build,
            visible: always,
        },
    ]
}
