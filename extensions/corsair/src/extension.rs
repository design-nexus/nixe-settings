//! The Settings extension protocol: `pages`, `describe PAGE`, `set PAGE KEY VALUE`.

use crate::usb::{self, Access, Device};
use crate::{access, models};
use anyhow::{Result, bail};
use nyx_ext::lighting;
use nyx_ext::openrgb;
use serde_json::{Value, json};
use std::process::Command;

/// Pages for Corsair devices OpenRGB finds that aren't one of the keyboards above.
const ORGB: &str = "orgb-";

fn corsair_lights() -> Vec<openrgb::Device> {
    lighting::devices().into_iter().filter(|d| d.vendor.to_lowercase().contains("corsair") || d.name.to_lowercase().contains("corsair")).collect()
}

/// The OpenRGB device for a USB keyboard: same model name.
fn lights_for<'a>(d: &Device, lights: &'a [openrgb::Device]) -> Option<&'a openrgb::Device> {
    let want = title(d).to_lowercase();
    let first = want.split_whitespace().next().unwrap_or("").to_string();
    lights.iter().find(|l| l.kind == 5 && l.name.to_lowercase().contains(&want)).or_else(|| {
        lights.iter().find(|l| l.kind == 5 && !first.is_empty() && l.name.to_lowercase().contains(&first))
    })
}

fn lighting_group(light: Option<&openrgb::Device>) -> Value {
    let mut rows = lighting::setup_rows();
    if rows.is_empty() {
        match light {
            Some(l) => rows = lighting::rows(l),
            None => rows.push(json!({ "kind": "info", "title": "Lighting", "value": "OpenRGB doesn't list this device" ,
                "tooltip": "OpenRGB supports most Corsair keyboards, mice, headsets, memory and fan hubs. A model it doesn't know yet still works with Corsair Web Hub." })),
        }
    }
    json!({ "title": "Lighting", "rows": rows })
}

pub const WEB_HUB: &str = "https://www.corsair.com/web-hub";
const SETUP: &str = "setup";

/// Corsair devices that get a page: known keyboards, and anything else with a keyboard interface.
fn keyboards(all: &[Device]) -> Vec<&Device> {
    all.iter().filter(|d| !models::is_stream_deck(d.product) && (models::by_keyboard(d.product).is_some() || d.has_keyboard())).collect()
}

fn title(d: &Device) -> String {
    match models::by_keyboard(d.product) {
        Some(m) => m.name.trim_start_matches("Corsair ").to_string(),
        None => d.name.clone(),
    }
}

pub fn pages() -> Value {
    let all = usb::scan();
    let boards = keyboards(&all);
    if !boards.is_empty() && boards.iter().all(|d| usb::access(d) == Access::Denied) {
        return json!([{
            "id": SETUP, "title": "Corsair", "icon": "input-keyboard-symbolic",
            "description": "A Corsair keyboard is plugged in, but this account can't talk to it yet.",
            "keywords": "corsair keyboard permission access udev web hub",
        }]);
    }
    let lights = corsair_lights();
    let mut out: Vec<Value> = boards
        .iter()
        .map(|d| {
            json!({
                "id": d.page_id(), "title": title(d), "icon": "input-keyboard-symbolic",
                "description": "Corsair keyboard: lighting, device details and access for Corsair Web Hub.",
                "keywords": format!("corsair keyboard galleon lighting rgb web hub {}", d.name.to_lowercase()),
            })
        })
        .collect();
    // Mice, headsets, memory and fan hubs, and keyboards with no USB page.
    for l in &lights {
        if boards.iter().any(|d| lights_for(d, &lights).is_some_and(|x| x.id() == l.id())) {
            continue;
        }
        out.push(json!({
            "id": format!("{ORGB}{}", l.id()), "title": l.name.trim_start_matches("Corsair ").to_string(), "icon": l.icon(),
            "description": format!("Corsair {}: lighting.", l.kind_name().to_lowercase()),
            "keywords": format!("corsair lighting rgb {} {}", l.kind_name().to_lowercase(), l.name.to_lowercase()),
        }));
    }
    Value::Array(out)
}

