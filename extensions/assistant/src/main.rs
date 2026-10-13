//! The voice assistant (nixe-assistant): running or not, persona and name,
//! voice, wake word, conversation, what it may do, its helpers, and its keys.
//!
//! Settings change `~/.config/nixe-assistant/config.toml` with toml_edit, so
//! comments and everything else in it stay, then the daemon reloads.

use anyhow::{Context, Result, bail};
use nixe_ext::page::{self, Page, desc, refreshes};
use nixe_ext::{Extension, sys};
use serde_json::{Value, json};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, value};

const PAGE: &str = "assistant";
const CLI: &str = "nixe-assistant";
const UNIT: &str = "nixe-assistantd.service";

/// Kokoro's voices: (id, what it sounds like).
const VOICES: &[(&str, &str)] = &[
    ("af_heart", "Heart · American woman"),
    ("af_bella", "Bella · American woman"),
    ("af_sarah", "Sarah · American woman"),
    ("af_nicole", "Nicole · American woman, soft"),
    ("af_sky", "Sky · American woman"),
    ("am_adam", "Adam · American man"),
    ("am_michael", "Michael · American man"),
    ("bf_emma", "Emma · British woman"),
    ("bf_isabella", "Isabella · British woman"),
    ("bm_george", "George · British man"),
    ("bm_lewis", "Lewis · British man"),
];

const PERMISSIONS: &[(&str, &str, &str)] = &[
    ("launch_app", "Open apps", "Starting programs you name."),
    ("volume", "Change the volume", ""),
    ("files_read", "Read files", ""),
    ("files_write", "Change files", ""),
    ("shell", "Run commands", "Terminal commands it writes itself."),
    ("shutdown", "Shut down or restart", ""),
];

fn config_dir() -> PathBuf {
    let new = sys::home().join(".config/nixe-assistant");
    if new.exists() { new } else { sys::home().join(".config/omarchy-assistant") }
}

fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn load() -> Result<DocumentMut> {
    let text = std::fs::read_to_string(config_path()).context("the assistant has no config.toml yet: run it once")?;
    text.parse::<DocumentMut>().context("config.toml isn't valid TOML")
}

fn get<'a>(doc: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for part in path.split('.') {
        item = item.get(part)?;
    }
    Some(item)
}

fn get_str(doc: &DocumentMut, path: &str) -> String {
    get(doc, path).and_then(|i| i.as_str()).unwrap_or("").to_string()
}
fn get_bool(doc: &DocumentMut, path: &str, default: bool) -> bool {
    get(doc, path).and_then(|i| i.as_bool()).unwrap_or(default)
}
fn get_num(doc: &DocumentMut, path: &str, default: f64) -> f64 {
    get(doc, path).and_then(|i| i.as_float().or_else(|| i.as_integer().map(|n| n as f64))).unwrap_or(default)
}

/// Set `section.key` to `v`, making the section if it's missing, and save.
fn save(path: &str, v: Item) -> Result<()> {
    let mut doc = load()?;
    let (section, key) = path.split_once('.').context("bad key")?;
    if doc.get(section).is_none() {
        doc[section] = toml_edit::table();
    }
    doc[section][key] = v;
    let file = config_path();
    let tmp = file.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string())?;
    std::fs::rename(&tmp, &file)?;
    Ok(())
}

fn running() -> bool {
    sys::run("systemctl", &["--user", "is-active", "--quiet", UNIT]).is_ok()
}

/// Ask the daemon to read its config again (when it's running).
fn reload() {
    if running() {
        let _ = sys::run(CLI, &["reload"]);
    }
}

/// (id, name) of each saved persona.
fn personas() -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(config_dir().join("personas")) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "toml" {
                return None;
            }
            let id = p.file_stem()?.to_string_lossy().to_string();
            let text = std::fs::read_to_string(&p).ok()?;
            let name = text.parse::<DocumentMut>().ok().and_then(|d| d.get("name").and_then(|n| n.as_str()).map(String::from)).unwrap_or_else(|| id.clone());
            Some((id, name))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The keys bound to the assistant, as Hyprland reports them.
