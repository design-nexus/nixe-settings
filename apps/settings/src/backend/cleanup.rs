//! One-time removal of the settings panels this app replaces.
//!
//! Nothing is imported from them. Everything they own is copied to a backup
//! folder first, then removed; if Hyprland reports config errors afterwards the
//! backup is put back automatically.

use super::hypr;
use crate::{cmd, paths};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub const PLUGINS: &[&str] =
    &["design-nexus.settings", "io.github.twiking.omasettings", "io.github.avillagran.omarchy-control-panel"];

/// Hyprland modules those panels generated.
const MODULES: &[&str] = &["hypr.omasettings", "hypr.control-panel", "hypr.gestures-generated", "hypr.control-panel-gestures"];

#[derive(Debug, Default, Clone)]
pub struct Plan {
    pub plugins: Vec<String>,
    pub files: Vec<PathBuf>,
    pub units: Vec<String>,
    /// file -> (line number, line) to remove or comment out
    pub edits: Vec<(PathBuf, Vec<(usize, String)>)>,
    pub bar_widgets: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
            && self.files.is_empty()
            && self.units.is_empty()
            && self.edits.is_empty()
            && self.bar_widgets.is_empty()
    }
}

fn plugin_dir(id: &str) -> PathBuf {
    paths::omarchy_config().join("plugins").join(id)
}

fn mentions_old_panel(text: &str) -> bool {
    PLUGINS.iter().any(|id| text.contains(&format!("plugins/{id}")))
}

fn requires_old_module(line: &str) -> bool {
    let l = line.trim();
    !l.starts_with("--")
        && MODULES.iter().any(|m| l.contains(&format!("require(\"{m}\")")) || l.contains(&format!("require('{m}')")))
}

pub fn plan() -> Plan {
    let cfg = paths::config_home();
    let mut p = Plan::default();

    for id in PLUGINS {
        if plugin_dir(id).exists() {
            p.plugins.push(id.to_string());
        }
    }

    let mut candidates: Vec<PathBuf> = vec![
        cfg.join("hypr/control-panel.lua"),
        cfg.join("hypr/gestures-generated.lua"),
        cfg.join("hypr/control-panel-gestures.lua"),
        cfg.join("hypr/hyprland.lua.omasettings.bak"),
        cfg.join("omarchy/omasettings.json"),
        cfg.join("omarchy/omasettings-ui.json"),
        cfg.join("omarchy/settings-audio-effects.json"),
        cfg.join("omarchy/settings-audio-effects.lock"),
        cfg.join("omarchy/settings-preamp-db"),
        cfg.join("omarchy/settings-auto-brightness.json"),
        cfg.join("omarchy/shell.json.omasettings.bak"),
        cfg.join("pipewire/settings-audio-effects.conf"),
        paths::state_home().join("omarchy/control-panel-prefs.json"),
        paths::state_home().join("omarchy/control-panel-profiles.json"),
    ];
    // hypr/omasettings.lua and its backups.
    if let Ok(entries) = std::fs::read_dir(cfg.join("hypr")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "omasettings.lua" || name.starts_with("omasettings.lua.") {
                candidates.push(e.path());
            }
        }
    }
    // Their launcher.
    let desktop = paths::home().join(".local/share/applications/settings.desktop");
    if std::fs::read_to_string(&desktop).is_ok_and(|t| t.contains("design-nexus.settings") || t.contains("X-Settings-Managed")) {
        candidates.push(desktop);
    }
    p.files = candidates.into_iter().filter(|f| f.exists()).collect();

    // User units they installed, or that run their code.
    let units_dir = cfg.join("systemd/user");
    if let Ok(entries) = std::fs::read_dir(&units_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".service") || e.path().is_dir() {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            if name == "settings-audio-effects.service" || mentions_old_panel(&text) {
                p.units.push(name);
            }
        }
    }
    p.units.sort();

    // Lines elsewhere that load their files or call their scripts.
    let deleting: Vec<PathBuf> = p.files.clone();
    if let Ok(entries) = std::fs::read_dir(cfg.join("hypr")) {
        let mut files: Vec<PathBuf> =
            entries.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "lua")).collect();
        files.sort();
        for f in files {
            if deleting.contains(&f) || f == paths::managed_lua() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let lines: Vec<(usize, String)> = text
                .lines()
                .enumerate()
                .filter(|(_, l)| {
                    requires_old_module(l)
                        || (!l.trim().starts_with("--") && mentions_old_panel(l))
                        || l.trim() == "-- Load settings written by OmaSettings (omasettings:managed)."
                })
                .map(|(i, l)| (i, l.to_string()))
                .collect();
            if !lines.is_empty() {
                p.edits.push((f, lines));
            }
        }
    }

    // Their bar widgets.
    let shell = super::shell::read();
    for section in ["left", "center", "right"] {
        if let Some(items) =
            shell.get("bar").and_then(|b| b.get("layout")).and_then(|l| l.get(section)).and_then(|s| s.as_array())
        {
            for item in items {
                if let Some(id) = item.get("id").and_then(|v| v.as_str())
                    && PLUGINS.contains(&id)
                    && !p.bar_widgets.contains(&id.to_string())
                {
                    p.bar_widgets.push(id.to_string());
                }
            }
        }
    }
    p
}

