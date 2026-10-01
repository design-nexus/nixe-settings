//! `watch`: the background service that puts saved settings back whenever a
//! device connects.

use crate::devices::{self, Discovered};
use crate::hidpp::device::Info;
use crate::hidpp::hidraw::NodeInfo;
use crate::hidpp::link::Link;
use crate::hidpp::receiver::{self, Notice};
use crate::state;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn apply_saved(link: &mut Link, info: &Info) {
    if let Some(s) = state::load().devices.get(&info.key()) {
        let errors = state::apply(link, info, s);
        if errors.is_empty() {
            println!("settings-logitech: applied saved settings to {}", info.name);
        }
        for e in errors {
            eprintln!("settings-logitech: {}: {e}", info.name);
        }
    }
}

fn open_and_discover(node: &NodeInfo) -> anyhow::Result<(Link, Discovered)> {
    let mut link = devices::open(node)?;
    let d = devices::discover(&mut link, node)?;
    Ok((link, d))
}

fn accent() -> Option<String> {
    crate::files::theme_accent()
}

/// Serve one node until it's unplugged.
fn serve(node: NodeInfo) {
    let (mut link, d) = match open_and_discover(&node) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("settings-logitech: {}: {e}", node.path.display());
            return;
        }
    };
    let mut online: Vec<Info> = d.devices.iter().filter(|f| f.online).map(|f| f.info.clone()).collect();
    let mut known: Vec<Info> = d.devices.iter().map(|f| f.info.clone()).collect();
    for info in &online {
        apply_saved(&mut link, info);
    }
    let mut last_accent = accent();
    loop {
        let events = match link.listen(1000) {
            Ok(e) => e,
            Err(_) => return,
        };
        for r in events {
            match receiver::parse_notice(&r) {
                Some(Notice::Connection { slot, online: true, wpid, kind }) if d.receiver.is_some() => {
                    if online.iter().any(|i| i.index == slot) {
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(300));
                    let base = known.iter().find(|i| i.index == slot).cloned().unwrap_or(Info { index: slot, wpid, kind, ..Default::default() });
                    if let Ok(f) = devices::identify(&mut link, slot, base.wpid, &base.serial, &base.name, base.kind) {
                        apply_saved(&mut link, &f.info);
                        known.retain(|i| i.index != slot);
                        known.push(f.info.clone());
                        online.push(f.info);
                    }
                }
                Some(Notice::Connection { slot, online: false, .. }) => online.retain(|i| i.index != slot),
                Some(Notice::Unpaired { slot }) => {
                    online.retain(|i| i.index != slot);
                    known.retain(|i| i.index != slot);
                }
                _ => {}
            }
        }
        // Lighting that follows the Omarchy theme changes with it.
        let now = accent();
        if now != last_accent {
            last_accent = now;
            let store = state::load();
            for info in &online {
                if let Some(s) = store.devices.get(&info.key())
                    && s.lighting.values().any(|z| z.colour == "theme")
                {
                    let only_lighting = state::Saved { lighting: s.lighting.clone(), ..Default::default() };
                    state::apply(&mut link, info, &only_lighting);
                }
            }
        }
    }
}

pub fn watch() -> anyhow::Result<()> {
    let active: Arc<Mutex<HashSet<String>>> = Arc::default();
    loop {
        for node in devices::scan() {
            let key = node.path.display().to_string();
            if !active.lock().unwrap().insert(key.clone()) {
                continue;
            }
            let active = active.clone();
            std::thread::spawn(move || {
                serve(node);
                active.lock().unwrap().remove(&key);
            });
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
