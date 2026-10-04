//! Moving your settings to another computer: everything Settings keeps in
//! `~/.config/settings` (its own preferences, `state.json`, sound and lighting)
//! as one JSON file.

use crate::{cmd, paths};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::Path;

const FORMAT: &str = "omarchy-settings-backup";

/// Only plain files Settings writes itself, by name, travel.
fn portable(name: &str) -> bool {
    !name.contains(['/', '\\']) && !name.starts_with('.') && (name.ends_with(".toml") || name == "state.json")
}

/// The backup of the files in `dir`.
pub fn export_from(dir: &Path) -> Result<String> {
    let mut files = Map::new();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| portable(n))
        .collect();
    names.sort();
    for name in names {
        let text = std::fs::read_to_string(dir.join(&name)).with_context(|| format!("reading {name}"))?;
        files.insert(name, Value::String(text));
    }
    let doc = json!({ "format": FORMAT, "version": 1, "app": env!("CARGO_PKG_VERSION"), "files": files });
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// The files in a backup, checked: (name, contents).
pub fn read_backup(text: &str) -> Result<Vec<(String, String)>> {
    let v: Value = serde_json::from_str(text).context("This isn't a Settings backup (not JSON)")?;
    if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
        bail!("This isn't a Settings backup");
    }
    let files = v.get("files").and_then(Value::as_object).context("The backup has no files in it")?;
    let mut out = Vec::new();
    for (name, text) in files {
        let Some(text) = text.as_str() else { continue };
        if !portable(name) {
            continue;
        }
        // Each file must still be readable, or the backup is damaged.
        if name.ends_with(".toml") {
            toml::from_str::<toml::Table>(text).with_context(|| format!("{name} in the backup is damaged"))?;
        } else {
            serde_json::from_str::<Value>(text).with_context(|| format!("{name} in the backup is damaged"))?;
        }
        out.push((name.clone(), text.to_string()));
    }
    if out.is_empty() {
        bail!("The backup has no settings in it");
    }
    Ok(out)
}

pub fn export() -> Result<String> {
    export_from(&paths::app_dir())
}

/// Write a backup's files into place. The caller restarts Settings to use them.
pub fn import(files: &[(String, String)]) -> Result<()> {
    let dir = paths::app_dir();
    std::fs::create_dir_all(&dir)?;
    for (name, text) in files {
        cmd::atomic_write(&dir.join(name), text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("settings-backup-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("old-panels-backup")).unwrap();
        std::fs::write(dir.join("settings.toml"), "theme = \"nord\"\n").unwrap();
        std::fs::write(dir.join("state.json"), "{\"options\":{}}").unwrap();
        std::fs::write(dir.join("notes.txt"), "not ours").unwrap();
        let text = export_from(&dir).unwrap();
        let files = read_backup(&text).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["settings.toml", "state.json"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_other_files() {
        assert!(read_backup("{}").is_err());
        assert!(read_backup("not json").is_err());
        let sneaky = r#"{"format":"omarchy-settings-backup","files":{"../../.bashrc":"x","a.toml":"ok = 1"}}"#;
        assert_eq!(read_backup(sneaky).unwrap(), vec![("a.toml".to_string(), "ok = 1".to_string())]);
        let damaged = r#"{"format":"omarchy-settings-backup","files":{"a.toml":"= broken"}}"#;
        assert!(read_backup(damaged).is_err());
    }
}
