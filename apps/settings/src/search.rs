//! How search words match settings: spelling variants and everyday names.

/// Words people search with, and the words the settings themselves use.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("volume", &["sound", "audio"]),
    ("sound", &["audio", "volume"]),
    ("speaker", &["sound", "audio", "output"]),
    ("mic", &["microphone", "input"]),
    ("dark", &["theme", "appearance"]),
    ("light", &["theme", "appearance", "night light"]),
    ("mode", &["theme", "profile"]),
    ("color", &["colour"]),
    ("colour", &["color"]),
    ("wallpaper", &["background"]),
    ("background", &["wallpaper"]),
    ("brightness", &["backlight"]),
    ("sleep", &["suspend", "idle"]),
    ("suspend", &["sleep"]),
    ("monitor", &["display", "screen"]),
    ("display", &["monitor", "screen"]),
    ("screen", &["display", "monitor"]),
    ("hotkey", &["shortcut", "keybinding"]),
    ("hotkeys", &["shortcut", "keybinding"]),
    ("keybind", &["shortcut", "keybinding"]),
    ("autostart", &["startup"]),
    ("touchpad", &["trackpad"]),
    ("trackpad", &["touchpad"]),
    ("internet", &["network", "wifi"]),
    ("password", &["passwd", "sign-in"]),
    ("user", &["account"]),
    ("account", &["user"]),
    ("battery", &["power"]),
    ("update", &["upgrade", "updates"]),
    ("upgrade", &["update"]),
    ("app", &["application", "software"]),
    ("program", &["app", "software", "startup"]),
    ("lock", &["lock screen", "idle"]),
    ("font", &["fonts", "text size"]),
    ("headphones", &["sound", "bluetooth", "output"]),
    ("timezone", &["time zone"]),
];

/// Lower case, with hyphens and dots dropped, so "Wi-Fi", "wifi" and "wi fi" agree.
pub fn normalise(text: &str) -> String {
    text.to_lowercase().replace(['-', '.', '\u{2011}'], "")
}

/// The query's words, each with what else it may match. Joined pairs count as one
/// word too: "wi fi" also looks for "wifi".
pub fn terms(query: &str) -> Vec<Vec<String>> {
    let words: Vec<String> = normalise(query).split_whitespace().map(String::from).collect();
    words
        .iter()
        .map(|w| {
            let mut alts = vec![w.clone()];
            if let Some((_, more)) = SYNONYMS.iter().find(|(k, _)| *k == w.as_str()) {
                alts.extend(more.iter().map(|m| normalise(m)));
            }
            // A plural finds the singular.
            if w.len() > 3 && w.ends_with('s') {
                alts.push(w[..w.len() - 1].to_string());
            }
            alts
        })
        .collect::<Vec<_>>()
        .into_iter()
        .enumerate()
        .fold(Vec::new(), |mut out: Vec<Vec<String>>, (i, alts)| {
            // "wi" + "fi": if the joined word matches, the pair is one term.
            if i > 0 && words[i - 1].len() <= 4 && words[i].len() <= 4 {
                let joined = format!("{}{}", words[i - 1], words[i]);
                if let Some(prev) = out.last_mut() {
                    prev.push(joined.clone());
                }
                let mut alts = alts;
                alts.push(joined);
                out.push(alts);
            } else {
                out.push(alts);
            }
            out
        })
}

/// Every term is found in `text` (already `normalise`d), by any of its variants.
pub fn matches(text: &str, terms: &[Vec<String>]) -> bool {
    !terms.is_empty() && terms.iter().all(|alts| alts.iter().any(|a| text.contains(a.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(text: &str, q: &str) -> bool {
        matches(&normalise(text), &terms(q))
    }

    #[test]
    fn spelling_variants() {
        assert!(m("Join Wi-Fi networks", "wifi"));
        assert!(m("Join Wi-Fi networks", "wi fi"));
        assert!(m("Join Wi-Fi networks", "WI-FI"));
        assert!(!m("Bluetooth", "wifi"));
    }

    #[test]
    fn everyday_names() {
        assert!(m("Sound: output device", "volume"));
        assert!(m("Theme and wallpaper", "dark mode"));
        assert!(m("Keyboard shortcuts", "hotkeys"));
        assert!(m("Trackpad gestures", "touchpad"));
        assert!(m("Monitors", "monitor"));
        assert!(m("printers", "printers"));
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(!m("anything", "   "));
    }
}
