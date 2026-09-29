use crate::backend::{hypr, store};
use crate::widgets::{self, Page, hypr_choice, hypr_slider, hypr_switch, opts};
use gtk::prelude::*;
use serde_json::json;

pub fn build(page: &Page) {
    let g = page.group("Pointer");
    g.add(&hypr_slider(
        "input.sensitivity",
        "Pointer speed",
        "Applies to every mouse and trackpad. 0 is the default.",
        (-1.0, 1.0, 0.05),
        2,
        "",
        false,
    ));
    g.add(&hypr_choice(
        "input.accel_profile",
        "Acceleration",
        "<b>Flat</b> moves the pointer the same distance however fast you move — best for gaming.",
        opts(&[("adaptive", "Adaptive"), ("flat", "Flat (no acceleration)")]),
    ));
    g.add(&hypr_switch("input.left_handed", "Left-handed", "Swap the left and right buttons."));

    let g = page.group("Scrolling");
    g.add(&hypr_switch("input.natural_scroll", "Natural scrolling (mouse)", "Content moves with the wheel, like a phone."));
    g.add(&hypr_slider("input.scroll_factor", "Scroll speed (mouse)", "", (0.1, 3.0, 0.05), 2, "×", false));

    // ----- Per device -----
    let g = page.group("Per device");
    g.note("Give one mouse different settings from the rest. These override everything above.");
    let mice: Vec<String> = hypr::json(&["devices"])
        .and_then(|d| d.get("mice").cloned())
        .and_then(|m| m.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
        .filter(|n| !n.contains("touchpad"))
        .collect();
    if mice.is_empty() {
        g.add(&widgets::row("No mice found", "Connect a mouse to give it its own settings.", None));
    }
    for name in mice {
        let fields = store::read(|s| s.devices.get(&name).cloned().unwrap_or_default());
        let body = widgets::vbox(8);

        let sens = fields.get("sensitivity").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (row, s) = widgets::slider(-1.0, 1.0, 0.05, sens, 2, "");
        let n = name.clone();
        s.scale
            .connect_value_changed(move |sc| store::set_device(&n, "sensitivity", json!((sc.value() * 100.0).round() / 100.0)));
        let line = widgets::hbox(10);
        let l = widgets::label("Speed", "dim");
        l.set_width_chars(14);
        line.append(&l);
        line.append(&row);
        body.append(&line);

        let accel = fields.get("accel_profile").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let choices = opts(&[("", "Same as above"), ("adaptive", "Adaptive"), ("flat", "Flat")]);
        let dd = widgets::dropdown(&choices, &accel);
        let n = name.clone();
        dd.connect_selected_notify(move |d| {
            let id = choices.get(d.selected() as usize).map(|c| c.0.clone()).unwrap_or_default();
            if !id.is_empty() {
                store::set_device(&n, "accel_profile", json!(id));
            }
        });
        let line = widgets::hbox(10);
        let l = widgets::label("Acceleration", "dim");
        l.set_width_chars(14);
        line.append(&l);
        line.append(&dd);
        body.append(&line);

        let natural = fields.get("natural_scroll").and_then(|v| v.as_bool()).unwrap_or(false);
        let sw = gtk::Switch::new();
        sw.set_active(natural);
        sw.set_halign(gtk::Align::Start);
        let n = name.clone();
        sw.connect_active_notify(move |s| store::set_device(&n, "natural_scroll", json!(s.is_active())));
        let line = widgets::hbox(10);
        let l = widgets::label("Natural scroll", "dim");
        l.set_width_chars(14);
        line.append(&l);
        line.append(&sw);
        body.append(&line);

        let reset = gtk::Button::with_label("Use the global settings");
        reset.set_halign(gtk::Align::Start);
        let n = name.clone();
        reset.connect_clicked(move |_| {
            store::reset_device(&n);
            crate::window::toast("Device overrides removed");
            crate::window::rebuild("mouse");
        });
        body.append(&reset);

        g.add(&widgets::stacked_row(&name, "", body.upcast_ref()));
    }
}
