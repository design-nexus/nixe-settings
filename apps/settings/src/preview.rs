//! Live previews at the top of Window Style and Tiling & Workspaces: a small
//! desktop drawn with the values on the page, redrawn as they change.

use crate::backend::{hypr, store};
use crate::sketch::{self, R, Win, mix, rounded, set};
use gtk::glib;
use gtk::prelude::*;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Pixels in the preview per pixel on a screen: windows on a real screen are
/// big, so gaps and borders are drawn a little larger than true scale.
const SCALE: f64 = 0.6;

type Vals = Rc<RefCell<HashMap<&'static str, Value>>>;

fn number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        // Gaps can be "5" or "5 10 5 10": the first is enough here.
        Value::String(s) => s.split_whitespace().next()?.parse().ok(),
        Value::Object(o) => o.values().find_map(|x| number(Some(x))),
        _ => None,
    }
}

fn current(key: &str) -> Option<Value> {
    store::option(key).or_else(|| hypr::get_option(key))
}

/// Read `keys` now, and keep them up to date while `area` lives.
fn watch(area: &gtk::DrawingArea, keys: &'static [&'static str]) -> Vals {
    let vals: Vals = Rc::default();
    for k in keys {
        if let Some(v) = current(k) {
            vals.borrow_mut().insert(k, v);
        }
    }
    for k in keys {
        let (vals, a) = (vals.clone(), area.downgrade());
        store::subscribe(area, k, move |key, value| {
            let Some(&k) = keys_match(keys, key) else { return };
            match value {
                Some(v) => {
                    vals.borrow_mut().insert(k, v.clone());
                    if let Some(a) = a.upgrade() {
                        a.queue_draw();
                    }
                }
                // Reset: the value comes back from the user's config after a reload.
                None => {
                    let (vals, a) = (vals.clone(), a.clone());
                    glib::timeout_add_local_once(std::time::Duration::from_millis(750), move || {
                        match hypr::get_option(k) {
                            Some(v) => vals.borrow_mut().insert(k, v),
                            None => vals.borrow_mut().remove(k),
                        };
                        if let Some(a) = a.upgrade() {
                            a.queue_draw();
                        }
                    });
                }
            }
        });
    }
    vals
}

fn keys_match<'a>(keys: &'a [&'static str], key: &str) -> Option<&'a &'static str> {
    keys.iter().find(|k| **k == key)
}

fn get(vals: &Vals, key: &str, default: f64) -> f64 {
    number(vals.borrow().get(key)).unwrap_or(default)
}

fn card(area: &gtk::DrawingArea, caption: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.add_css_class("preview-card");
    area.set_hexpand(true);
    b.append(area);
    let l = gtk::Label::new(Some(caption));
    l.add_css_class("preview-caption");
    l.set_xalign(0.0);
    b.append(&l);
    b
}

