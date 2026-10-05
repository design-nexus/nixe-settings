//! Autostart desktop applications (`~/.config/autostart` and `/etc/xdg/autostart`).

use crate::paths;
use gtk::gio;
use gtk::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const GROUP: &str = "Desktop Entry";

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub comment: String,
    pub exec: String,
    pub icon: String,
    pub enabled: bool,
    pub is_system: bool,
}

#[derive(Default, Debug)]
struct RawDesktop {
    name: Option<String>,
    exec: Option<String>,
    comment: Option<String>,
    icon: Option<String>,
    hidden: Option<bool>,
    no_display: Option<bool>,
    autostart_enabled: Option<bool>,
}

fn read_desktop_file(path: &Path) -> Option<RawDesktop> {
    let kf = gtk::glib::KeyFile::new();
    if kf.load_from_file(path, gtk::glib::KeyFileFlags::NONE).is_err() {
        return None;
    }
    let name = kf.string(GROUP, "Name").ok().map(|s| s.to_string());
    let exec = kf.string(GROUP, "Exec").ok().map(|s| s.to_string());
    let comment = kf.string(GROUP, "Comment").ok().map(|s| s.to_string());
    let icon = kf.string(GROUP, "Icon").ok().map(|s| s.to_string());
    let hidden = kf.boolean(GROUP, "Hidden").ok();
    let no_display = kf.boolean(GROUP, "NoDisplay").ok();
    let autostart_enabled = kf.boolean(GROUP, "X-GNOME-Autostart-enabled").ok();

    Some(RawDesktop { name, exec, comment, icon, hidden, no_display, autostart_enabled })
}

pub fn system_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(val) = std::env::var_os("XDG_CONFIG_DIRS") {
        for p in std::env::split_paths(&val) {
            dirs.push(p.join("autostart"));
        }
    }
    if dirs.is_empty() {
        dirs.push(PathBuf::from("/etc/xdg/autostart"));
    }
    dirs
}

/// Read all autostart items combining user and system autostart directories.
pub fn list_from(user_dir: &Path, sys_dirs: &[PathBuf]) -> Vec<Item> {
    let mut system_entries: BTreeMap<String, RawDesktop> = BTreeMap::new();
    for dir in sys_dirs {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
                if system_entries.contains_key(&id) {
                    continue;
                }
                if let Some(raw) = read_desktop_file(&path) {
                    system_entries.insert(id, raw);
                }
            }
        }
    }

    let mut user_entries: BTreeMap<String, RawDesktop> = BTreeMap::new();
    if user_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(user_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            if let Some(raw) = read_desktop_file(&path) {
                user_entries.insert(id, raw);
            }
        }
    }

    let all_ids: HashSet<String> = system_entries.keys().chain(user_entries.keys()).cloned().collect();
    let mut items = Vec::new();

    for id in all_ids {
        let sys = system_entries.get(&id);
        let usr = user_entries.get(&id);

        let no_display = usr.and_then(|u| u.no_display).or_else(|| sys.and_then(|s| s.no_display)).unwrap_or(false);
        if no_display {
            continue;
        }

        let name = usr
            .and_then(|u| u.name.clone())
            .or_else(|| sys.and_then(|s| s.name.clone()))
            .unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());

        let exec = usr.and_then(|u| u.exec.clone()).or_else(|| sys.and_then(|s| s.exec.clone())).unwrap_or_default();

        let comment = usr.and_then(|u| u.comment.clone()).or_else(|| sys.and_then(|s| s.comment.clone())).unwrap_or_default();

        let icon = usr.and_then(|u| u.icon.clone()).or_else(|| sys.and_then(|s| s.icon.clone())).unwrap_or_default();

        // An orphan file with no name and no exec is invalid
        if exec.is_empty() && name == id.trim_end_matches(".desktop") {
            continue;
        }

        let is_system = sys.is_some();

        let enabled = if let Some(u) = usr {
            !(u.hidden == Some(true) || u.autostart_enabled == Some(false))
        } else if let Some(s) = sys {
            !(s.hidden == Some(true) || s.autostart_enabled == Some(false))
        } else {
            false
        };

        items.push(Item { id, name, comment, exec, icon, enabled, is_system });
    }

    // Active/enabled first, then alphabetical by name
    items.sort_by(|a, b| b.enabled.cmp(&a.enabled).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

    items
}

