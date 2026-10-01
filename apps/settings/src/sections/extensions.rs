//! Extensions: device support for more brands, installed from GitHub.

use crate::ext::{self, Extension, manage};
use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::glib;
use gtk::prelude::*;
use std::cell::Cell;

thread_local! {
    /// The catalog is fetched once per run; after that the page uses the copy on disk.
    static FETCHED: Cell<bool> = const { Cell::new(false) };
}

fn after_change() {
    // An extension may want (or no longer want) to hear about theme changes.
    cmd::background(crate::backend::themehook::ensure, |_| {});
    window::reload_sections(false);
    window::rebuild("extensions");
}

/// Disable a button and show progress while `work` runs on a worker thread.
fn busy<T: Send + 'static>(
    b: &gtk::Button,
    label: &str,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let old = b.label().map(|l| l.to_string()).unwrap_or_default();
    b.set_sensitive(false);
    b.set_label(label);
    let b = b.clone();
    cmd::background(
        move || work().map_err(|e| format!("{e:#}")),
        move |r| {
            b.set_sensitive(true);
            b.set_label(&old);
            match r {
                Ok(v) => done(v),
                Err(e) => window::toast(&e),
            }
        },
    );
}

fn is_asus_laptop() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/sys_vendor").is_ok_and(|v| v.to_uppercase().contains("ASUS"))
}

fn installed_desc(e: &Extension) -> String {
    let m = &e.manifest;
    let mut parts = vec![glib::markup_escape_text(&m.description).to_string()];
    let missing = e.missing();
    if !ext::enabled(e.id()) {
        parts.push("<b>Turned off.</b> Its pages are hidden until you turn it back on.".into());
    } else if !missing.is_empty() {
        let hint = if m.needs_hint.is_empty() {
            format!("Needs <tt>{}</tt>, which isn't installed.", glib::markup_escape_text(&missing.join(", ")))
        } else {
            m.needs_hint.clone()
        };
        parts.push(format!("<b>{hint}</b>"));
    } else {
        match ext::cached_pages(e.id()) {
            Some(p) if !p.is_empty() => {
                let titles: Vec<String> = p.iter().map(|p| glib::markup_escape_text(&p.title).to_string()).collect();
                parts.push(format!("Pages: {}", titles.join(", ")));
            }
            _ => parts.push("No matching devices found right now.".into()),
        }
    }
    if !m.version.is_empty() {
        parts.push(format!("<tt>{}</tt> {}", glib::markup_escape_text(e.id()), glib::markup_escape_text(&m.version)));
    }
    parts.retain(|p| !p.is_empty());
    parts.join("\n")
}

