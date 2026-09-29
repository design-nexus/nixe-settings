use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::prelude::*;

fn panel(args: &'static [&'static str]) -> gtk::Button {
    widgets::command_button("Open", args)
}

pub fn build(page: &Page) {
    // ----- Wi-Fi -----
    let g = page.group("Wi-Fi");
    if cmd::present("nmcli") {
        let on = cmd::output(&["nmcli", "radio", "wifi"]).is_some_and(|s| s.trim() == "enabled");
        let (r, _) = widgets::switch_row("Wi-Fi", "", on, |on| {
            cmd::run_async(&["nmcli", "radio", "wifi", if on { "on" } else { "off" }], |r| {
                if let Err(e) = r {
                    window::toast(&format!("{e}"));
                }
            });
        });
        widgets::keywords("wireless airplane radio");
        g.add(&r);

        let status = cmd::output(&["nmcli", "-t", "-f", "TYPE,STATE,CONNECTION", "device"]).unwrap_or_default();
        let connected: Vec<String> = status
            .lines()
            .filter_map(|l| {
                let parts: Vec<&str> = l.splitn(3, ':').collect();
                (parts.len() == 3 && (parts[0] == "wifi" || parts[0] == "ethernet") && parts[1] == "connected")
                    .then(|| format!("{} ({})", parts[2], if parts[0] == "wifi" { "Wi-Fi" } else { "Ethernet" }))
            })
            .collect();
        let text = if connected.is_empty() { "Nothing".to_string() } else { connected.join(", ") };
        let (r, _) = widgets::info_row("Connected to", &text);
        g.add(&r);
    }
    g.add(&widgets::row(
        "Networks",
        "Join a network or change its settings. Also <tt>Super Ctrl W</tt>.",
        Some(panel(&["omarchy-shell", "shell", "toggle", "omarchy.network"]).upcast_ref()),
    ));

    // ----- Bluetooth -----
    let g = page.group("Bluetooth");
    if cmd::present("bluetoothctl") {
        let show = cmd::output(&["bluetoothctl", "show"]).unwrap_or_default();
        let powered = show.lines().any(|l| l.trim() == "Powered: yes");
        let (r, _) = widgets::switch_row("Bluetooth", "", powered, |on| {
            cmd::run_async(&["bluetoothctl", "power", if on { "on" } else { "off" }], |r| {
                if let Err(e) = r {
                    window::toast(&format!("{e}"));
                }
            });
        });
        g.add(&r);
        let devices = cmd::output(&["bluetoothctl", "devices", "Connected"]).unwrap_or_default();
        let names: Vec<String> = devices
            .lines()
            .filter_map(|l| l.strip_prefix("Device ").and_then(|r| r.split_once(' ')).map(|(_, n)| n.to_string()))
            .collect();
        let text = if names.is_empty() { "None".to_string() } else { names.join(", ") };
        let (r, _) = widgets::info_row("Connected devices", &text);
        g.add(&r);
    }
    g.add(&widgets::row(
        "Devices",
        "Pair headphones, keyboards and more. Also <tt>Super Ctrl B</tt>.",
        Some(panel(&["omarchy-shell", "shell", "toggle", "omarchy.bluetooth"]).upcast_ref()),
    ));
}
