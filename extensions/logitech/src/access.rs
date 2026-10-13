//! Permission to reach the devices: a udev rule that tags Logitech hidraw
//! nodes for the logged-in user.

use crate::paths;
use anyhow::{Context, Result, bail};
use std::process::Command;

pub const UDEV_RULE: &str = include_str!("../data/42-logi.rules");

/// Install the udev rule with pkexec and re-trigger hidraw devices.
pub fn install_udev_rule() -> Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let tmp = dir.join("42-logi.rules");
    std::fs::write(&tmp, UDEV_RULE).context("couldn't stage the rule")?;
    let script = format!(
        "install -Dm644 '{}' '{}' && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw --action=change",
        tmp.display(),
        paths::udev_rule().display()
    );
    let out = Command::new("pkexec").args(["sh", "-c", &script]).output().context("couldn't start pkexec");
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