fn binds() -> Vec<(String, String)> {
    let Ok(text) = sys::run("hyprctl", &["binds", "-j"]) else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let mods = |m: u64| {
        let mut s = Vec::new();
        for (bit, name) in [(64, "Super"), (4, "Ctrl"), (8, "Alt"), (1, "Shift")] {
            if m & bit != 0 {
                s.push(name);
            }
        }
        s
    };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let d = b["description"].as_str()?;
                    let low = d.to_lowercase();
                    if !(low.contains("assistant") || low.contains("dictation") || low.contains("ptt")) {
                        return None;
                    }
                    let mut keys = mods(b["modmask"].as_u64().unwrap_or(0));
                    let key = b["key"].as_str().unwrap_or("");
                    keys.push(key);
                    Some((d.to_string(), keys.join(" + ")))
                })
                .collect()
        })
        .unwrap_or_default()
}

struct Assistant;

impl Extension for Assistant {
    fn pages(&self) -> Value {
        if !sys::present(CLI) && !sys::home().join(".local/bin").join(CLI).exists() {
            return json!([]);
        }
        json!([page::entry(PAGE, "Assistant", "audio-input-microphone-symbolic", "The voice assistant: voice, wake word, conversation and what it may do.",
            "assistant voice nixe echo persona wake word push to talk speech kokoro parakeet permissions")])
    }

