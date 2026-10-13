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

/// The suite's programs by the names they had before it was called Nixe.
const OLD_PROGRAMS: &[(&str, &str)] = &[
    ("archive", "nixe-archive"),
    ("chat", "nixe-chat"),
    ("fonts", "nixe-fonts"),
    ("media-player", "nixe-media-player"),
    ("music", "nixe-music"),
    ("notepad", "nixe-notepad"),
    ("pdf", "nixe-pdf"),
    ("settings", "nixe-settings"),
    ("tasks", "nixe-tasks"),
    ("torrents", "nixe-torrents"),
    ("nexus-photos", "nixe-photos"),
    ("npv", "nixe-viewer"),
    ("nexus-office", "nixe-office"),
    ("nexus-write", "nixe-write"),
    ("nexus-sheets", "nixe-sheets"),
    ("nexus-present", "nixe-present"),
];

/// A command line that starts one of the suite's programs by its old name,
/// with the new name.
fn renamed_command(command: &str) -> Option<String> {
    let trimmed = command.trim_start();
    let (word, rest) = trimmed.split_once(char::is_whitespace).map_or((trimmed, ""), |(w, r)| (w, r));
    let (dir, program) = word.rsplit_once('/').map_or(("", word), |(d, p)| (d, p));
    let new = OLD_PROGRAMS.iter().find(|(old, _)| *old == program)?.1;
    // A path must be the installer's, not some other program of the same name.
    if !dir.is_empty() && !dir.ends_with("/.local/bin") {
        return None;
    }
    let dir = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    Some(if rest.is_empty() { format!("{dir}{new}") } else { format!("{dir}{new} {rest}") })
}

/// A window class of the suite from before the rename, with the new one.
fn renamed_class(class: &str) -> String {
    let class = class.replace("io.github.nexus.photos.viewer", "io.github.design_nexus.nixe.PhotoViewer");
    let class = class.replace("io.github.nexus.photos", "io.github.design_nexus.nixe.Photos");
    let class = class.replace("io\\.github\\.nexus\\.photos", "io\\.github\\.design_nexus\\.nixe\\.Photos");
    if class.contains("design_nexus.nixe") || class.contains("design_nexus\\.nixe") {
        return class;
    }
    class.replace("design_nexus.", "design_nexus.nixe.").replace("design_nexus\\.", "design_nexus\\.nixe\\.")
}

impl State {
    pub fn load(path: &std::path::Path) -> Self {
        let mut state: Self =
            std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        state.rename_old_programs();
        state
    }

    /// Point bindings, autostart entries and window rules at the suite's programs
    /// by their Nixe names.
    fn rename_old_programs(&mut self) {
        for b in &mut self.binds {
            if let Some(c) = renamed_command(&b.command) {
                b.command = c;
            }
        }
        for c in self.autostart.values_mut() {
            if let Some(n) = renamed_command(c) {
                *c = n;
            }
        }
        for r in &mut self.window_rules {
            r.class = renamed_class(&r.class);
        }
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn renames_the_suite_programs() {
        assert_eq!(renamed_command("tasks").as_deref(), Some("nixe-tasks"));
        assert_eq!(renamed_command("settings --section audio").as_deref(), Some("nixe-settings --section audio"));
        assert_eq!(renamed_command("/home/u/.local/bin/notepad --toggle").as_deref(), Some("/home/u/.local/bin/nixe-notepad --toggle"));
        assert_eq!(renamed_command("/usr/bin/music"), None);
        assert_eq!(renamed_command("taskset -c 1 foo"), None);
        assert_eq!(renamed_command("nixe-tasks"), None);
        assert_eq!(renamed_class("^io\\.github\\.design_nexus\\.Notepad$"), "^io\\.github\\.design_nexus\\.nixe\\.Notepad$");
        assert_eq!(renamed_class("io.github.design_nexus.nixe.Music"), "io.github.design_nexus.nixe.Music");
        assert_eq!(renamed_class("firefox"), "firefox");
    }
}
