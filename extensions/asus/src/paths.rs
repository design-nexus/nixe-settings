//! Well-known locations, honouring the XDG overrides.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(fallback))
}

pub fn config_home() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn omarchy_theme_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("omarchy/current/theme")
}

/// Small things to remember between runs (which fan profile is being edited).
pub fn state_dir() -> PathBuf {
    std::env::var_os("SETTINGS_EXTENSION_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg("XDG_STATE_HOME", ".local/state").join("settings/extensions/asus"))
}

/// The Omarchy theme's accent colour (its `accent`, else `blue`).
pub fn theme_accent() -> Option<String> {
    let text = std::fs::read_to_string(omarchy_theme_dir().join("colors.toml")).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let get = |k: &str| table.get(k).and_then(|v| v.as_str()).map(str::to_string);
    get("accent").or_else(|| get("blue"))
}

/// Whether Settings shows temperatures in Fahrenheit.
pub fn fahrenheit() -> bool {
    std::env::var("SETTINGS_TEMP_UNIT").is_ok_and(|u| u == "F")
}
