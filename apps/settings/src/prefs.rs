//! This app's own preferences (`~/.config/settings/settings.toml`).

use crate::{cmd, paths};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    /// Follow the active Omarchy theme live.
    Omarchy,
    /// Use a bundled or custom theme.
    Theme,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: ThemeMode,
    pub theme: String,
    pub density: String,
    pub reduce_motion: bool,
    pub glow: bool,
    /// How temperatures are shown (they are always stored in Celsius).
    pub temp_unit: TempUnit,
    /// The old-panel cleanup has been offered once already.
    pub cleanup_offered: bool,
    pub last_section: String,
    /// Installed extensions that are turned off: kept, but their pages are hidden.
    pub disabled_extensions: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            density: "comfortable".into(),
            reduce_motion: false,
            glow: true,
            temp_unit: TempUnit::Celsius,
            cleanup_offered: false,
            last_section: "theme".into(),
            disabled_extensions: Vec::new(),
        }
    }
}

thread_local! {
    static PREFS: RefCell<Prefs> = RefCell::new(load());
}

/// Read from disk (not this thread's cached copy), for worker threads and headless commands.
pub fn read() -> Prefs {
    load()
}

fn load() -> Prefs {
    std::fs::read_to_string(paths::prefs_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
}

pub fn get() -> Prefs {
    PREFS.with(|p| p.borrow().clone())
}

pub fn update(change: impl FnOnce(&mut Prefs)) {
    PREFS.with(|p| {
        let mut prefs = p.borrow_mut();
        change(&mut prefs);
        if let Ok(text) = toml::to_string_pretty(&*prefs) {
            let _ = cmd::atomic_write(&paths::prefs_file(), &text);
        }
    });
}