/// A human-readable summary for the confirmation dialog.
pub fn describe(p: &Plan) -> String {
    let mut s = String::new();
    if !p.plugins.is_empty() {
        s.push_str("Remove plugins:\n");
        for id in &p.plugins {
            s.push_str(&format!("  • {id}\n"));
        }
    }
    if !p.units.is_empty() {
        s.push_str("\nStop and remove services:\n");
        for u in &p.units {
            s.push_str(&format!("  • {u}\n"));
        }
    }
    if !p.files.is_empty() {
        s.push_str("\nDelete files they generated:\n");
        for f in &p.files {
            s.push_str(&format!("  • {}\n", paths::pretty(f)));
        }
    }
    if !p.edits.is_empty() {
        s.push_str("\nEdit your config (lines that load or call them):\n");
        for (f, lines) in &p.edits {
            s.push_str(&format!("  • {}\n", paths::pretty(f)));
            for (n, l) in lines {
                let action = if requires_old_module(l) || l.trim().starts_with("--") { "remove" } else { "comment out" };
                s.push_str(&format!("      line {} ({action}): {}\n", n + 1, l.trim()));
            }
        }
    }
    if !p.bar_widgets.is_empty() {
        s.push_str("\nTake their widgets off the bar.\n");
    }
    s
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, to)?;
    } else if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

/// Where a file lives inside the backup: its path relative to $HOME.
fn backup_path(backup: &Path, file: &Path) -> PathBuf {
    match file.strip_prefix(paths::home()) {
        Ok(rel) => backup.join(rel),
        Err(_) => backup.join(file.strip_prefix("/").unwrap_or(file)),
    }
}

