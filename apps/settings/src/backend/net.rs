//! Wi-Fi through NetworkManager's `nmcli`: nearby networks, saved ones, and
//! joining, leaving and forgetting them.

use crate::cmd;
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0–100.
    pub signal: u8,
    pub secure: bool,
    /// WPA-Enterprise (802.1X), which needs more than a password.
    pub enterprise: bool,
    pub in_use: bool,
}

/// Split one line of `nmcli -t` output, where `:` inside a field is `\:`.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// Parse `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY device wifi list`: one entry
/// per name (the strongest), hidden networks left out, strongest first with the
/// one in use on top.
pub fn parse_networks(text: &str) -> Vec<Network> {
    let mut out: Vec<Network> = Vec::new();
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let security = f[3].trim();
        let n = Network {
            ssid: f[1].clone(),
            signal: f[2].trim().parse().unwrap_or(0),
            secure: !security.is_empty() && security != "--",
            enterprise: security.contains("802.1X"),
            in_use: f[0].trim() == "*",
        };
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) => {
                o.in_use |= n.in_use;
                if n.signal > o.signal {
                    o.signal = n.signal;
                }
            }
            None => out.push(n),
        }
    }
    out.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal)));
    out
}

pub fn networks(rescan: bool) -> Vec<Network> {
    let text = cmd::output(&[
        "nmcli",
        "-t",
        "-f",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        if rescan { "yes" } else { "auto" },
    ])
    .unwrap_or_default();
    parse_networks(&text)
}

/// Saved Wi-Fi connections as (connection name, SSID).
pub fn saved() -> Vec<(String, String)> {
    let list = cmd::output(&["nmcli", "-t", "-f", "NAME,TYPE", "connection", "show"]).unwrap_or_default();
    list.lines()
        .map(split_terse)
        .filter(|f| f.len() >= 2 && f[1] == "802-11-wireless")
        .map(|f| {
            let ssid = cmd::output(&["nmcli", "-g", "802-11-wireless.ssid", "connection", "show", "id", &f[0]])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| f[0].clone());
            (f[0].clone(), ssid)
        })
        .collect()
}

/// The Wi-Fi device in use, if any, e.g. "wlan0".
pub fn wifi_device() -> Option<String> {
    let text = cmd::output(&["nmcli", "-t", "-f", "DEVICE,TYPE,STATE", "device"])?;
    text.lines().map(split_terse).find(|f| f.len() >= 3 && f[1] == "wifi" && f[2] == "connected").map(|f| f[0].clone())
}

/// IP address, gateway and DNS servers of a device.
pub fn parse_details(text: &str) -> Vec<(String, String)> {
    let mut ip = Vec::new();
    let mut gateway = String::new();
    let mut dns = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if value.is_empty() || value == "--" {
            continue;
        }
        if key.starts_with("IP4.ADDRESS") {
            ip.push(value.split('/').next().unwrap_or(value).to_string());
        } else if key == "IP4.GATEWAY" {
            gateway = value.to_string();
        } else if key.starts_with("IP4.DNS") {
            dns.push(value.to_string());
        }
    }
    let mut out = Vec::new();
    if !ip.is_empty() {
        out.push(("IP address".to_string(), ip.join(", ")));
    }
    if !gateway.is_empty() {
        out.push(("Gateway".to_string(), gateway));
    }
    if !dns.is_empty() {
        out.push(("DNS".to_string(), dns.join(", ")));
    }
    out
}

pub fn details(device: &str) -> Vec<(String, String)> {
    parse_details(
        &cmd::output(&["nmcli", "-t", "-f", "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS", "device", "show", device]).unwrap_or_default(),
    )
}

/// Join a network: a saved one by its connection, otherwise as a new one.
pub fn connect(ssid: &str, saved_as: Option<&str>, password: Option<&str>) -> Result<String> {
    match (saved_as, password) {
        (Some(name), None) => cmd::run(&["nmcli", "connection", "up", "id", name]),
        (_, Some(pw)) => cmd::run(&["nmcli", "device", "wifi", "connect", ssid, "password", pw]),
        (None, None) => cmd::run(&["nmcli", "device", "wifi", "connect", ssid]),
    }
}

pub fn disconnect(name: &str) -> Result<String> {
    cmd::run(&["nmcli", "connection", "down", "id", name])
}

pub fn forget(name: &str) -> Result<String> {
    cmd::run(&["nmcli", "connection", "delete", "id", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_escaped_colons() {
        assert_eq!(split_terse(r"*:Cafe\:Guest:72:WPA2"), ["*", "Cafe:Guest", "72", "WPA2"]);
        assert_eq!(split_terse(r"a\\b:c"), [r"a\b", "c"]);
    }

    #[test]
    fn networks_are_deduped_and_sorted() {
        let text = " :Neighbour:40:WPA2\n*:Home:70:WPA2 WPA3\n :Home:55:WPA2 WPA3\n :Cafe:80:\n :Work:60:WPA2 802.1X\n :::--\n";
        let n = parse_networks(text);
        assert_eq!(n.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["Home", "Cafe", "Work", "Neighbour"]);
        assert!(n[0].in_use && n[0].secure && n[0].signal == 70);
        assert!(!n[1].secure, "no security is an open network");
        assert!(n[2].enterprise);
    }

    #[test]
    fn details_are_readable() {
        let text = "IP4.ADDRESS[1]:192.168.1.23/24\nIP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:1.1.1.1\nIP4.DNS[2]:9.9.9.9\n";
        assert_eq!(
            parse_details(text),
            [
                ("IP address".to_string(), "192.168.1.23".to_string()),
                ("Gateway".to_string(), "192.168.1.1".to_string()),
                ("DNS".to_string(), "1.1.1.1, 9.9.9.9".to_string()),
            ]
        );
        assert!(parse_details("IP4.GATEWAY:--\n").is_empty());
    }
}
