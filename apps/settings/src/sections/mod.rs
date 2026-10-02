//! Every page in the sidebar, in order.

use crate::widgets::Page;
use crate::{ext, paths};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub mod about;
pub mod apps;
pub mod audio;
pub mod bar;
pub mod barlayout;
pub mod connectivity;
pub mod displays;
pub mod extensions;
pub mod home;
pub mod idle;
pub mod keybindings;
pub mod keyboard;
pub mod look;
pub mod mouse;
pub mod nightlight;
pub mod plugins;
pub mod power;
pub mod theme;
pub mod trackpad;
pub mod workspaces;

/// How a page is filled in.
#[derive(Clone)]
pub enum Build {
    Native(fn(&Page)),
    /// Drawn from what an extension describes.
    Extension(Rc<ext::PageRef>),
}

#[derive(Clone)]
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
    pub build: Build,
    pub visible: fn() -> bool,
}

impl Section {
    pub fn run(&self, page: &Page) {
        match &self.build {
            Build::Native(f) => f(page),
            Build::Extension(r) => ext::render::build(page, self.id, r.clone()),
        }
    }

    pub fn config_files(&self) -> Vec<PathBuf> {
        match &self.build {
            Build::Native(_) => (self.files)(),
            Build::Extension(r) => r.ext.files(),
        }
    }
}

thread_local! {
    static INTERNED: RefCell<HashMap<String, &'static str>> = RefCell::default();
}

/// Section ids and titles are `&'static str`; extension ones are made once and reused.
fn intern(s: &str) -> &'static str {
    INTERNED.with(|m| *m.borrow_mut().entry(s.to_string()).or_insert_with(|| Box::leak(s.to_string().into_boxed_str())))
}

fn extension_sections(fresh: bool) -> Vec<Section> {
    ext::all_pages(fresh)
        .into_iter()
        .map(|r| Section {
            id: intern(&r.section_id()),
            title: intern(&r.page.title),
            icon: intern(&r.page.icon),
            group: "Devices",
            description: intern(&r.page.description),
            keywords: intern(&format!("{} {}", r.ext.manifest.name, r.page.keywords)),
            files: Vec::new,
            build: Build::Extension(Rc::new(r)),
            visible: always,
        })
        .collect()
}

fn always() -> bool {
    true
}

fn hypr(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|n| paths::hypr_dir().join(n)).collect()
}

pub fn all() -> Vec<Section> {
    all_with(false)
}

/// `fresh` asks every extension what it has now instead of using what it said last time.
pub fn all_with(fresh: bool) -> Vec<Section> {
    let mut list = vec![
        // No group: Home sits above the first heading.
        Section {
            id: "home",
            title: "Home",
            icon: "user-home-symbolic",
            group: "",
            description: "This computer at a glance: activity, storage and updates.",
            keywords: "overview system info stats cpu processor memory ram disk storage battery temperature network uptime updates packages aur firmware snapshot",
            files: Vec::new,
            build: Build::Native(home::build),
            visible: always,
        },
        // ----- Appearance -----
        Section {
            id: "theme",
            title: "Theme",
            icon: "preferences-desktop-appearance-symbolic",
            group: "Appearance",
            description: "Omarchy theme, wallpaper, fonts, and how this window looks.",
            keywords: "colors colours palette wallpaper background font dracula catppuccin tokyo night one dark nord gruvbox follow omarchy",
            files: || vec![paths::omarchy_colors(), paths::app_dir().join("settings.toml")],
            build: Build::Native(theme::build),
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
            build: Build::Native(look::build),
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
            build: Build::Native(bar::build),
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
            build: Build::Native(workspaces::build),
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
            build: Build::Native(keybindings::build),
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
            build: Build::Native(idle::build),
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
            build: Build::Native(nightlight::build),
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
            build: Build::Native(keyboard::build),
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
            build: Build::Native(mouse::build),
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
            build: Build::Native(trackpad::build),
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
            build: Build::Native(displays::build),
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
            build: Build::Native(audio::build),
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
            build: Build::Native(connectivity::build),
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
            build: Build::Native(power::build),
            visible: always,
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
            build: Build::Native(apps::build),
            visible: always,
        },
        Section {
            id: "extensions",
            title: "Extensions",
            icon: "application-x-addon-symbolic",
            group: "System",
            description: "Device support for more brands, installed from GitHub. Each one adds its own pages under Devices.",
            keywords: "extensions add-ons addons devices brands asus aura logitech obsbot webcam headset razer corsair install github",
            files: Vec::new,
            build: Build::Native(extensions::build),
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
            build: Build::Native(plugins::build),
            visible: always,
        },
        Section {
            id: "about",
            title: "About",
            icon: "help-about-symbolic",
            group: "System",
            description: "Settings and Omarchy versions, updating them, and snapshots.",
            keywords: "update upgrade version snapshot about system info kernel cpu memory",
            files: Vec::new,
            build: Build::Native(about::build),
            visible: always,
        },
    ];
    // Extension pages go at the end of Devices.
    let at = list.iter().position(|s| s.group == "System").unwrap_or(list.len());
    list.splice(at..at, extension_sections(fresh));
    list
}