fn edited(text: &str, lines: &[(usize, String)]) -> String {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        match lines.iter().find(|(n, _)| *n == i) {
            Some((_, l)) if requires_old_module(l) || l.trim().starts_with("--") => {}
            Some(_) => out.push(format!("-- (off: called a removed settings panel) {line}")),
            None => out.push(line.to_string()),
        }
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

pub struct Outcome {
    pub backup: PathBuf,
}

pub fn execute(p: &Plan) -> Result<Outcome> {
    let backup = paths::app_dir().join(format!("old-panels-backup-{}", hypr::timestamp()));
    std::fs::create_dir_all(&backup)?;

    // 1. Back everything up.
    let mut touched: Vec<PathBuf> = Vec::new();
    for id in &p.plugins {
        touched.push(plugin_dir(id));
    }
    touched.extend(p.files.iter().cloned());
    for u in &p.units {
        touched.push(paths::config_home().join("systemd/user").join(u));
    }
    for (f, _) in &p.edits {
        touched.push(f.clone());
    }
    touched.push(paths::shell_json());
    touched.push(paths::hyprland_lua());
    for t in &touched {
        if t.exists() {
            copy_tree(t, &backup_path(&backup, t)).with_context(|| format!("backing up {}", t.display()))?;
        }
    }
    let manifest: Vec<String> = touched.iter().map(|t| t.to_string_lossy().to_string()).collect();
    std::fs::write(backup.join("MANIFEST"), manifest.join("\n") + "\n")?;

    let result = (|| -> Result<()> {
        // 2. Services.
        for u in &p.units {
            let _ = cmd::run(&["systemctl", "--user", "disable", "--now", u]);
            let _ = std::fs::remove_file(paths::config_home().join("systemd/user").join(u));
        }
        let _ = cmd::run(&["systemctl", "--user", "daemon-reload"]);

        // 3. Plugins.
        for id in &p.plugins {
            let _ = cmd::run(&["omarchy-plugin-disable", id]);
            if cmd::run(&["omarchy-plugin-remove", id, "--yes"]).is_err() || plugin_dir(id).exists() {
                std::fs::remove_dir_all(plugin_dir(id)).with_context(|| format!("removing {id}"))?;
            }
        }

        // 4. Generated files.
        for f in &p.files {
            if f.exists() {
                std::fs::remove_file(f).with_context(|| format!("deleting {}", f.display()))?;
            }
        }

        // 5. Lines that load or call them.
        for (f, lines) in &p.edits {
            let text = std::fs::read_to_string(f)?;
            cmd::atomic_write(f, &edited(&text, lines))?;
        }

        // 6. Bar widgets.
        if !p.bar_widgets.is_empty() {
            let mut shell = super::shell::read();
            if let Some(layout) = shell.get_mut("bar").and_then(|b| b.get_mut("layout")).and_then(|l| l.as_object_mut()) {
                for (_, items) in layout.iter_mut() {
                    if let Some(arr) = items.as_array_mut() {
                        arr.retain(|i| !i.get("id").and_then(|v| v.as_str()).is_some_and(|id| PLUGINS.contains(&id)));
                    }
                }
            }
            cmd::atomic_write(&paths::shell_json(), &(serde_json::to_string_pretty(&shell)? + "\n"))?;
        }

        // 7. Make sure Settings' own file is loaded, then reload.
        super::store::flush();
        hypr::ensure_required()?;
        let _ = cmd::run(&["hyprctl", "reload"]);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let errors = hypr::config_errors();
        if !errors.is_empty() {
            bail!("Hyprland reported errors after the cleanup:\n{}", errors.join("\n"));
        }
        Ok(())
    })();

    match result {
        Ok(()) => Ok(Outcome { backup }),
        Err(e) => {
            restore(&backup)?;
            let _ = cmd::run(&["hyprctl", "reload"]);
            Err(e.context(format!("everything was restored from {}", paths::pretty(&backup))))
        }
    }
}

/// Put every backed-up path back where it came from.
pub fn restore(backup: &Path) -> Result<()> {
    let manifest = std::fs::read_to_string(backup.join("MANIFEST"))?;
    for line in manifest.lines().filter(|l| !l.is_empty()) {
        let original = PathBuf::from(line);
        let saved = backup_path(backup, &original);
        if !saved.exists() {
            continue;
        }
        if original.is_dir() {
            std::fs::remove_dir_all(&original)?;
        }
        copy_tree(&saved, &original)?;
    }
    let _ = cmd::run(&["systemctl", "--user", "daemon-reload"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_old_requires() {
        assert!(requires_old_module("require(\"hypr.omasettings\")"));
        assert!(requires_old_module("  require('hypr.control-panel')"));
        assert!(!requires_old_module("-- require(\"hypr.omasettings\")"));
        assert!(!requires_old_module("require(\"hypr.settings\")"));
    }

    #[test]
    fn edits_remove_requires_and_comment_calls() {
        let text = "a\n-- Load settings written by OmaSettings (omasettings:managed).\nrequire(\"hypr.omasettings\")\nhl.gesture({ action = function() hl.exec_cmd(\"/h/.config/omarchy/plugins/design-nexus.settings/bin/x\") end })\nb\n";
        let lines: Vec<(usize, String)> =
            text.lines().enumerate().filter(|(i, _)| [1, 2, 3].contains(i)).map(|(i, l)| (i, l.to_string())).collect();
        let out = edited(text, &lines);
        assert_eq!(out.lines().count(), 3);
        assert!(out.starts_with("a\n-- (off: called a removed settings panel) hl.gesture"));
        assert!(out.ends_with("b\n"));
    }

    #[test]
    fn mentions_plugin_paths() {
        assert!(mentions_old_panel("x /home/k/.config/omarchy/plugins/design-nexus.settings/bin/y"));
        assert!(!mentions_old_panel("x /home/k/.config/omarchy/plugins/asus.rog-g16/bin/y"));
    }
}
