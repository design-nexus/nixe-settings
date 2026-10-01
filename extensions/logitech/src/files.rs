//! Small file helpers shared by the app and the helper.

use anyhow::{Context, Result};
use std::path::Path;

/// Write a file atomically: temp file in the same directory, then rename.
pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

/// The Omarchy theme's accent colour (`accent`, else `blue`), as `#rrggbb`.
pub fn theme_accent() -> Option<String> {
    let text = std::fs::read_to_string(crate::paths::omarchy_colors()).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let get = |k: &str| table.get(k).and_then(|v| v.as_str()).map(str::to_string);
    get("accent").or_else(|| get("blue"))
}