    fn describe(&self, _id: &str) -> Result<Value> {
        let doc = load()?;
        let on = running();
        let mut status = vec![desc(
            page::switch("run", "Running", on),
            "Off frees the memory its speech and language models use. It starts again at sign-in while this is on.",
        )];
        if on && let Ok(s) = sys::run(CLI, &["status", "--json"]) {
            let v: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
            let loaded = v["model_loaded"].as_bool().unwrap_or(false);
            status.push(page::info("model", "Language model", if loaded { "Loaded" } else { "Loading…" }));
            status.push(page::buttons("mute", "Microphone", &[("mute", "Mute"), ("unmute", "Listen")]));
        }
        status.push(desc(page::button("doctor", "Check everything", "Check"), "Runs the assistant's own checks in a terminal: models, audio, browser and search."));

        let mut who = Vec::new();
        let ps = personas();
        if !ps.is_empty() {
            let current = get_str(&doc, "assistant.persona_id");
            let name = get_str(&doc, "assistant.name");
            let pick = ps.iter().find(|(id, n)| *id == current || n.eq_ignore_ascii_case(&name)).map(|(id, _)| id.clone()).unwrap_or_default();
            who.push(refreshes(desc(page::choice_owned("persona", "Persona", &pick, &ps), "A persona brings its own name, voice, wake words and manner.")));
        }
        who.push(desc(page::entry_row("assistant.name", "Name", &get_str(&doc, "assistant.name"), "Echo"), "What it calls itself."));

        let voice = get_str(&doc, "tts.voice");
        let voices: Vec<(String, String)> = VOICES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let speech = vec![
            page::choice_owned("tts.voice", "Voice", &voice, &voices),
            page::slider("tts.speed", "Speed", get_num(&doc, "tts.speed", 1.0), (0.6, 1.6, 0.05), "×"),
            page::button("preview", "Hear it", "Preview"),
        ];

        let phrases: Vec<String> =
            get(&doc, "wake_word.phrases").and_then(|i| i.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let wake = vec![
            desc(page::switch("wake_word.enabled", "Wake word", get_bool(&doc, "wake_word.enabled", true)), "Say its name to start, without the key."),
            desc(page::entry_row("wake_word.phrases", "Answers to", &phrases.join(", "), "echo, ekko"), "Names it listens for, separated by commas. Spellings a speech model might hear help."),
        ];

        let talk = vec![
            desc(page::switch("conversation.enabled", "Follow-up questions", get_bool(&doc, "conversation.enabled", true)), "After an answer it listens a little longer, so you can ask again without its name."),
            page::slider("conversation.follow_up_secs", "Listen after an answer", get_num(&doc, "conversation.follow_up_secs", 10.0), (3.0, 30.0, 1.0), " s"),
            page::slider("conversation.idle_secs", "A conversation ends after", get_num(&doc, "conversation.idle_secs", 60.0), (15.0, 300.0, 15.0), " s"),
            desc(page::switch("barge_in.enabled", "Interrupt it by talking", get_bool(&doc, "barge_in.enabled", true)), "Speaking while it talks stops it."),
        ];

        let perms: Vec<Value> = PERMISSIONS
            .iter()
            .map(|(k, title, d)| {
                let path = format!("permissions.{k}");
                let mut row = page::choice(&path, title, &get_str(&doc, &path), &[("allow", "Yes"), ("confirm", "Ask first"), ("deny", "Never")]);
                row["kind"] = json!("segmented");
                desc(row, d)
            })
            .collect();

        let agent = get_str(&doc, "agent.default_agent");
        let search = if get_str(&doc, "browser.search_tool").is_empty() { "builtin" } else { "duckduckgo" };
        let helpers = vec![
            desc(
                page::choice("agent.default_agent", "Coding agent", &agent, &[("agy", "Antigravity"), ("claude", "Claude Code"), ("codex", "Codex"), ("cursor", "Cursor")]),
                "Which one it starts when you ask for coding help.",
            ),
            desc(refreshes(page::segmented("search", "Web search", search, &[("builtin", "Built in"), ("duckduckgo", "DuckDuckGo")])), "Where it looks up current facts. Neither needs an account."),
            desc(page::switch("location.enabled", "Use my location", get_bool(&doc, "location.enabled", true)), "An approximate city, for the weather. No GPS."),
            desc(page::entry_row("location.city", "City", &get_str(&doc, "location.city"), "Automatic"), "Set one to use it instead of the automatic city."),
        ];

        let keys: Vec<Value> = binds().iter().map(|(d, k)| page::info(d, d, k)).collect();

        Ok(Page::new(&format!("<b>{}</b> · {}", get_str(&doc, "assistant.name"), if on { "running" } else { "stopped" }))
            .group("Status", "", status)
            .group("Who it is", "", who)
            .group("Voice", "", speech)
            .group("Wake word", "", wake)
            .group("Conversation", "", talk)
            .group("What it may do", "“Ask first” shows the exact command or click for you to approve.", perms)
            .group("Helpers", "", helpers)
            .group("Keys", "Change them on Keyboard Shortcuts.", keys)
            .build())
    }

    fn set(&self, _page: &str, key: &str, v: &str) -> Result<Option<Value>> {
        match key {
            "run" => {
                sys::run(CLI, &[if v == "true" { "start" } else { "stop" }])?;
                return Ok(nixe_ext::refresh());
            }
            "mute" => {
                sys::run(CLI, &[if v == "mute" { "mute" } else { "unmute" }])?;
                return Ok(nixe_ext::toast(if v == "mute" { "Not listening" } else { "Listening" }));
            }
            "doctor" => {
                std::process::Command::new("xdg-terminal-exec").args(["bash", "-c", "nixe-assistant doctor; echo; read -rp 'Press Enter to close'"]).spawn()?;
                return Ok(None);
            }
            "preview" => {
                if !running() {
                    bail!("Start the assistant first");
                }
                sys::run(CLI, &["preview"])?;
                return Ok(None);
            }
            "persona" => {
                sys::run(CLI, &["persona", "set", v])?;
                return Ok(nixe_ext::refresh());
            }
            "search" => {
                sys::run(CLI, &["search-provider", if v == "duckduckgo" { "duckduckgo" } else { "builtin" }])?;
                return Ok(nixe_ext::refresh());
            }
            "tts.speed" => save(key, value((v.parse::<f64>()? * 100.0).round() / 100.0))?,
            "conversation.follow_up_secs" | "conversation.idle_secs" => save(key, value(v.parse::<f64>()?.round() as i64))?,
            "wake_word.phrases" => {
                let mut arr = toml_edit::Array::new();
                for p in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    arr.push(p.to_lowercase());
                }
                save(key, value(arr))?;
            }
            k if k.ends_with(".enabled") => save(k, value(v == "true"))?,
            "assistant.name" | "location.city" => save(key, value(v.trim()))?,
            k if k.starts_with("permissions.") => {
                if !["allow", "confirm", "deny"].contains(&v) {
                    bail!("not a choice");
                }
                save(k, value(v))?;
            }
            "tts.voice" => {
                if !VOICES.iter().any(|(id, _)| *id == v) {
                    bail!("not a voice it has");
                }
                save(key, value(v))?;
            }
            "agent.default_agent" => save(key, value(v))?,
            _ => bail!("unknown setting {key}"),
        }
        reload();
        Ok(None)
    }
}

fn main() {
    nixe_ext::run(&Assistant);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_the_rest() {
        let text = "[assistant]\nname = \"Echo\" # who\n\n[tts]\nvoice = \"af_heart\"\nspeed = 1.0\n";
        let mut doc: DocumentMut = text.parse().unwrap();
        doc["tts"]["speed"] = value(1.25);
        let out = doc.to_string();
        assert!(out.contains("name = \"Echo\" # who"));
        assert!(out.contains("speed = 1.25"));
        assert_eq!(get_str(&doc, "assistant.name"), "Echo");
        assert_eq!(get_num(&doc, "tts.speed", 0.0), 1.25);
        assert!(get_bool(&doc, "nope.x", true));
    }
}