/// The wallpaper and the bar, and the area left for windows.
fn desktop(cr: &gtk::cairo::Context, p: &sketch::Pal, w: f64, h: f64) -> R {
    rounded(cr, 0.5, 0.5, w - 1.0, h - 1.0, 8.0);
    let grad = gtk::cairo::LinearGradient::new(0.0, 0.0, w, h);
    let a = mix(p.bg, p.accent, 0.35);
    let b = mix(p.bg, p.text, 0.08);
    grad.add_color_stop_rgb(0.0, a.0, a.1, a.2);
    grad.add_color_stop_rgb(1.0, b.0, b.1, b.2);
    let _ = cr.set_source(&grad);
    let _ = cr.fill();
    // The bar.
    set(cr, p.bg, 0.85);
    cr.rectangle(1.0, 1.0, w - 2.0, 14.0);
    let _ = cr.fill();
    for i in 0..5 {
        set(cr, if i == 0 { p.accent } else { p.dim }, 0.9);
        cr.arc(12.0 + i as f64 * 9.0, 8.0, 2.2, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
    R::new(1.0, 15.0, w - 2.0, h - 16.0)
}

const STYLE_KEYS: &[&str] = &[
    "general.gaps_in",
    "general.gaps_out",
    "general.border_size",
    "decoration.rounding",
    "decoration.active_opacity",
    "decoration.inactive_opacity",
    "decoration.dim_inactive",
    "decoration.dim_strength",
    "decoration.shadow.enabled",
    "decoration.shadow.range",
];

/// Window Style: gaps, borders, rounding, opacity, dimming and shadows.
pub fn window_style() -> gtk::Box {
    let area = gtk::DrawingArea::new();
    area.set_content_height(200);
    let vals = watch(&area, STYLE_KEYS);
    area.set_draw_func(move |_, cr, w, h| {
        let p = sketch::pal();
        let (w, h) = (w as f64, h as f64);
        let room = desktop(cr, &p, w, h);
        let px = |key: &str, d: f64| (get(&vals, key, d) * SCALE).max(0.0);
        let gin = px("general.gaps_in", 5.0);
        let gout = px("general.gaps_out", 10.0);
        let border = get(&vals, "general.border_size", 2.0);
        let radius = px("decoration.rounding", 0.0);
        let on = get(&vals, "decoration.active_opacity", 1.0);
        let off = get(&vals, "decoration.inactive_opacity", 1.0);
        let dim = if get(&vals, "decoration.dim_inactive", 0.0) > 0.5 { get(&vals, "decoration.dim_strength", 0.5) } else { 0.0 };
        let shadow = get(&vals, "decoration.shadow.enabled", 1.0) > 0.5;
        let range = px("decoration.shadow.range", 4.0);

        let inner = room.inset(gout);
        let (left, right) = inner.hsplit(0.55, gin * 2.0);
        let (top, bottom) = right.vsplit(0.5, gin * 2.0);
        let wins = [(left, true), (top, false), (bottom, false)];
        if shadow {
            for (r, _) in wins {
                let steps = 6;
                for i in (1..=steps).rev() {
                    let d = range * i as f64 / steps as f64;
                    rounded(cr, r.x - d, r.y - d + 2.0, r.w + 2.0 * d, r.h + 2.0 * d, radius + d);
                    set(cr, (0.0, 0.0, 0.0), 0.5 / steps as f64);
                    let _ = cr.fill();
                }
            }
        }
        for (r, focused) in wins {
            let alpha = if focused { on } else { off };
            sketch::win(
                cr,
                &p,
                r,
                Win {
                    focused,
                    alpha,
                    radius: radius.max(0.01),
                    border: if border > 0.0 { border.max(1.0) } else { 0.01 },
                    lines: 3,
                    ..Win::default()
                },
            );
            if !focused && dim > 0.0 {
                rounded(cr, r.x, r.y, r.w, r.h, radius);
                set(cr, (0.0, 0.0, 0.0), dim);
                let _ = cr.fill();
            }
        }
    });
    card(&area, "Preview: the focused window is on the left. It changes as you do.")
}

const TILING_KEYS: &[&str] =
    &["general.layout", "general.gaps_in", "general.gaps_out", "master.mfact", "master.new_status", "scrolling.column_width"];

/// Tiling: the layout, master width, column width, and where a new window goes.
pub fn tiling() -> gtk::Box {
    let area = gtk::DrawingArea::new();
    area.set_content_height(200);
    let vals = watch(&area, TILING_KEYS);
    area.set_draw_func(move |_, cr, w, h| {
        let p = sketch::pal();
        let (w, h) = (w as f64, h as f64);
        let room = desktop(cr, &p, w, h);
        let gin = (get(&vals, "general.gaps_in", 5.0) * SCALE).max(2.0);
        let gout = (get(&vals, "general.gaps_out", 10.0) * SCALE).max(2.0);
        let inner = room.inset(gout);
        let g = gin * 2.0;
        let layout =
            vals.borrow().get("general.layout").and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "dwindle".into());
        let plain = Win { lines: 3, ..Win::default() };
        let focused = Win { focused: true, ..plain };
        let ghost = Win { ghost: true, ..plain };
        match layout.as_str() {
            "master" => {
                let mfact = get(&vals, "master.mfact", 0.55).clamp(0.1, 0.9);
                let status = vals
                    .borrow()
                    .get("master.new_status")
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| "slave".into());
                let (m, stack) = inner.hsplit(mfact, g);
                let (a, rest) = stack.vsplit(1.0 / 3.0, g);
                let (b, c) = rest.vsplit(0.5, g);
                match status.as_str() {
                    "master" => {
                        sketch::win(cr, &p, m, ghost);
                        sketch::win(cr, &p, a, focused);
                        sketch::win(cr, &p, b, plain);
                        sketch::win(cr, &p, c, plain);
                    }
                    _ => {
                        sketch::win(cr, &p, m, focused);
                        sketch::win(cr, &p, a, plain);
                        sketch::win(cr, &p, b, plain);
                        sketch::win(cr, &p, c, ghost);
                    }
                }
                caption_at(cr, &p, m, &format!("{:.0}%", mfact * 100.0));
            }
            "scrolling" => {
                let cw = get(&vals, "scrolling.column_width", 0.5).clamp(0.1, 1.0);
                let col = inner.w * cw;
                cr.save().ok();
                cr.rectangle(room.x, room.y, room.w, room.h);
                cr.clip();
                let x0 = inner.cx() - col / 2.0;
                for i in -2i32..=2 {
                    let x = x0 + i as f64 * (col + g);
                    let r = R::new(x, inner.y, col, inner.h);
                    sketch::win(
                        cr,
                        &p,
                        r,
                        if i == 0 {
                            focused
                        } else if i == 1 {
                            ghost
                        } else {
                            plain
                        },
                    );
                }
                cr.restore().ok();
                caption_at(cr, &p, R::new(x0, inner.y, col, inner.h), &format!("{:.0}%", cw * 100.0));
            }
            _ => {
                let (a, rest) = inner.hsplit(0.5, g);
                let (b, rest) = rest.vsplit(0.5, g);
                let (c, rest) = rest.hsplit(0.5, g);
                let (d, e) = rest.vsplit(0.5, g);
                sketch::win(cr, &p, a, plain);
                sketch::win(cr, &p, b, plain);
                sketch::win(cr, &p, c, plain);
                sketch::win(cr, &p, d, focused);
                sketch::win(cr, &p, e, ghost);
            }
        }
    });
    card(&area, "Preview: the dashed outline is where the next window opens.")
}

fn caption_at(cr: &gtk::cairo::Context, p: &sketch::Pal, r: R, text: &str) {
    cr.select_font_face("sans-serif", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
    cr.set_font_size(11.0);
    if let Ok(e) = cr.text_extents(text) {
        let (x, y) = (r.cx() - e.width() / 2.0, r.y + r.h - 10.0);
        rounded(cr, x - 5.0, y - e.height() - 3.0, e.width() + 10.0, e.height() + 7.0, 4.0);
        set(cr, p.bg, 0.8);
        let _ = cr.fill();
        set(cr, p.text, 1.0);
        cr.move_to(x - e.x_bearing(), y);
        let _ = cr.show_text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::number;
    use serde_json::json;

    #[test]
    fn reads_numbers_from_hyprland_values() {
        assert_eq!(number(Some(&json!(5))), Some(5.0));
        assert_eq!(number(Some(&json!("5 10 5 10"))), Some(5.0));
        assert_eq!(number(Some(&json!(true))), Some(1.0));
        assert_eq!(number(Some(&json!({"custom": "3 3 3 3"}))), Some(3.0));
        assert_eq!(number(None), None);
    }
}
