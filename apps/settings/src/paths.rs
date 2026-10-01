//! Well-known locations. Every path honours the XDG overrides so the whole app
//! can be pointed at a scratch copy of `~/.config` for testing.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".config"))
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/state"))
}

pub fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".local/share"))
}

pub fn cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".cache"))
}

/// `~/.local/share/settings/extensions` — one folder per installed extension.
pub fn ext_dir() -> PathBuf {
    data_home().join("settings/extensions")
}

/// What extensions said they offer last time, so the sidebar fills in without waiting.
pub fn ext_cache_dir() -> PathBuf {
    cache_home().join("settings/extensions")
}

/// `~/.config/settings` — everything this app owns lives here.
pub fn app_dir() -> PathBuf {
    config_home().join("settings")
}

pub fn prefs_file() -> PathBuf {
    app_dir().join("settings.toml")
}

pub fn state_file() -> PathBuf {
    app_dir().join("state.json")
}

pub fn custom_themes_dir() -> PathBuf {
    app_dir().join("themes")
}

pub fn hypr_dir() -> PathBuf {
    config_home().join("hypr")
}

/// The single Hyprland file this app writes.
pub fn managed_lua() -> PathBuf {
    hypr_dir().join("settings.lua")
}

pub fn hyprland_lua() -> PathBuf {
    hypr_dir().join("hyprland.lua")
}

pub fn omarchy_config() -> PathBuf {
    config_home().join("omarchy")
}

pub fn shell_json() -> PathBuf {
    omarchy_config().join("shell.json")
}

pub fn omarchy_theme_dir() -> PathBuf {
    state_home().join("omarchy/current/theme")
}

pub fn omarchy_colors() -> PathBuf {
    omarchy_theme_dir().join("colors.toml")
}

/// Replace `$HOME` with `~` for display.
pub fn pretty(path: &std::path::Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