fn light_page(id: &str) -> Result<Value> {
    let Some(l) = corsair_lights().into_iter().find(|l| l.id() == id) else { bail!("OpenRGB doesn't see that device any more") };
    let mut rows = vec![json!({ "kind": "info", "title": "Kind", "value": l.kind_name() })];
    if !l.version.is_empty() {
        rows.push(json!({ "kind": "info", "title": "Firmware", "value": l.version }));
    }
    if !l.serial.is_empty() {
        rows.push(json!({ "kind": "info", "title": "Serial", "value": l.serial }));
    }
    Ok(json!({
        "subtitle": format!("<b>{}</b> · {}", gtk_escape(&l.name), l.kind_name()),
        "groups": [lighting_group(Some(&l)), { "title": "Device", "rows": rows }],
    }))
}

fn gtk_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn access_group() -> Value {
    let installed = access::rule_installed();
    json!({ "title": "Device access", "rows": [{
        "kind": "button", "key": "allow", "title": "Allow access to Corsair keyboards",
        "desc": format!(
            "{}Installs <tt>{}</tt>, which lets your account (and Corsair Web Hub in the browser) talk to them. Asks for your password.",
            if installed { "Already set up. " } else { "" },
            access::rule_path().display()
        ),
        "label": if installed { "Set up again" } else { "Allow access" },
        "keywords": "permission udev hidraw access",
    }]})
}

fn setup_page() -> Value {
    json!({
        "banners": [{ "text": "Linux only lets <b>root</b> talk to these devices until a small rule allows your account to.", "warning": true }],
        "groups": [access_group()],
    })
}

fn stream_deck_installed() -> bool {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    data.join("settings/extensions/streamdeck").is_dir()
}

fn stream_deck_running() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "galleon-deck.service"])
        .status()
        .is_ok_and(|s| s.success())
}

pub fn describe(id: &str) -> Result<Value> {
    if id == SETUP {
        return Ok(setup_page());
    }
    if let Some(l) = id.strip_prefix(ORGB) {
        return light_page(l);
    }
    let all = usb::scan();
    let Some(d) = keyboards(&all).into_iter().find(|d| d.page_id() == id).cloned() else {
        bail!("that keyboard isn't plugged in any more");
    };
    let model = models::by_keyboard(d.product);
    let mut groups = Vec::new();
    let mut banners = Vec::new();
    if model.is_none() {
        banners.push(json!({ "text": "Lighting for this keyboard comes from OpenRGB; key remaps and firmware are in Corsair Web Hub.", "warning": false }));
    }
    if d.simulated {
        banners.push(json!({ "text": "Simulated keyboard (<tt>CORSAIR_MOCK</tt>).", "warning": false }));
    }

    // Lighting through OpenRGB; key remaps, dials and firmware in Web Hub.
    let lights = corsair_lights();
    groups.push(lighting_group(lights_for(&d, &lights)));
    groups.push(json!({ "title": "Keys and firmware", "rows": [{
        "kind": "button", "key": "web-hub", "title": "Corsair Web Hub",
        "desc": "Key remaps, the dials and firmware updates, saved on the keyboard itself. Opens in your browser; needs Chrome or Chromium and device access.",
        "label": "Open", "keywords": "remap macro firmware update web hub",
    }]}));

    if let Some(deck) = model.and_then(|m| m.stream_deck) {
        let present = all.iter().any(|x| x.product == deck);
        let value = match (present, stream_deck_installed(), stream_deck_running()) {
            (false, _, _) => "Not detected",
            (true, false, _) => "Install the Stream Deck extension (Settings → Extensions)",
            (true, true, true) => "Running: set it up on the Stream Deck page",
            (true, true, false) => "Stopped: start it on the Stream Deck page",
        };
        groups.push(json!({ "title": "Stream Deck", "rows": [{
            "kind": "info", "title": "Built-in Stream Deck", "value": value,
            "tooltip": "The 12 screen keys, top screen and dials are a separate device, run by the Stream Deck extension.",
            "keywords": "stream deck lcd keys dials screen",
        }]}));
    }

    let mut rows = vec![
        json!({ "kind": "info", "title": "Model", "value": model.map(|m| m.name).unwrap_or(&d.name) }),
        json!({ "kind": "info", "title": "USB id", "value": format!("{:04x}:{:04x}", models::CORSAIR, d.product) }),
        json!({ "kind": "info", "title": "USB release", "value": d.release_text(), "tooltip": "The version number the keyboard reports over USB." }),
    ];
    if !d.serial.is_empty() {
        rows.push(json!({ "kind": "info", "title": "Serial", "value": d.serial }));
    }
    let ifaces: Vec<String> = d.interfaces.iter().map(|i| format!("{}: {}", i.number, i.summary().page_name())).collect();
    rows.push(json!({ "kind": "info", "title": "Interfaces", "value": ifaces.join("\n") }));
    groups.push(json!({ "title": "Device", "rows": rows }));

    if usb::access(&d) != Access::Allowed || !access::rule_installed() {
        groups.push(access_group());
    }
    Ok(json!({ "banners": banners, "groups": groups }))
}

