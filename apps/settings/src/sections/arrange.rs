//! Displays: the monitors drawn to scale, dragged into place; and Identify,
//! which shows each monitor's name on the monitor itself.

use crate::theme;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::rc::Rc;

/// A monitor in layout coordinates (logical pixels: scaled, and rotated).
#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Its size in the layout: resolution over scale, sideways when turned 90° or 270°.
pub fn logical_size(width: f64, height: f64, scale: f64, transform: i64) -> (f64, f64) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (w, h) = (width / scale, height / scale);
    if transform % 2 == 1 { (h, w) } else { (w, h) }
}

/// Where a dropped monitor lands: against the nearest edge of another one, so
/// screens touch and don't overlap. Returns whole-pixel coordinates.
pub fn snap(moving: &Rect, others: &[Rect]) -> (i64, i64) {
    if others.is_empty() {
        return (0, 0);
    }
    let mut best: Option<(f64, f64, f64)> = None;
    for o in others {
        // Beside it (left or right), lined up top, bottom or as dropped; and above or below.
        let ys = [o.y, o.y + o.h - moving.h, moving.y.clamp(o.y - moving.h + 1.0, o.y + o.h - 1.0)];
        let xs = [o.x, o.x + o.w - moving.w, moving.x.clamp(o.x - moving.w + 1.0, o.x + o.w - 1.0)];
        let mut candidates: Vec<(f64, f64)> = Vec::new();
        for y in ys {
            candidates.push((o.x - moving.w, y));
            candidates.push((o.x + o.w, y));
        }
        for x in xs {
            candidates.push((x, o.y - moving.h));
            candidates.push((x, o.y + o.h));
        }
        for (x, y) in candidates {
            let overlaps = others.iter().any(|p| x < p.x + p.w && x + moving.w > p.x && y < p.y + p.h && y + moving.h > p.y);
            if overlaps {
                continue;
            }
            let d = (x - moving.x).powi(2) + (y - moving.y).powi(2);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((x, y, d));
            }
        }
    }
    let (x, y, _) = best.unwrap_or((moving.x, moving.y, 0.0));
    (x.round() as i64, y.round() as i64)
}

/// Shift a layout so its top-left corner is at 0,0 (Hyprland's convention).
pub fn normalise(rects: &mut [Rect]) {
    let min_x = rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
    let min_y = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
    if min_x.is_finite() {
        for r in rects.iter_mut() {
            r.x -= min_x;
            r.y -= min_y;
        }
    }
}

/// The drawing. `on_moved` gets every monitor's new position once one is dropped.
pub fn canvas(rects: Vec<Rect>, on_moved: impl Fn(Vec<(String, i64, i64)>) + 'static) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(220);
    area.set_hexpand(true);
    area.add_css_class("arrange-canvas");
    let rects = Rc::new(RefCell::new(rects));
    // (index, offset from the monitor's corner to the pointer, in layout pixels)
    let drag: Rc<RefCell<Option<(usize, f64, f64)>>> = Rc::default();
    // Layout → screen: scale and offset, set while drawing.
    let view: Rc<RefCell<(f64, f64, f64)>> = Rc::new(RefCell::new((1.0, 0.0, 0.0)));

    {
        let (rects, view) = (rects.clone(), view.clone());
        area.set_draw_func(move |_, cr, w, h| {
            let pal = theme::current_palette();
            let rgb = |c: &str| crate::fangraph::rgb(c);
            let rs = rects.borrow();
            let min_x = rs.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
            let min_y = rs.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
            let max_x = rs.iter().map(|r| r.x + r.w).fold(f64::NEG_INFINITY, f64::max);
            let max_y = rs.iter().map(|r| r.y + r.h).fold(f64::NEG_INFINITY, f64::max);
            let (lw, lh) = ((max_x - min_x).max(1.0), (max_y - min_y).max(1.0));
            let pad = 24.0;
            // Leave room to drag one past the others.
            let scale = ((w as f64 - 2.0 * pad) / (lw * 1.35)).min((h as f64 - 2.0 * pad) / (lh * 1.35));
            let ox = (w as f64 - lw * scale) / 2.0 - min_x * scale;
            let oy = (h as f64 - lh * scale) / 2.0 - min_y * scale;
            *view.borrow_mut() = (scale, ox, oy);
            for r in rs.iter() {
                let (x, y, rw, rh) = (ox + r.x * scale, oy + r.y * scale, r.w * scale, r.h * scale);
                let (a, b, c) = rgb(&pal.accent);
                cr.set_source_rgba(a, b, c, 0.16);
                cr.rectangle(x + 2.0, y + 2.0, rw - 4.0, rh - 4.0);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(a, b, c, 0.9);
                cr.set_line_width(1.5);
                let _ = cr.stroke();
                let (t1, t2, t3) = rgb(&pal.text);
                cr.set_source_rgb(t1, t2, t3);
                cr.select_font_face("sans-serif", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
                cr.set_font_size(13.0);
                if let Ok(ext) = cr.text_extents(&r.name) {
                    cr.move_to(x + (rw - ext.width()) / 2.0, y + (rh + ext.height()) / 2.0);
                    let _ = cr.show_text(&r.name);
                }
            }
        });
    }

    let gesture = gtk::GestureDrag::new();
    {
        let (rects, view, drag) = (rects.clone(), view.clone(), drag.clone());
        gesture.connect_drag_begin(move |_, px, py| {
            let (s, ox, oy) = *view.borrow();
            let (lx, ly) = ((px - ox) / s, (py - oy) / s);
            let hit = rects.borrow().iter().position(|r| lx >= r.x && lx <= r.x + r.w && ly >= r.y && ly <= r.y + r.h);
            *drag.borrow_mut() = hit.map(|i| {
                let r = &rects.borrow()[i];
                (i, lx - r.x, ly - r.y)
            });
        });
    }
    {
        let (rects, view, drag, area2) = (rects.clone(), view.clone(), drag.clone(), area.clone());
        gesture.connect_drag_update(move |g, dx, dy| {
            let Some((i, offx, offy)) = *drag.borrow() else { return };
            let Some((sx, sy)) = g.start_point() else { return };
            let (s, ox, oy) = *view.borrow();
            let (lx, ly) = ((sx + dx - ox) / s, (sy + dy - oy) / s);
            if let Some(r) = rects.borrow_mut().get_mut(i) {
                r.x = lx - offx;
                r.y = ly - offy;
            }
            area2.queue_draw();
        });
    }
    {
        let (rects, drag, area2) = (rects.clone(), drag.clone(), area.clone());
        gesture.connect_drag_end(move |_, _, _| {
            let Some((i, _, _)) = drag.borrow_mut().take() else { return };
            let mut rs = rects.borrow_mut();
            let others: Vec<Rect> = rs.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, r)| r.clone()).collect();
            let (x, y) = snap(&rs[i], &others);
            rs[i].x = x as f64;
            rs[i].y = y as f64;
            normalise(&mut rs);
            let positions = rs.iter().map(|r| (r.name.clone(), r.x.round() as i64, r.y.round() as i64)).collect();
            drop(rs);
            area2.queue_draw();
            on_moved(positions);
        });
    }
    area.add_controller(gesture);
    area.set_cursor_from_name(Some("grab"));
    area
}