/// List all startup items on the running system.
pub fn list() -> Vec<Item> {
    list_from(&paths::autostart_dir(), &system_dirs())
}

/// Set enabled/disabled state for an autostart item.
pub fn set_enabled_in(user_dir: &Path, id: &str, is_system: bool, enabled: bool) -> Result<(), String> {
    std::fs::create_dir_all(user_dir).map_err(|e| format!("Couldn't create autostart directory: {e}"))?;
    let path = user_dir.join(id);

    if enabled {
        if is_system && path.exists() {
            // If the user file only exists to hide the system file, removing it restores the system default
            let is_minimal_mask = std::fs::read_to_string(&path).map(|t| {
                let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
                lines.len() <= 2 && lines.contains(&"Hidden=true")
            }).unwrap_or(false);

            if is_minimal_mask {
                let _ = std::fs::remove_file(&path);
                return Ok(());
            }
        }

        let kf = gtk::glib::KeyFile::new();
        if path.exists() {
            let _ = kf.load_from_file(&path, gtk::glib::KeyFileFlags::NONE);
        }
        kf.set_boolean(GROUP, "Hidden", false);
        kf.set_boolean(GROUP, "X-GNOME-Autostart-enabled", true);
        kf.save_to_file(&path).map_err(|e| format!("Couldn't save {id}: {e}"))?;
    } else {
        let kf = gtk::glib::KeyFile::new();
        if path.exists() {
            let _ = kf.load_from_file(&path, gtk::glib::KeyFileFlags::NONE);
        } else {
            kf.set_string(GROUP, "Type", "Application");
        }
        kf.set_boolean(GROUP, "Hidden", true);
        kf.set_boolean(GROUP, "X-GNOME-Autostart-enabled", false);
        kf.save_to_file(&path).map_err(|e| format!("Couldn't save {id}: {e}"))?;
    }

    Ok(())
}

pub fn set_enabled(id: &str, is_system: bool, enabled: bool) -> Result<(), String> {
    set_enabled_in(&paths::autostart_dir(), id, is_system, enabled)
}

/// Remove an autostart item. User items are deleted; system items are disabled via Hidden=true.
pub fn remove_in(user_dir: &Path, id: &str, is_system: bool) -> Result<(), String> {
    if is_system {
        set_enabled_in(user_dir, id, is_system, false)
    } else {
        let path = user_dir.join(id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Couldn't remove {id}: {e}"))?;
        }
        Ok(())
    }
}

pub fn remove(id: &str, is_system: bool) -> Result<(), String> {
    remove_in(&paths::autostart_dir(), id, is_system)
}

