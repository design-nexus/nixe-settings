//! Permission to reach Corsair keyboards: a udev rule that tags their hidraw
//! nodes for the logged-in user. Corsair Web Hub in the browser needs it too.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Command;

pub const UDEV_RULE: &str = include_str!("../data/43-corsair.rules");

pub fn rule_path() -> PathBuf {
    PathBuf::from("/etc/udev/rules.d/43-corsair.rules")
}

pub fn rule_installed() -> bool {
    std::fs::read_to_string(rule_path()).is_ok_and(|t| t == UDEV_RULE)
}

/// Install the rule with pkexec (it asks for a password) and re-trigger hidraw devices.
pub fn install_rule() -> Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let tmp = dir.join("43-corsair.rules");
    std::fs::write(&tmp, UDEV_RULE).context("couldn't stage the rule")?;
    let script = format!(
        "install -Dm644 '{}' '{}' && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw --action=change",
        tmp.display(),
        rule_path().display()
    );
    let out = Command::new("pkexec").args(["sh", "-c", &script]).output().context("couldn't start pkexec");
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("{}", if msg.is_empty() { "Not allowed (the password prompt was cancelled?)".into() } else { msg });
    }
    Ok(())
}
