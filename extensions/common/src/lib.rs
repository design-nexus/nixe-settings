//! What every Settings extension in this repository shares: the command
//! line Settings talks to ([`run`]), page and row builders ([`page`]), the
//! one-time udev rule that lets the signed-in user reach a device
//! ([`access`]), small file and command helpers, and an OpenRGB client.

pub mod access;
pub mod lighting;
pub mod openrgb;
pub mod page;
pub mod sys;

use anyhow::{Result, bail};
use serde_json::Value;

/// One extension: its pages, how each is drawn, and what a change does.
pub trait Extension {
    /// `[{id, title, icon, description, keywords}]`, `[]` when there's nothing to show.
    fn pages(&self) -> Value;
    fn describe(&self, page: &str) -> Result<Value>;
    /// A row changed: `{"toast": …, "refresh": true}` or nothing.
    fn set(&self, page: &str, key: &str, value: &str) -> Result<Option<Value>>;
    /// After the Omarchy theme changes (with `hooks = ["theme-changed"]`).
    fn theme_changed(&self) -> Result<()> {
        Ok(())
    }
}

/// The program's main: `pages`, `describe PAGE`, `set PAGE KEY VALUE`,
/// `theme-changed`. Errors go to stderr with a non-zero exit, which Settings
/// shows as a message.
pub fn run(ext: &impl Extension) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let r: Result<()> = (|| {
        match arg(0) {
            "pages" => println!("{}", ext.pages()),
            "describe" => println!("{}", ext.describe(arg(1))?),
            "set" => {
                if let Some(reply) = ext.set(arg(1), arg(2), arg(3))? {
                    println!("{reply}");
                }
            }
            "theme-changed" => ext.theme_changed()?,
            other => bail!("usage: pages | describe PAGE | set PAGE KEY VALUE | theme-changed (not “{other}”)"),
        }
        Ok(())
    })();
    if let Err(e) = r {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

/// A short toast after a change.
pub fn toast(text: &str) -> Option<Value> {
    Some(serde_json::json!({ "toast": text }))
}

/// Ask Settings to draw the page again after a change.
pub fn refresh() -> Option<Value> {
    Some(serde_json::json!({ "refresh": true }))
}

/// Ask Settings for the page list again (devices came or went).
pub fn reload(toast: &str) -> Option<Value> {
    Some(serde_json::json!({ "toast": toast, "refresh": true, "reload": true }))
}