/// Add an installed desktop application to the autostart directory.
pub fn add_app_in(user_dir: &Path, app: &gio::AppInfo) -> Result<String, String> {
    std::fs::create_dir_all(user_dir).map_err(|e| format!("Couldn't create autostart directory: {e}"))?;

    let name = app.name().to_string();
    let id = app.id().map(|i| i.to_string()).unwrap_or_else(|| {
        format!("{}.desktop", name.to_lowercase().replace(' ', "-"))
    });
    let id = if id.ends_with(".desktop") { id } else { format!("{id}.desktop") };

    let target = user_dir.join(&id);

    // Look for existing desktop file in applications directories to copy
    let app_dirs = [
        paths::data_home().join("applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
    ];

    let mut copied = false;
    for dir in &app_dirs {
        let src = dir.join(&id);
        if src.exists() {
            let kf = gtk::glib::KeyFile::new();
            if kf.load_from_file(&src, gtk::glib::KeyFileFlags::NONE).is_ok() {
                kf.set_boolean(GROUP, "Hidden", false);
                kf.set_boolean(GROUP, "X-GNOME-Autostart-enabled", true);
                if kf.save_to_file(&target).is_ok() {
                    copied = true;
                    break;
                }
            }
        }
    }

    if !copied {
        let exec = app.commandline().map(|c| c.to_string_lossy().to_string()).unwrap_or_default();
        let icon = app.icon().and_then(|i| i.to_string()).map(|s| s.to_string()).unwrap_or_default();
        let kf = gtk::glib::KeyFile::new();
        kf.set_string(GROUP, "Type", "Application");
        kf.set_string(GROUP, "Name", &name);
        kf.set_string(GROUP, "Exec", &exec);
        if !icon.is_empty() {
            kf.set_string(GROUP, "Icon", &icon);
        }
        kf.set_boolean(GROUP, "Hidden", false);
        kf.set_boolean(GROUP, "X-GNOME-Autostart-enabled", true);
        kf.save_to_file(&target).map_err(|e| format!("Couldn't create autostart desktop file: {e}"))?;
    }

    Ok(name)
}

pub fn add_app(app: &gio::AppInfo) -> Result<String, String> {
    add_app_in(&paths::autostart_dir(), app)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("settings-autostart-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_and_sorts_items() {
        let temp = TempDir::new("parse");
        let user_dir = temp.path().join("user_autostart");
        let sys_dir = temp.path().join("sys_autostart");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::create_dir_all(&sys_dir).unwrap();

        // 1. User app: Enabled
        std::fs::write(
            user_dir.join("dropbox.desktop"),
            "[Desktop Entry]\nName=Dropbox\nExec=dropbox\nType=Application\n",
        )
        .unwrap();

        // 2. System app: Enabled
        std::fs::write(
            sys_dir.join("system-helper.desktop"),
            "[Desktop Entry]\nName=System Helper\nExec=syshelper\nType=Application\n",
        )
        .unwrap();

        // 3. System app with NoDisplay: should be ignored
        std::fs::write(
            sys_dir.join("daemon.desktop"),
            "[Desktop Entry]\nName=Daemon\nExec=daemon\nNoDisplay=true\n",
        )
        .unwrap();

        // 4. System app masked by user with Hidden=true: should be disabled
        std::fs::write(
            sys_dir.join("print-applet.desktop"),
            "[Desktop Entry]\nName=Print Applet\nExec=printapp\n",
        )
        .unwrap();
        std::fs::write(user_dir.join("print-applet.desktop"), "[Desktop Entry]\nHidden=true\n").unwrap();

        let items = list_from(&user_dir, &[sys_dir]);
        assert_eq!(items.len(), 3);

        // Enabled items come first
        assert!(items[0].enabled);
        assert!(items[1].enabled);
        assert!(!items[2].enabled);

        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"Dropbox"));
        assert!(names.contains(&"System Helper"));
        assert!(names.contains(&"Print Applet"));
    }

    #[test]
    fn toggles_and_removes() {
        let temp = TempDir::new("toggle");
        let user_dir = temp.path().join("user_autostart");
        let sys_dir = temp.path().join("sys_autostart");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::create_dir_all(&sys_dir).unwrap();

        std::fs::write(
            user_dir.join("app.desktop"),
            "[Desktop Entry]\nName=App\nExec=app\nType=Application\n",
        )
        .unwrap();

        // Disable
        set_enabled_in(&user_dir, "app.desktop", false, false).unwrap();
        let items = list_from(&user_dir, &[sys_dir.clone()]);
        assert_eq!(items.len(), 1);
        assert!(!items[0].enabled);

        // Re-enable
        set_enabled_in(&user_dir, "app.desktop", false, true).unwrap();
        let items = list_from(&user_dir, &[sys_dir.clone()]);
        assert_eq!(items.len(), 1);
        assert!(items[0].enabled);

        // Remove
        remove_in(&user_dir, "app.desktop", false).unwrap();
        let items = list_from(&user_dir, &[sys_dir]);
        assert!(items.is_empty());
    }

    #[test]
    fn lists_system_and_user_items() {
        let items = list();
        assert!(!items.is_empty());
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"1Password"));
        assert!(names.contains(&"Dropbox"));
    }
}
