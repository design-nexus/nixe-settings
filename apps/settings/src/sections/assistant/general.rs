//! Whether the assistant runs, who it is, how you talk to it, how much it does on its own,
//! and its helpers (coding agent, web search, location).

use super::{choice, client, save, slider, switch};
use crate::widgets::{self, Page};
use crate::{cmd, paths, window};
use gtk::prelude::*;

fn health(status: Option<&serde_json::Value>) -> String {
    let Some(s) = status else {
        return if client::running() { "Starting…".into() } else { "Stopped".into() };
    };
    let model = s["model"].as_str().and_then(|p| p.rsplit('/').next()).unwrap_or("no model");
    let state = if s["model_loaded"] == true { "loaded" } else { "loads when you speak" };
    let muted = if s["muted"] == true { " · muted" } else { "" };
    format!("Running · {model} ({state}){muted}")
}

/// Start, stop or restart the service, then redraw the page.
fn service_button(label: &str, action: &'static str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.connect_clicked(move |b| {
        b.set_sensitive(false);
        cmd::background(move || client::service(action), move |r| {
            if let Err(e) = r {
                window::toast(&format!("Couldn't {action} the assistant: {e}"));
            }
            window::rebuild("assistant");
        });
    });
    b
}

/// Run `nyx-assistant ARGS…` off the main thread; redraw the page when it's done.
fn run_then_redraw(args: Vec<String>, failed: &'static str) {
    cmd::background(
        move || client::run(&args.iter().map(String::as_str).collect::<Vec<_>>()),
        move |r| {
            if let Err(e) = r {
                window::toast(&format!("{failed}: {e}"));
            }
            window::rebuild("assistant");
        },
    );
}

