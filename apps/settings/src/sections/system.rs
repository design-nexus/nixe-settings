use crate::backend::{cleanup, hypr, store};
use crate::widgets::{self, Page};
use crate::{cmd, paths, prefs, window};
use gtk::glib;
use gtk::prelude::*;

fn code_block(text: &str) -> gtk::ScrolledWindow {
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("code-block");
    view.buffer().set_text(text);
    gtk::ScrolledWindow::builder()
        .child(&view)
        .min_content_height(160)
        .max_content_height(340)
        .propagate_natural_height(true)
        .build()
}

/// Ask before removing the old panels; runs the cleanup when confirmed.
pub fn confirm_cleanup() {
    let plan = cleanup::plan();
    let Some(win) = window::window() else { return };
    if plan.is_empty() {
        window::toast("Nothing left to remove");
        return;
    }
    let dialog =
        gtk::Window::builder().transient_for(&win).modal(true).title("Remove old settings panels").default_width(640).build();
    dialog.add_css_class("settings-window");
    dialog.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let card = widgets::vbox(12);
    card.add_css_class("dialog-card");
    card.append(&widgets::label("Remove the old settings panels?", "section-title"));
    let intro = widgets::label(
        "Settings replaces them. Nothing is carried over — Settings starts from Omarchy's defaults. \
         Everything below is backed up first and put back automatically if Hyprland reports an error.",
        "dim",
    );
    intro.set_wrap(true);
    card.append(&intro);
    card.append(&code_block(&cleanup::describe(&plan)));
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let later = gtk::Button::with_label("Not now");
    let go = gtk::Button::with_label("Back up and remove");
    go.add_css_class("destructive-action");
    buttons.append(&later);
    buttons.append(&go);
    card.append(&buttons);
    dialog.set_child(Some(&card));

    let d = dialog.clone();
    later.connect_clicked(move |_| d.close());
    let d = dialog.clone();
    go.connect_clicked(move |b| {
        b.set_sensitive(false);
        b.set_label("Removing…");
        let d = d.clone();
        let plan = plan.clone();
        // Flush first so our own file exists before theirs go away.
        store::flush();
        cmd::background(
            move || cleanup::execute(&plan).map_err(|e| format!("{e:#}")),
            move |r| {
                d.close();
                match r {
                    Ok(out) => window::toast(&format!("Old panels removed. Backup: {}", paths::pretty(&out.backup))),
                    Err(e) => window::toast(&format!("Cleanup failed — {e}")),
                }
                window::rebuild("system");
            },
        );
    });
    dialog.present();
}

/// Offer the cleanup once, the first time Settings opens with old panels present.
pub fn maybe_offer_cleanup() {
    if prefs::get().cleanup_offered {
        return;
    }
    prefs::update(|p| p.cleanup_offered = true);
    glib::timeout_add_local_once(std::time::Duration::from_millis(600), || {
        if !cleanup::plan().is_empty() {
            confirm_cleanup();
        }
    });
}

pub fn build(page: &Page) {
    // ----- Old panels -----
    let plan = cleanup::plan();
    let g = page.group("Old settings panels");
    if plan.is_empty() {
        g.add(&widgets::row(
            "All clear",
            "OmaSettings, Omarchy Control Panel and the old Settings plugin aren't installed.",
            None,
        ));
    } else {
        let (r, b) = widgets::button_row(
            "Remove the panels Settings replaces",
            "OmaSettings, Omarchy Control Panel and the old Settings plugin still change your config and fight over the same values.",
            "Review…",
            |_| confirm_cleanup(),
        );
        b.add_css_class("suggested-action");
        widgets::keywords("omasettings control panel cleanup remove uninstall");
        g.add(&r);
    }
    let backups: Vec<std::path::PathBuf> = std::fs::read_dir(paths::app_dir())
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("old-panels-backup-")))
                .collect()
        })
        .unwrap_or_default();
    for b in backups {
        let path = b.clone();
        let (r, _) = widgets::button_row(
            &format!("Backup {}", paths::pretty(&b)),
            "Everything the cleanup removed.",
            "Restore",
            move |btn| {
                btn.set_sensitive(false);
                match cleanup::restore(&path) {
                    Ok(()) => {
                        hypr::reload();
                        window::toast("Restored. Re-enable the plugins from the Plugins page if you want them back.");
                    }
                    Err(e) => window::toast(&format!("{e}")),
                }
            },
        );
        g.add(&r);
    }

    // ----- What Settings manages -----
    let g = page.group("What Settings manages");
    let (n_opts, n_dev, n_bind, n_mon, gestures) = store::read(|s| {
        (s.options.len(), s.devices.len(), s.binds.len() + s.unbinds.len(), s.monitors.len(), s.gestures.enabled)
    });
    let summary = format!(
        "{n_opts} options · {n_dev} device overrides · {n_bind} shortcuts · {n_mon} displays · gestures {}",
        if gestures { "on" } else { "off" }
    );
    let (r, _) = widgets::info_row("Managed values", &summary);
    g.add(&r);
    let required = std::fs::read_to_string(paths::hyprland_lua()).is_ok_and(|t| hypr::is_required(&t));
    let (r, _) = widgets::info_row(
        "Loaded by Hyprland",
        if required {
            "Yes — hyprland.lua loads hypr/settings.lua last"
        } else {
            "Not yet — added the first time you change something"
        },
    );
    g.add(&r);
    let errors = hypr::config_errors();
    if !errors.is_empty() {
        g.add(&widgets::stacked_row("Hyprland config errors", "", code_block(&errors.join("\n")).upcast_ref()));
    }
    let (r, b) = widgets::button_row(
        "Reset everything",
        "Forget every Hyprland value set here. Your own config and Omarchy's defaults take over again.",
        "Reset all",
        |b| {
            if !b.has_css_class("armed") {
                b.add_css_class("armed");
                b.set_label("Click again to reset");
                return;
            }
            store::update(true, |s| {
                let max = s.max_volume;
                let autostart = s.autostart.clone();
                *s = Default::default();
                s.max_volume = max;
                s.autostart = autostart;
            });
            store::flush();
            window::toast("All Hyprland values handed back");
            glib::timeout_add_local_once(std::time::Duration::from_millis(800), || window::rebuild("system"));
        },
    );
    b.add_css_class("destructive-action");
    g.add(&r);
}
