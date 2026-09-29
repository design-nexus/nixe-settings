use crate::backend::state::Monitor;
use crate::backend::{hypr, store};
use crate::widgets::{self, Page, opts};
use crate::{cmd, window};
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

const SCALES: &[f64] = &[1.0, 1.25, 1.333333, 1.5, 1.6, 1.666667, 1.75, 2.0, 2.5, 3.0];

fn scale_label(s: f64) -> String {
    format!("{}%", (s * 100.0).round() as i64)
}

/// After a display change, ask to keep it; put the old layout back if nobody answers.
fn confirm_or_revert(previous: BTreeMap<String, Monitor>) {
    let Some(win) = window::window() else { return };
    let dialog = gtk::Window::builder().transient_for(&win).modal(true).title("Keep these display settings?").build();
    dialog.add_css_class("settings-window");
    dialog.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let card = widgets::vbox(14);
    card.add_css_class("dialog-card");
    card.append(&widgets::label("Keep these display settings?", "section-title"));
    let msg = widgets::label("Reverting in 15 seconds.", "dim");
    card.append(&msg);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let revert = gtk::Button::with_label("Revert");
    let keep = gtk::Button::with_label("Keep");
    keep.add_css_class("suggested-action");
    buttons.append(&revert);
    buttons.append(&keep);
    card.append(&buttons);
    dialog.set_child(Some(&card));

    let done = Rc::new(Cell::new(false));
    let do_revert = {
        let dialog = dialog.clone();
        let done = done.clone();
        let previous = previous.clone();
        move || {
            if done.replace(true) {
                return;
            }
            let p = previous.clone();
            store::update(true, move |s| s.monitors = p);
            store::flush();
            dialog.close();
            glib::timeout_add_local_once(std::time::Duration::from_millis(900), || window::rebuild("displays"));
        }
    };
    let r1 = do_revert.clone();
    revert.connect_clicked(move |_| r1());
    let d2 = dialog.clone();
    let done2 = done.clone();
    keep.connect_clicked(move |_| {
        done2.set(true);
        d2.close();
    });
    let left = Rc::new(Cell::new(15));
    let r2 = do_revert.clone();
    let done3 = done.clone();
    glib::timeout_add_seconds_local(1, move || {
        if done3.get() {
            return glib::ControlFlow::Break;
        }
        let n = left.get() - 1;
        left.set(n);
        msg.set_text(&format!("Reverting in {n} seconds."));
        if n <= 0 {
            r2();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
    dialog.present();
}

fn change(output: &str, f: impl FnOnce(&mut Monitor)) {
    let previous = store::read(|s| s.monitors.clone());
    let current = live_monitor(output);
    store::update(true, |s| {
        let m = s.monitors.entry(output.to_string()).or_insert_with(|| current.unwrap_or_default());
        f(m);
    });
    store::flush();
    confirm_or_revert(previous);
}

/// The monitor as it runs now, as a starting point for an override.
fn live_monitor(name: &str) -> Option<Monitor> {
    let mons = hypr::json(&["monitors", "all"])?;
    let m = mons.as_array()?.iter().find(|m| m.get("name").and_then(|n| n.as_str()) == Some(name))?;
    Some(Monitor {
        mode: Some(format!(
            "{}x{}@{:.2}",
            m.get("width")?.as_i64()?,
            m.get("height")?.as_i64()?,
            m.get("refreshRate")?.as_f64()?
        )),
        position: Some(format!("{}x{}", m.get("x")?.as_i64()?, m.get("y")?.as_i64()?)),
        scale: m.get("scale").and_then(|v| v.as_f64()),
        transform: m.get("transform").and_then(|v| v.as_i64()),
        disabled: m.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

pub fn build(page: &Page) {
    let monitors: Vec<Value> = hypr::json(&["monitors", "all"]).and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if monitors.is_empty() {
        page.banner("Couldn't read the displays from Hyprland.", true);
        return;
    }
    let brightness = cmd::output(&["omarchy-brightness-display", "--no-osd"]).and_then(|s| s.trim().parse::<f64>().ok());

    for m in &monitors {
        let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let desc = m.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let g = page.group(&name);
        g.note(&glib::markup_escape_text(&desc));

        if let Some(b) = brightness.filter(|_| m.get("focused").and_then(|f| f.as_bool()).unwrap_or(false)) {
            let n = name.clone();
            let (r, _) = widgets::slider_row("Brightness", "", (1.0, 100.0, 1.0), b, 0, "%", move |v| {
                cmd::spawn(&["omarchy-brightness-display", "--no-osd", "--monitor", &n, &format!("{}%", v.round() as i64)]);
            });
            g.add(&r);
        }

        let modes: Vec<String> = m
            .get("availableModes")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim_end_matches("Hz").to_string())).collect())
            .unwrap_or_default();
        let current_mode = format!(
            "{}x{}@{:.2}",
            m.get("width").and_then(|v| v.as_i64()).unwrap_or(0),
            m.get("height").and_then(|v| v.as_i64()).unwrap_or(0),
            m.get("refreshRate").and_then(|v| v.as_f64()).unwrap_or(0.0)
        );
        let mode_opts: Vec<(String, String)> = modes
            .iter()
            .map(|md| {
                let (res, hz) = md.split_once('@').unwrap_or((md, ""));
                let hz = hz.parse::<f64>().map(|h| format!("{} Hz", h.round())).unwrap_or_default();
                (md.clone(), format!("{} · {hz}", res.replace('x', " × ")))
            })
            .collect();
        let n = name.clone();
        let (r, _) = widgets::choice_row("Resolution & refresh rate", "", mode_opts, &current_mode, move |mode| {
            change(&n, |mon| mon.mode = Some(mode));
        });
        g.add(&r);

        let scale = m.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let mut scale_opts: Vec<(String, String)> = SCALES.iter().map(|s| (format!("{s}"), scale_label(*s))).collect();
        let current_scale = SCALES.iter().find(|s| (*s - scale).abs() < 0.01).map(|s| format!("{s}")).unwrap_or_else(|| {
            scale_opts.push((format!("{scale}"), scale_label(scale)));
            format!("{scale}")
        });
        let n = name.clone();
        let (r, _) = widgets::choice_row(
            "Scale",
            "Makes everything bigger on high-resolution screens.",
            scale_opts,
            &current_scale,
            move |s| {
                let v: f64 = s.parse().unwrap_or(1.0);
                change(&n, |mon| mon.scale = Some(v));
            },
        );
        widgets::keywords("hidpi zoom size");
        g.add(&r);

        let transform = m.get("transform").and_then(|v| v.as_i64()).unwrap_or(0);
        let n = name.clone();
        let (r, _) = widgets::choice_row(
            "Rotation",
            "",
            opts(&[
                ("0", "Normal"),
                ("1", "90°"),
                ("2", "180°"),
                ("3", "270°"),
                ("4", "Flipped"),
                ("5", "Flipped 90°"),
                ("6", "Flipped 180°"),
                ("7", "Flipped 270°"),
            ]),
            &transform.to_string(),
            move |t| {
                let v: i64 = t.parse().unwrap_or(0);
                change(&n, |mon| mon.transform = Some(v));
            },
        );
        widgets::keywords("rotate portrait transform");
        g.add(&r);

        if monitors.len() > 1 {
            let pos = format!(
                "{}x{}",
                m.get("x").and_then(|v| v.as_i64()).unwrap_or(0),
                m.get("y").and_then(|v| v.as_i64()).unwrap_or(0)
            );
            let n = name.clone();
            let (r, _) = widgets::entry_row(
                "Position",
                "Top-left corner in the layout, like <tt>2560x0</tt>, or <tt>auto-right</tt>.",
                &pos,
                "auto",
                move |p| change(&n, |mon| mon.position = Some(p.trim().to_string())),
            );
            g.add(&r);
            let disabled = m.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let n = name.clone();
            let (r, _) = widgets::switch_row("Use this display", "", !disabled, move |on| change(&n, |mon| mon.disabled = !on));
            g.add(&r);
        }

        let managed = store::read(|s| s.monitors.contains_key(&name));
        if managed {
            let n = name.clone();
            let (r, _) = widgets::button_row(
                "Settings manages this display",
                "Hand it back to <tt>monitors.lua</tt>.",
                "Reset",
                move |_| {
                    let previous = store::read(|s| s.monitors.clone());
                    let n = n.clone();
                    store::update(true, move |s| {
                        s.monitors.remove(&n);
                    });
                    store::flush();
                    confirm_or_revert(previous);
                },
            );
            g.add(&r);
        }
    }
}
