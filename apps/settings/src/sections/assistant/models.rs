//! The language model: which one answers, the ones on this computer, and new ones from
//! Hugging Face.

use super::{client, dim, size};
use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::BufRead;

/// A download in progress, by URL. Kept outside the page so a redraw doesn't lose it.
#[derive(Clone, Default)]
struct Progress {
    bytes: u64,
    total: u64,
    speed: f64,
}

thread_local! {
    static DOWNLOADS: RefCell<HashMap<String, Progress>> = RefCell::default();
    /// The progress bars showing each download (the page may be redrawn meanwhile).
    static BARS: RefCell<Vec<(String, glib::WeakRef<gtk::ProgressBar>)>> = RefCell::default();
}

enum Update {
    Progress(String, Progress),
    Done(String, Result<(), String>),
}

/// Download each URL in turn (a model, then its projector) with `nixe-assistant models download`.
fn download(urls: Vec<String>, name: String) {
    let urls: Vec<String> = urls.into_iter().filter(|u| !DOWNLOADS.with(|d| d.borrow().contains_key(u))).collect();
    if urls.is_empty() {
        return;
    }
    for u in &urls {
        DOWNLOADS.with(|d| d.borrow_mut().insert(u.clone(), Progress::default()));
    }
    let (tx, rx) = std::sync::mpsc::channel::<Update>();
    std::thread::spawn(move || {
        for url in urls {
            let result = (|| -> Result<(), String> {
                let mut child = std::process::Command::new(client::PROGRAM)
                    .args(["models", "download", &url])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let mut failed = None;
                for line in std::io::BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    if let Some(e) = v["error"].as_str() {
                        failed = Some(e.to_string());
                    }
                    let p = Progress { bytes: v["bytes"].as_u64().unwrap_or(0), total: v["total"].as_u64().unwrap_or(0), speed: v["speed"].as_f64().unwrap_or(0.0) };
                    let _ = tx.send(Update::Progress(url.clone(), p));
                }
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                match failed {
                    Some(e) => Err(e),
                    None if !out.status.success() => Err(String::from_utf8_lossy(&out.stderr).rsplit("Error: ").next().unwrap_or("").trim().to_string()),
                    None => Ok(()),
                }
            })();
            let stop = result.is_err();
            let _ = tx.send(Update::Done(url, result));
            if stop {
                break;
            }
        }
    });
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        while let Ok(u) = rx.try_recv() {
            match u {
                Update::Progress(url, p) => {
                    show(&url, &p);
                    DOWNLOADS.with(|d| d.borrow_mut().insert(url, p));
                }
                Update::Done(url, result) => {
                    DOWNLOADS.with(|d| d.borrow_mut().remove(&url));
                    if let Err(e) = result {
                        window::toast(&format!("Couldn't download {name}: {e}"));
                        // The rest of this batch was skipped.
                        DOWNLOADS.with(|d| d.borrow_mut().clear());
                        window::rebuild("assistant-models");
                        return glib::ControlFlow::Break;
                    }
                    if DOWNLOADS.with(|d| d.borrow().is_empty()) {
                        window::toast(&format!("Downloaded {name}."));
                        window::rebuild("assistant-models");
                        return glib::ControlFlow::Break;
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn show(url: &str, p: &Progress) {
    BARS.with(|b| {
        b.borrow_mut().retain(|(_, w)| w.upgrade().is_some());
        for (u, w) in b.borrow().iter() {
            if u == url
                && let Some(bar) = w.upgrade()
            {
                if p.total > 0 {
                    bar.set_fraction(p.bytes as f64 / p.total as f64);
                }
                bar.set_text(Some(&format!("{} of {} · {}/s", size(p.bytes), size(p.total), size(p.speed as u64))));
            }
        }
    });
}

/// A Download button, or a progress bar while that file downloads.
fn download_control(urls: Vec<String>, name: &str) -> gtk::Widget {
    if let Some(p) = DOWNLOADS.with(|d| urls.iter().find_map(|u| d.borrow().get(u).cloned().map(|p| (u.clone(), p)))) {
        let bar = gtk::ProgressBar::new();
        bar.set_show_text(true);
        bar.set_width_request(220);
        bar.set_valign(gtk::Align::Center);
        // The projector follows the model in the same bar.
        for u in &urls {
            BARS.with(|b| b.borrow_mut().push((u.clone(), bar.downgrade())));
        }
        show(&p.0, &p.1);
        return bar.upcast();
    }
    let b = gtk::Button::with_label("Download");
    let name = name.to_string();
    b.connect_clicked(move |_| {
        download(urls.clone(), name.clone());
        window::rebuild("assistant-models");
    });
    b.upcast()
}

fn use_model(path: String) {
    cmd::background(move || client::run(&["models", "use", &path]), |r| {
        match r {
            Ok(_) => window::toast("Switched models. The assistant restarted to load it."),
            Err(e) => window::toast(&format!("Couldn't switch: {}", e.to_string().rsplit("Error: ").next().unwrap_or(""))),
        }
        window::rebuild("assistant-models");
    });
}

fn use_button(path: &str) -> gtk::Button {
    let b = gtk::Button::with_label("Use");
    let path = path.to_string();
    b.connect_clicked(move |b| {
        b.set_sensitive(false);
        use_model(path.clone());
    });
    b
}

fn files_list(repo: String, into: gtk::Box) {
    into.append(&dim("Looking…"));
    cmd::background(move || client::json(&["models", "files", &repo, "--json"]), move |files| {
        while let Some(c) = into.first_child() {
            into.remove(&c);
        }
        let files = files.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if files.is_empty() {
            into.append(&dim("No single-file GGUF models here."));
        }
        for f in files {
            let (Some(name), Some(url)) = (f["name"].as_str(), f["url"].as_str()) else { continue };
            let desc = format!("{}{}", size(f["bytes"].as_u64().unwrap_or(0)), if f["projector"] == true { " · vision projector" } else { "" });
            into.append(&widgets::row(name, &desc, Some(&download_control(vec![url.to_string()], name))));
        }
    });
}

fn search(query: String, results: gtk::Box) {
    while let Some(c) = results.first_child() {
        results.remove(&c);
    }
    results.append(&dim("Searching…"));
    cmd::background(move || client::json(&["models", "search", &query, "--json"]), move |found| {
        while let Some(c) = results.first_child() {
            results.remove(&c);
        }
        let found = found.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if found.is_empty() {
            results.append(&dim("Nothing found."));
        }
        for m in found.iter().take(15) {
            let Some(repo) = m["id"].as_str() else { continue };
            let desc = format!("{} downloads · {} likes", m["downloads"].as_u64().unwrap_or(0), m["likes"].as_u64().unwrap_or(0));
            let (wrapper, content) = widgets::disclosure(repo, &desc);
            let repo = repo.to_string();
            if let Some(button) = wrapper.first_child().and_downcast::<gtk::Button>() {
                button.connect_clicked(move |_| {
                    if content.first_child().is_none() {
                        files_list(repo.clone(), content.clone());
                    }
                });
            }
            results.append(&wrapper);
        }
    });
}

pub fn build(page: &Page) {
    let Some(list) = client::json(&["models", "list", "--json"]) else {
        page.banner("Couldn't list the models. Update Nixe Assistant (./install.sh assistant) and try again.", true);
        return;
    };

    let g = page.group("In use");
    let current = list["current"].as_str().unwrap_or("");
    let name = current.rsplit('/').next().unwrap_or("none");
    let vision = if list["projector"].is_null() { "Can't see the screen." } else { "Can look at the screen." };
    g.add(&widgets::row(name, vision, None));

    let g = page.group("On this computer");
    for m in list["installed"].as_array().cloned().unwrap_or_default() {
        let Some(path) = m["path"].as_str() else { continue };
        if m["current"] == true {
            continue;
        }
        let mut desc = size(m["bytes"].as_u64().unwrap_or(0));
        if m["vision"] == true {
            desc.push_str(" · vision");
        }
        desc.push_str(&format!(" · {}", gtk::glib::markup_escape_text(path.rsplit_once('/').map(|p| p.0).unwrap_or(""))));
        g.add(&widgets::row(m["name"].as_str().unwrap_or(path), &desc, Some(use_button(path).upcast_ref())));
    }
    g.note("Switching restarts the assistant.");

    let g = page.group("Recommended");
    for m in list["curated"].as_array().cloned().unwrap_or_default() {
        let title = m["name"].as_str().unwrap_or("");
        let desc = format!(
            "{} · {} · about {:.1} GB of graphics memory",
            m["description"].as_str().unwrap_or(""),
            m["size_str"].as_str().unwrap_or(""),
            m["vram_gb"].as_f64().unwrap_or(0.0)
        );
        let control: gtk::Widget = if m["installed"] == true {
            let path = list["installed"]
                .as_array()
                .and_then(|a| a.iter().find(|i| i["name"] == m["filename"]).and_then(|i| i["path"].as_str().map(str::to_string)));
            match path {
                Some(p) if p == current => widgets::tag("In use").upcast(),
                Some(p) => use_button(&p).upcast(),
                None => widgets::tag("Installed").upcast(),
            }
        } else {
            let mut urls = vec![m["download_url"].as_str().unwrap_or("").to_string()];
            if let Some(p) = m["mmproj_url"].as_str() {
                urls.push(p.to_string());
            }
            download_control(urls, title)
        };
        g.add(&widgets::row(title, &gtk::glib::markup_escape_text(&desc), Some(&control)));
    }
    g.note("The graphics card also holds the speech and voice models, and the conversation's context.");

    let g = page.group("Find more on Hugging Face");
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Search GGUF models, e.g. qwen 3.5"));
    entry.set_hexpand(true);
    let results = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let r = results.clone();
    entry.connect_activate(move |e| {
        let q = e.text().trim().to_string();
        if !q.is_empty() {
            search(q, r.clone());
        }
    });
    g.add(&widgets::stacked_row("", "Press Enter to search. Models download to the assistant's models folder.", entry.upcast_ref()));
    g.add(&results);
    widgets::keywords("model llm gguf download hugging face huggingface gemma qwen glm vision");
}
