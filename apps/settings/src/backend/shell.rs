//! Omarchy shell config (`~/.config/omarchy/shell.json`) and toggle flags.
//! Edits keep every other key and the key order intact.

use crate::{cmd, paths};
use anyhow::{Context, Result};
use serde_json::{Map, Value};

pub fn read() -> Value {
    std::fs::read_to_string(paths::shell_json())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

pub fn get(path: &[&str]) -> Option<Value> {
    let mut v = read();
    for p in path {
        v = v.get(*p)?.clone();
    }
    Some(v)
}

/// Set a nested value, creating objects on the way.
pub fn set(path: &[&str], value: Value) -> Result<()> {
    let mut root = read();
    let mut cur = &mut root;
    for (i, p) in path.iter().enumerate() {
        let obj = cur.as_object_mut().context("shell.json has an unexpected shape")?;
        if i == path.len() - 1 {
            obj.insert(p.to_string(), value);
            break;
        }
        cur = obj.entry(p.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    let text = serde_json::to_string_pretty(&root)? + "\n";
    cmd::atomic_write(&paths::shell_json(), &text)
}

pub fn toggle_on(name: &str) -> bool {
    paths::state_home().join("omarchy/toggles").join(name).exists()
}
