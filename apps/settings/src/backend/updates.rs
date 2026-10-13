//! Updates for Settings itself (GitHub releases) and its extensions (git).
//! No GTK: the window and `settings --update` share it.
//!
//! The result of the last check is cached in `~/.cache/settings/updates.json`;
//! the window checks again at most every 20 hours.

use crate::{ext, paths};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const REPO: &str = "design-nexus/omarchy-settings";
const RECHECK: u64 = 20 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    /// Unix time of the last check (0: never).
    pub checked: u64,
    /// The newest release, e.g. `0.4.1` (empty if unknown).
    pub latest: String,
    /// Its release page.
    pub url: String,
    /// Installed extensions with a newer version upstream.
    pub extensions: Vec<String>,
}

impl Status {
    /// A newer Settings than this one is out.
    pub fn settings_update(&self) -> bool {
        newer(&self.latest, env!("CARGO_PKG_VERSION"))
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cache_file() -> std::path::PathBuf {
    paths::cache_home().join("settings/updates.json")
}

pub fn cached() -> Status {
    std::fs::read_to_string(cache_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(s: &Status) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = crate::cmd::atomic_write(&cache_file(), &t);
    }
}

/// `1.10.0` > `1.9.3`; a leading `v` is ignored; anything unparsable is never newer.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The latest release: (version, page).
pub fn latest_release() -> Result<(String, String)> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = Command::new("curl").args(["-fsSL", "--max-time", "15", "-H", "Accept: application/vnd.github+json", &url]).output()?;
    if !out.status.success() {
        bail!("couldn't reach GitHub");
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("GitHub sent something unexpected")?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).context("no release found")?;
    let page = v.get("html_url").and_then(|t| t.as_str()).unwrap_or_default();
    Ok((tag.trim_start_matches('v').to_string(), page.to_string()))
}

/// Check now (`force`), or only if the last check is old. Extensions that are turned
/// off are checked too, so they're current when turned back on.
pub fn check(force: bool) -> Status {
    let old = cached();
    if !force && now().saturating_sub(old.checked) < RECHECK {
        return old;
    }
    let mut s = Status { checked: now(), ..old };
    if let Ok((latest, url)) = latest_release() {
        s.latest = latest;
        s.url = url;
    }
    s.extensions = ext::installed().iter().filter(|e| ext::manage::has_update(e).unwrap_or(false)).map(|e| e.id().to_string()).collect();
    save(&s);
    s
}

/// Forget an extension's pending update (after updating or removing it).
pub fn clear_extension(id: &str) {
    let mut s = cached();
    if s.extensions.iter().any(|e| e == id) {
        s.extensions.retain(|e| e != id);
        save(&s);
    }
}

/// Update every extension that has an update; returns the names updated.
pub fn update_extensions() -> Result<Vec<String>> {
    let pending = cached().extensions;
    let mut done = Vec::new();
    let mut errors = Vec::new();
    for e in ext::installed().into_iter().filter(|e| pending.iter().any(|p| p == e.id())) {
        match ext::manage::update(&e) {
            Ok(_) => {
                clear_extension(e.id());
                done.push(e.manifest.name.clone());
            }
            Err(err) => errors.push(format!("{}: {err:#}", e.manifest.name)),
        }
    }
    if !errors.is_empty() {
        bail!("{}", errors.join("; "));
    }
    Ok(done)
}

/// Whether this copy is the one the installer manages (`~/.local/bin/settings`),
/// rather than a development build.
pub fn self_updatable() -> bool {
    let installed = paths::home().join(".local/bin/settings");
    match (std::env::current_exe().and_then(|p| p.canonicalize()), installed.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Install the latest release with the regular installer.
pub fn update_settings() -> Result<()> {
    if !self_updatable() {
        bail!("This copy of Settings is a development build; update it from its source");
    }
    let script = format!("curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | bash");
    let mut c = Command::new("bash");
    c.args(["-c", &script]);
    ext::run_command(c, "the Settings installer", Duration::from_secs(900))?;
    // The keyboard backlight helper runs this binary; start it on the new one.
    let _ = Command::new("systemctl").args(["--user", "try-restart", "settings-kbd-idle.service"]).status();
    let mut s = cached();
    s.latest = String::new();
    save(&s);
    Ok(())
}

/// `settings --update [--check]`
pub fn cli(args: &[String]) -> Result<()> {
    let s = check(true);
    let current = env!("CARGO_PKG_VERSION");
    if s.settings_update() {
        println!("Settings {} is available (this is {current}).", s.latest);
    } else if s.latest.is_empty() {
        println!("Settings {current}: couldn't check for a newer release.");
    } else {
        println!("Settings {current} is up to date.");
    }
    if s.extensions.is_empty() {
        println!("Extensions are up to date.");
    } else {
        println!("Extension updates: {}", s.extensions.join(", "));
    }
    if args.iter().any(|a| a == "--check") {
        return Ok(());
    }
    if !s.extensions.is_empty() {
        let done = update_extensions()?;
        println!("Updated {}", done.join(", "));
    }
    if s.settings_update() {
        update_settings()?;
        println!("Settings updated to {}.", s.latest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("0.4.1", "0.4.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.4.0", "0.4.0"));
        assert!(!newer("0.3.9", "0.4.0"));
        assert!(!newer("", "0.4.0") && !newer("garbage", "0.4.0"));
    }

    #[test]
    fn status_round_trips() {
        let s = Status { checked: 5, latest: "9.9.9".into(), url: "u".into(), extensions: vec!["asus".into()] };
        let back: Status = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert!(back.settings_update());
        let none: Status = serde_json::from_str("{}").unwrap();
        assert!(!none.settings_update());
    }
}