/// (id, name) of each saved persona.
fn personas() -> Vec<(String, String)> {
    let dir = paths::config_home().join("nyx-assistant/personas");
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "toml" {
                return None;
            }
            let id = p.file_stem()?.to_string_lossy().to_string();
            let name = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| t.parse::<toml::Table>().ok())
                .and_then(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
                .unwrap_or_else(|| id.clone());
            Some((id, name))
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The keys bound to the assistant, as Hyprland reports them: (what, keys).
fn binds() -> Vec<(String, String)> {
    let Ok(text) = cmd::run(&["hyprctl", "binds", "-j"]) else { return Vec::new() };
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let mods = |m: u64| {
        [(64, "Super"), (4, "Ctrl"), (8, "Alt"), (1, "Shift")].iter().filter(|(bit, _)| m & bit != 0).map(|(_, n)| *n).collect::<Vec<_>>()
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
                    keys.push(b["key"].as_str().unwrap_or(""));
                    Some((d.to_string(), keys.join(" + ")))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn build(page: &Page) {
    let Some(cfg) = super::config_or_banner(page) else { return };
    let status = client::status();
    let running = client::running();

    let g = page.group("Assistant");
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if running {
        controls.append(&service_button("Restart", "restart"));
        controls.append(&service_button("Stop", "stop"));
    } else {
        controls.append(&service_button("Start", "start"));
    }
    let name = client::get_str(&cfg, "assistant.name");
    g.add(&widgets::row(&name, &health(status.as_ref()), Some(controls.upcast_ref())));
    let waiting: Vec<String> = status
        .as_ref()
        .and_then(|s| s["restart_required"].as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if !waiting.is_empty() {
        g.note(&format!("Restart to use the new {}.", gtk::glib::markup_escape_text(&waiting.join(", "))));
    }
    if let Some(s) = &status {
        let muted = s["muted"] == true;
        let (r, _) = widgets::switch_row("Microphone", "Off: it doesn't listen, even for its name.", !muted, |on| {
            run_then_redraw(vec![if on { "unmute" } else { "mute" }.into()], "Couldn't change the microphone");
        });
        g.add(&r);
    }
    let (r, _) = widgets::button_row("Check everything", "Runs the assistant's own checks in a terminal: models, audio, browser and search.", "Check", |_| {
        cmd::spawn(&["xdg-terminal-exec", "bash", "-c", "nyx-assistant doctor; echo; read -rp 'Press Enter to close'"]);
    });
    g.add(&r);
    widgets::keywords("start stop restart service mute doctor");

    let g = page.group("Who it is");
    let ps = personas();
    if !ps.is_empty() {
        let current = client::get_str(&cfg, "assistant.persona_id");
        let pick = ps.iter().find(|(id, n)| *id == current || n.eq_ignore_ascii_case(&name)).map(|(id, _)| id.clone()).unwrap_or_default();
        let (r, _) = widgets::choice_row("Persona", "Brings its own name, voice, wake words and manner.", ps, &pick, |id| {
            run_then_redraw(vec!["persona".into(), "set".into(), id], "Couldn't switch persona");
        });
        g.add(&r);
    }
    let (r, _) = widgets::entry_row("Name", "What it calls itself.", &name, "Echo", |text| {
        if !text.trim().is_empty() {
            save("assistant.name", client::quoted(text.trim()));
        }
    });
    g.add(&r);
    let phrases: Vec<String> = client::get(&cfg, "wake_word.phrases").as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let (r, e) = widgets::entry_row("Answers to", "Names it listens for, comma-separated. Spellings a speech model might hear help.", &phrases.join(", "), "echo, ekko", |text| {
        let items: Vec<String> = text.split(',').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty()).collect();
        save("wake_word.phrases", client::list(&items));
    });
    e.set_width_chars(30);
    g.add(&r);
    widgets::keywords("persona name wake phrases aliases");

    let g = page.group("Talking");
    g.add(&switch(&cfg, "wake_word.enabled", "Wake word", &format!("Start listening when you say “{}”.", gtk::glib::markup_escape_text(&name))));
    g.add(&switch(&cfg, "conversation.enabled", "Conversation", "Keep listening after an answer, so you can reply without the wake word."));
    g.add(&slider(&cfg, "conversation.follow_up_secs", "Listen for a reply for", "0 stops listening straight after the answer.", (0.0, 60.0, 1.0), 0, " s"));
    g.add(&slider(&cfg, "conversation.idle_secs", "A conversation ends after", "", (10.0, 600.0, 10.0), 0, " s"));
    g.add(&switch(&cfg, "ui.enabled", "Overlay", "Show what the assistant hears, says and asks on screen."));
    widgets::keywords("hey wake follow up hud overlay");

    let g = page.group("Tasks");
    g.add(&choice(
        &cfg,
        "agent.mode",
        "Works on its own",
        "How much the assistant does without asking first.",
        &[("autonomous", "Everything allowed"), ("assisted", "Small changes only"), ("manual", "Asks before any change")],
    ));
    g.add(&slider(&cfg, "agent.max_calls", "Steps per task", "The most actions one request may take.", (1.0, 32.0, 1.0), 0, ""));
    g.add(&slider(&cfg, "agent.max_active_secs", "Time per task", "Waiting for your approval doesn't count.", (10.0, 600.0, 10.0), 0, " s"));
    g.note("What it may do is set on the Permissions page.");
    widgets::keywords("autonomy autonomous assisted manual agent budget");

    let g = page.group("Helpers");
    g.add(&choice(
        &cfg,
        "agent.default_agent",
        "Coding agent",
        "Which one it starts when you ask for coding help.",
        &[("agy", "Antigravity"), ("claude", "Claude Code"), ("codex", "Codex"), ("cursor", "Cursor")],
    ));
    let search = if client::get_str(&cfg, "browser.search_tool").is_empty() { "builtin" } else { "duckduckgo" };
    let (r, _) = widgets::choice_row(
        "Web search",
        "Where it looks up current facts. Neither needs an account.",
        vec![("builtin".into(), "Built in".into()), ("duckduckgo".into(), "DuckDuckGo".into())],
        search,
        |id| run_then_redraw(vec!["search-provider".into(), id], "Couldn't change web search"),
    );
    g.add(&r);
    g.add(&switch(&cfg, "location.enabled", "Use my location", "An approximate city, for the weather. No GPS."));
    let (r, _) = widgets::entry_row("City", "Set one to use it instead of the automatic city.", &client::get_str(&cfg, "location.city"), "Automatic", |text| {
        save("location.city", client::quoted(text.trim()));
    });
    g.add(&r);
    widgets::keywords("coding agent claude codex cursor antigravity search duckduckgo location city weather");

    let keys = binds();
    if !keys.is_empty() {
        let g = page.group("Keys");
        g.note("Change them on Keyboard Shortcuts.");
        for (what, combo) in keys {
            g.add(&widgets::info_row(&what, &combo).0);
        }
        widgets::keywords("shortcut hotkey push to talk keybinding");
    }
}
