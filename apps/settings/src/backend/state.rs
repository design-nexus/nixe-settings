//! Everything Settings has changed, persisted in `~/.config/settings/state.json`.
//! The managed Lua file is always regenerated from this, never parsed back.

use super::gestures::Gestures;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bind {
    pub keys: String,
    pub description: String,
    pub command: String,
}

/// A window rule (matching `class` and/or `title`) or a layer rule (matching
/// `class` as the namespace), with one effect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Rule {
    pub class: String,
    pub title: String,
    pub effect: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Monitor {
    pub mode: Option<String>,
    pub position: Option<String>,
    pub scale: Option<f64>,
    pub transform: Option<i64>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct State {
    /// Hyprland options by dotted path, e.g. `input.touchpad.natural_scroll`.
    pub options: BTreeMap<String, Value>,
    /// Per-device overrides (`hl.device`), keyed by device name.
    pub devices: BTreeMap<String, BTreeMap<String, Value>>,
    pub gestures: Gestures,
    /// Bindings added here.
    pub binds: Vec<Bind>,
    /// Existing bindings turned off here.
    pub unbinds: Vec<String>,
    pub monitors: BTreeMap<String, Monitor>,
    pub env: BTreeMap<String, String>,
    /// Loudest the volume keys may go, in percent. 100 leaves Omarchy's keys alone.
    pub max_volume: Option<u32>,
    /// Programs started with the session if they aren't already running
    /// (process name -> command).
    pub autostart: BTreeMap<String, String>,
    pub window_rules: Vec<Rule>,
    pub layer_rules: Vec<Rule>,
}

impl State {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
}
