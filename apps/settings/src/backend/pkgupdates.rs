//! Updates waiting for the system: Omarchy, repository packages, the AUR and
//! firmware. No GTK. Checking never changes anything: `checkupdates` syncs a
//! private copy of the package databases.
//!
//! The last result is cached in `~/.cache/settings/system-updates.json`.

use crate::{cmd, paths};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const RECHECK: u64 = 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pkg {
    pub name: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    /// Unix time of the last check (0: never).
    pub checked: u64,
    /// What `omarchy-update-available` reported, if Omarchy has an update.
    pub omarchy: Option<String>,
    pub system: Vec<Pkg>,
    pub aur: Vec<Pkg>,
    pub firmware: Vec<Pkg>,
    /// Checks that couldn't run, e.g. "Package mirrors couldn't be reached".
    pub errors: Vec<String>,
}

impl Status {
    /// Everything `omarchy-update` would install (Omarchy itself is one of the packages).
    pub fn count(&self) -> usize {
        self.system.len() + self.aur.len() + self.firmware.len()
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cache_file() -> std::path::PathBuf {
    paths::cache_home().join("settings/system-updates.json")
}

pub fn cached() -> Status {
    std::fs::read_to_string(cache_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn stale(s: &Status) -> bool {
    now().saturating_sub(s.checked) >= RECHECK
}

/// `name old -> new [anything]`, as printed by checkupdates and `yay -Qua`.
pub fn parse_list(text: &str) -> Vec<Pkg> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 4 && f[2] == "->").then(|| Pkg { name: f[0].into(), old: f[1].into(), new: f[3].into() })
        })
        .collect()
}

/// `fwupdmgr get-updates --json`: each device with a release newer than what it runs.
pub fn parse_fwupd(text: &str) -> Vec<Pkg> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    v.get("Devices")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let release = d.get("Releases")?.as_array()?.first()?;
            Some(Pkg { name: s(d, "Name"), old: s(d, "Version"), new: s(release, "Version") })
        })
        .collect()
}

/// Run a command, returning (exit code, stdout).
fn run(args: &[&str]) -> Option<(i32, String)> {
    let out = Command::new(args[0]).args(&args[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    Some((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).to_string()))
}

/// Check now (`force`), or return the cached result if it's recent.
pub fn check(force: bool) -> Status {
    let old = cached();
    if !force && !stale(&old) {
        return old;
    }
    let mut s = Status { checked: now(), ..Default::default() };
    if cmd::present("omarchy-update-available")
        && let Some((0, out)) = run(&["omarchy-update-available"])
    {
        s.omarchy = out.lines().next().map(|l| l.trim().to_string());
    }
    // checkupdates: 0 = updates, 2 = none, anything else = it couldn't check.
    match run(&["checkupdates", "--nocolor"]) {
        Some((0, out)) => s.system = parse_list(&out),
        Some((2, _)) => {}
        Some(_) => s.errors.push("Package mirrors couldn't be reached".into()),
        None => s.errors.push("checkupdates isn't installed (pacman-contrib)".into()),
    }
    if cmd::present("yay")
        && let Some((_, out)) = run(&["yay", "-Qua"])
    {
        s.aur = parse_list(&out);
    }
    if cmd::present("fwupdmgr")
        && let Some((_, out)) = run(&["fwupdmgr", "get-updates", "--json"])
    {
        s.firmware = parse_fwupd(&out);
    }
    if let Ok(t) = serde_json::to_string_pretty(&s) {
        let _ = cmd::atomic_write(&cache_file(), &t);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_lists() {
        let p = parse_list("clang 22.1.8-1 -> 23.1.1-1\ndocker 1:29.8.1-1 -> 1:29.8.2-1 [ignored]\nnoise line\n");
        assert_eq!(p.len(), 2);
        assert_eq!(p[1], Pkg { name: "docker".into(), old: "1:29.8.1-1".into(), new: "1:29.8.2-1".into() });
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn parses_fwupd() {
        assert!(parse_fwupd("{\"Devices\": []}").is_empty());
        assert!(parse_fwupd("No updatable devices").is_empty());
        let p = parse_fwupd(
            r#"{"Devices":[{"Name":"UEFI dbx","Version":"77","Releases":[{"Version":"83"}]},{"Name":"SSD","Version":"1","Releases":[]}]}"#,
        );
        assert_eq!(p, vec![Pkg { name: "UEFI dbx".into(), old: "77".into(), new: "83".into() }]);
    }

    #[test]
    fn counts_and_round_trips() {
        let s =
            Status { checked: 1, system: parse_list("a 1 -> 2\nb 1 -> 2"), aur: parse_list("c 1 -> 2"), ..Default::default() };
        assert_eq!(s.count(), 3);
        let back: Status = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert!(stale(&Status::default()));
    }
}