fn probe_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("corsair-probe.txt")
}

pub fn save_probe(path: &std::path::Path) -> Result<()> {
    std::fs::write(path, crate::probe::report(true))?;
    Ok(())
}

pub fn set(page: &str, key: &str, value: &str) -> Result<Option<Value>> {
    if key.starts_with("light/") || key.starts_with("openrgb/") {
        let device = match page.strip_prefix(ORGB) {
            Some(id) => id.to_string(),
            None => {
                let all = usb::scan();
                let lights = corsair_lights();
                keyboards(&all).into_iter().find(|d| d.page_id() == page).and_then(|d| lights_for(d, &lights)).map(|l| l.id()).unwrap_or_default()
            }
        };
        if let Some(reply) = lighting::set(&device, key, value)? {
            return Ok(reply);
        }
    }
    match key {
        "allow" => {
            access::install_rule()?;
            Ok(Some(json!({ "toast": "Access allowed", "reload": true })))
        }
        "web-hub" => {
            Command::new("xdg-open").arg(WEB_HUB).spawn()?;
            Ok(None)
        }
        "probe" => {
            let path = probe_path();
            save_probe(&path)?;
            Ok(Some(json!({ "toast": format!("Saved {}", path.display()) })))
        }
        _ => bail!("unknown setting {key}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock() {
        // SAFETY: tests in this crate only ever set this one value.
        unsafe { std::env::set_var("CORSAIR_MOCK", "galleon-100-sd") };
    }

    #[test]
    fn one_page_for_the_galleon_not_its_deck() {
        mock();
        let p = pages();
        let p = p.as_array().unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["title"], "Galleon 100 SD");
        assert_eq!(p[0]["id"], "2b0c-sim0000galleon");
    }

    #[test]
    fn describes_the_galleon() {
        mock();
        let d = describe("2b0c-sim0000galleon").unwrap();
        let titles: Vec<&str> = d["groups"].as_array().unwrap().iter().map(|g| g["title"].as_str().unwrap()).collect();
        assert_eq!(&titles[..4], ["Lighting", "Keys and firmware", "Stream Deck", "Device"]);
        let device = &d["groups"][3]["rows"];
        assert_eq!(device[1]["value"], "1b1c:2b0c");
        assert!(device[4]["value"].as_str().unwrap().contains("1: Vendor-defined (0xFF42)"));
        assert!(describe("nope").is_err());
        assert!(set("x", "teleport", "").is_err());
    }
}
