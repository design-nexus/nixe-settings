//! Device access: Linux lets only root talk to many devices (hidraw, i2c,
//! LEDs) until a udev rule lets the signed-in user. The rule is installed
//! once with pkexec, which asks for a password.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A rule this extension installs: its file name in `/etc/udev/rules.d` and contents.
pub struct Rule {
    pub file: &'static str,
    pub text: &'static str,
    /// Subsystems to re-trigger so the rule applies without replugging.
    pub subsystems: &'static [&'static str],
}

impl Rule {
    pub fn path(&self) -> PathBuf {
        Path::new("/etc/udev/rules.d").join(self.file)
    }

    pub fn installed(&self) -> bool {
        std::fs::read_to_string(self.path()).is_ok_and(|t| t == self.text)
    }

    /// Install (or update) the rule and apply it now.
    pub fn install(&self) -> Result<()> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let tmp = dir.join(self.file);
        std::fs::write(&tmp, self.text).context("couldn't stage the rule")?;
        let mut script = format!("install -Dm644 '{}' '{}' && udevadm control --reload-rules", tmp.display(), self.path().display());
        for s in self.subsystems {
            script.push_str(&format!(" && udevadm trigger --subsystem-match={s} --action=change"));
        }
        // Loading a module the rule needs (i2c-dev) is part of the same step.
        if self.subsystems.contains(&"i2c-dev") {
            script = format!("modprobe i2c-dev; {script}");
        }
        let out = Command::new("pkexec").args(["sh", "-c", &script]).output().context("couldn't start pkexec");
        let _ = std::fs::remove_file(&tmp);
        let out = out?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            bail!("{}", if msg.is_empty() { "Not allowed (the password prompt was cancelled?)".into() } else { msg });
        }
        Ok(())
    }

    /// The banner and button rows a page shows while the rule is missing.
    pub fn rows(&self, what: &str) -> Vec<serde_json::Value> {
        vec![crate::page::desc(
            crate::page::button("access", "Allow your account", "Allow…"),
            &format!("Installs <tt>{}</tt> so {what} works without root. Asks for your password once.", self.path().display()),
        )]
    }
}

/// The text a page banner shows while access is missing.
pub const BANNER: &str = "Linux only lets <b>root</b> talk to these devices until a small rule allows your account to.";