/// Show each monitor's name, large, on that monitor for a moment.
pub fn identify() {
    let Some(display) = gdk::Display::default() else { return };
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        let Some(mon) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        let name = mon.connector().map(|c| c.to_string()).unwrap_or_default();
        let win = gtk::Window::builder().title("Settings: identify display").decorated(false).build();
        win.add_css_class("identify-window");
        let label = gtk::Label::new(Some(&name));
        label.add_css_class("identify-name");
        let model = mon.model().map(|m| m.to_string()).unwrap_or_default();
        let bx = gtk::Box::new(gtk::Orientation::Vertical, 8);
        bx.set_halign(gtk::Align::Center);
        bx.set_valign(gtk::Align::Center);
        bx.append(&label);
        if !model.is_empty() {
            let m = gtk::Label::new(Some(&model));
            m.add_css_class("identify-model");
            bx.append(&m);
        }
        win.set_child(Some(&bx));
        win.fullscreen_on_monitor(&mon);
        win.present();
        // Any click closes it early.
        let click = gtk::GestureClick::new();
        let w2 = win.clone();
        click.connect_pressed(move |_, _, _, _| w2.close());
        win.add_controller(click);
        glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || win.close());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { name: name.into(), x, y, w, h }
    }

    #[test]
    fn sizes() {
        assert_eq!(logical_size(2560.0, 1600.0, 1.6, 0), (1600.0, 1000.0));
        assert_eq!(logical_size(1920.0, 1080.0, 1.0, 1), (1080.0, 1920.0));
    }

    #[test]
    fn snaps_to_the_nearest_edge() {
        let laptop = r("eDP-1", 0.0, 0.0, 1600.0, 1000.0);
        // Dropped a little right of the laptop and lower: lands touching its right edge.
        let ext = r("DP-1", 1650.0, 40.0, 1920.0, 1080.0);
        assert_eq!(snap(&ext, &[laptop.clone()]), (1600, 40));
        // Dropped mostly above: sits on top.
        let ext = r("DP-1", 100.0, -1000.0, 1920.0, 1080.0);
        assert_eq!(snap(&ext, &[laptop]), (100, -1080));
    }

    #[test]
    fn never_overlaps() {
        let a = r("A", 0.0, 0.0, 1000.0, 1000.0);
        let b = r("B", 500.0, 500.0, 1000.0, 1000.0);
        let (x, y) = snap(&b, &[a.clone()]);
        assert!(x >= 1000 || y >= 1000 || x <= -1000 || y <= -1000);
    }

    #[test]
    fn layout_starts_at_zero() {
        let mut v = vec![r("A", 0.0, 0.0, 10.0, 10.0), r("B", -20.0, -5.0, 20.0, 10.0)];
        normalise(&mut v);
        assert_eq!((v[0].x, v[0].y, v[1].x, v[1].y), (20.0, 5.0, 0.0, 0.0));
    }
}