pub fn build(page: &Page) {
    let installed = ext::installed();
    let catalog = manage::catalog(false);
    if !FETCHED.with(|f| f.replace(true)) {
        // Fetch the latest catalog; redraw only if it differs from what's shown.
        let shown = catalog.clone();
        cmd::background(
            || manage::catalog(true),
            move |fresh| {
                if fresh != shown {
                    window::rebuild("extensions");
                }
            },
        );
    }

    if !installed.iter().any(|e| e.id() == "asus") && cmd::present("asusctl") && is_asus_laptop() {
        let b = page.banner(
            "<b>ASUS controls are now an extension.</b> Install it to get the ASUS and Aura Lighting pages back.",
            false,
        );
        let install = gtk::Button::with_label("Install");
        install.add_css_class("suggested-action");
        install.set_valign(gtk::Align::Center);
        install.connect_clicked(|b| {
            busy(b, "Installing…", || manage::install("asus"), |e| {
                window::toast(&format!("Installed {}", e.manifest.name));
                after_change();
            })
        });
        b.append(&install);
    }

    // ----- Installed -----
    if !installed.is_empty() {
        let g = page.group("Installed");
        for e in installed.clone() {
            let controls = widgets::hbox(8);
            let update = gtk::Button::from_icon_name("view-refresh-symbolic");
            update.add_css_class("flat");
            update.set_tooltip_text(Some("Update"));
            let e2 = e.clone();
            update.connect_clicked(move |b| {
                let (e3, name) = (e2.clone(), e2.manifest.name.clone());
                busy(b, "", move || manage::update(&e3), move |changed| {
                    window::toast(&if changed { format!("Updated {name}") } else { format!("{name} is up to date") });
                    if changed {
                        after_change();
                    }
                });
            });
            update.set_visible(e.root.join(".git").exists());
            controls.append(&update);
            let e2 = e.clone();
            let remove = widgets::confirm_button("Remove", "Click again to remove", move |b| {
                let (e3, name) = (e2.clone(), e2.manifest.name.clone());
                busy(b, "Removing…", move || manage::remove(&e3), move |_| {
                    window::toast(&format!("Removed {name}"));
                    after_change();
                });
            });
            remove.add_css_class("destructive-action");
            controls.append(&remove);
            let on = gtk::Switch::new();
            on.set_active(ext::enabled(e.id()));
            on.set_valign(gtk::Align::Center);
            on.set_tooltip_text(Some("Turn off to hide its pages without removing it"));
            let id = e.id().to_string();
            on.connect_active_notify(move |s| {
                manage::set_enabled(&id, s.is_active());
                // This page is rebuilt with the new state; not from inside the switch's own handler.
                glib::idle_add_local_once(after_change);
            });
            controls.append(&on);
            g.add(&widgets::row(&e.manifest.name, &installed_desc(&e), Some(controls.upcast_ref())));
            widgets::keywords("enable disable turn on off hide extension");
        }
        let (r, _) = widgets::button_row(
            "Look for devices again",
            "After plugging something in, ask every extension what it can see.",
            "Refresh",
            |b| {
                busy(b, "Looking…", || Ok(ext::refresh_pages()), |changed| {
                    window::toast(if changed { "Found changes" } else { "Nothing new" });
                    after_change();
                });
            },
        );
        widgets::keywords("rescan refresh detect hardware plugged");
        g.add(&r);
    }

    // ----- Available -----
    let available: Vec<_> = catalog.iter().filter(|c| !installed.iter().any(|e| e.id() == c.id)).cloned().collect();
    if !available.is_empty() {
        let g = page.group("Available");
        for c in available {
            let mut desc = glib::markup_escape_text(&c.description).to_string();
            if !c.devices.is_empty() {
                desc.push_str(&format!("\n<i>{}</i>", glib::markup_escape_text(&c.devices)));
            }
            let missing: Vec<&str> = c.needs.iter().map(String::as_str).filter(|p| !cmd::present(p)).collect();
            if !missing.is_empty() {
                desc.push_str(&format!(
                    "\nNeeds <tt>{}</tt>, which isn't installed.",
                    glib::markup_escape_text(&missing.join(", "))
                ));
            }
            let install = gtk::Button::with_label("Install");
            let id = c.id.clone();
            install.connect_clicked(move |b| {
                let id = id.clone();
                busy(b, "Installing…", move || manage::install(&id), |e| {
                    let n = ext::cached_pages(e.id()).map(|p| p.len()).unwrap_or(0);
                    window::toast(&if n == 0 {
                        format!("Installed {}. No matching devices found yet.", e.manifest.name)
                    } else {
                        format!("Installed {}", e.manifest.name)
                    });
                    after_change();
                });
            });
            let r = widgets::row(&c.name, &desc, Some(install.upcast_ref()));
            r.insert_child_after(&gtk::Image::from_icon_name(&c.icon), None::<&gtk::Widget>);
            widgets::keywords(&format!("{} {}", c.id, c.devices));
            g.add(&r);
        }
    }

    // ----- From a link -----
    let g = page.group("Add from GitHub");
    g.note("Any git repository with an <tt>extension.toml</tt>. Only add extensions you trust: they run as you.");
    let repo = gtk::Entry::new();
    repo.set_placeholder_text(Some("https://github.com/user/repo"));
    repo.set_hexpand(true);
    let path = gtk::Entry::new();
    path.set_placeholder_text(Some("folder (optional)"));
    path.set_width_chars(14);
    let add = gtk::Button::with_label("Install");
    {
        let (repo, path) = (repo.clone(), path.clone());
        add.connect_clicked(move |b| {
            let url = repo.text().trim().to_string();
            if url.is_empty() {
                repo.grab_focus();
                return;
            }
            let sub = path.text().trim().to_string();
            let repo = repo.clone();
            busy(b, "Installing…", move || manage::install_from(&url, &sub, None), move |e| {
                repo.set_text("");
                window::toast(&format!("Installed {}", e.manifest.name));
                after_change();
            });
        });
    }
    let line = widgets::hbox(8);
    line.append(&repo);
    line.append(&path);
    line.append(&add);
    g.add(&widgets::stacked_row("Repository", "", line.upcast_ref()));
    widgets::keywords("url link git clone custom third party");
}
