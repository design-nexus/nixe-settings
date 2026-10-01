//! Running external commands.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Stdio};

/// Run a command to completion and return its trimmed stdout.
pub fn run(args: &[&str]) -> Result<String> {
    let (program, rest) = args.split_first().context("empty command")?;
    let output =
        Command::new(program).args(rest).stdin(Stdio::null()).output().with_context(|| format!("could not start {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
}

/// Like [`run`] but returns `None` on any failure.
pub fn output(args: &[&str]) -> Option<String> {
    run(args).ok()
}

pub fn present(program: &str) -> bool {
    std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())).unwrap_or(false)
}

/// Write a file atomically: temp file in the same directory, then rename.
pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}
