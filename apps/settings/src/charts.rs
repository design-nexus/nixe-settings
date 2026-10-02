//! Small read-only charts in the theme's colours: a ring gauge, a rolling line
//! chart and a horizontal meter. Values are fractions (0–1) unless a chart is
//! given its own scale.

use crate::theme::{self, Palette};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::f64::consts::PI;
use std::rc::Rc;

/// Seconds of history a line chart keeps.
pub const HISTORY: usize = 60;
/// At or above this a gauge or meter turns the danger colour.
const DANGER: f64 = 0.9;
/// At or below this a `warn_when_low` gauge turns the danger colour.
const LOW: f64 = 0.15;

thread_local! {
    static PALETTE: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

fn palette() -> Palette {
    PALETTE.with(|p| p.borrow_mut().get_or_insert_with(theme::current_palette).clone())
}

/// Pick up a theme change (the Home page calls this once per sample).
pub fn refresh_palette() {
    PALETTE.with(|p| *p.borrow_mut() = Some(theme::current_palette()));
}

type Rgb = (f64, f64, f64);

fn set(cr: &gtk::cairo::Context, c: Rgb, a: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, a);
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(h / 2.0).min(w / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

// ---------- Ring ----------

#[derive(Clone)]
pub struct Ring {
    pub area: gtk::DrawingArea,
    value: Rc<RefCell<f64>>,
    /// Low values are the warning (battery), not high ones.
    low_is_bad: Rc<RefCell<bool>>,
}

impl Ring {
    pub fn set(&self, v: f64) {
        *self.value.borrow_mut() = v.clamp(0.0, 1.0);
        self.area.queue_draw();
    }

    pub fn warn_when_low(&self) {
        *self.low_is_bad.borrow_mut() = true;
    }
}

pub fn ring(size: i32) -> Ring {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_halign(gtk::Align::Center);
    let value: Rc<RefCell<f64>> = Rc::default();
    let low_is_bad: Rc<RefCell<bool>> = Rc::default();
    let (v, low) = (value.clone(), low_is_bad.clone());
    area.set_draw_func(move |_, cr, w, h| {
        let pal = palette();
        let v = *v.borrow();
        let bad = if *low.borrow() { v <= LOW } else { v >= DANGER };
        let width = 7.0;
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let r = cx.min(cy) - width / 2.0 - 1.0;
        cr.set_line_width(width);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        set(cr, crate::fangraph::rgb(&pal.dim_text), 0.22);
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        if v > 0.0 {
            let c = if bad { &pal.danger } else { &pal.accent };
            set(cr, crate::fangraph::rgb(c), 1.0);
            cr.arc(cx, cy, r, -PI / 2.0, -PI / 2.0 + v.max(0.01) * 2.0 * PI);
            let _ = cr.stroke();
        }
    });
    Ring { area, value, low_is_bad }
}

// ---------- Line chart ----------

#[derive(Clone)]
pub struct LineChart {
    pub area: gtk::DrawingArea,
    series: Rc<RefCell<Vec<VecDeque<f64>>>>,
}

impl LineChart {
    /// Add one value to each series and redraw.
    pub fn push(&self, values: &[f64]) {
        let mut s = self.series.borrow_mut();
        for (q, v) in s.iter_mut().zip(values) {
            if q.len() == HISTORY {
                q.pop_front();
            }
            q.push_back(*v);
        }
        drop(s);
        self.area.queue_draw();
    }
}

/// `lines` series; the first is filled. With `max` the scale is fixed (1.0 for
/// fractions); without it the scale fits the highest value shown, at least `floor`.
pub fn line_chart(lines: usize, max: Option<f64>, floor: f64, height: i32) -> LineChart {
    let area = gtk::DrawingArea::new();
    area.add_css_class("chart");
    area.set_content_height(height);
    area.set_hexpand(true);
    area.set_vexpand(true);
    let series: Rc<RefCell<Vec<VecDeque<f64>>>> = Rc::new(RefCell::new(vec![VecDeque::with_capacity(HISTORY); lines]));
    let s = series.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let pal = palette();
        let (w, h) = (w as f64, h as f64);
        let pad = 8.0;
        let s = s.borrow();
        let top = max.unwrap_or_else(|| s.iter().flatten().copied().fold(floor, f64::max) * 1.15);
        let x = |i: usize, n: usize| pad + (w - 2.0 * pad) * (HISTORY - n + i) as f64 / (HISTORY - 1) as f64;
        let y = |v: f64| h - pad - (h - 2.0 * pad) * (v / top).clamp(0.0, 1.0);

        cr.set_line_width(1.0);
        set(cr, crate::fangraph::rgb(&pal.border), 0.35);
        for i in 0..=3 {
            let gy = (pad + (h - 2.0 * pad) * i as f64 / 3.0).round() + 0.5;
            cr.move_to(pad, gy);
            cr.line_to(w - pad, gy);
        }
        let _ = cr.stroke();

        let colours = [crate::fangraph::rgb(&pal.accent), crate::fangraph::rgb(&pal.dim_text)];
        for (k, q) in s.iter().enumerate().rev() {
            if q.len() < 2 {
                continue;
            }
            let c = colours[k.min(1)];
            let n = q.len();
            if k == 0 {
                cr.move_to(x(0, n), h - pad);
                for (i, v) in q.iter().enumerate() {
                    cr.line_to(x(i, n), y(*v));
                }
                cr.line_to(x(n - 1, n), h - pad);
                cr.close_path();
                set(cr, c, 0.14);
                let _ = cr.fill();
            }
            cr.set_line_width(2.0);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            for (i, v) in q.iter().enumerate() {
                if i == 0 { cr.move_to(x(i, n), y(*v)) } else { cr.line_to(x(i, n), y(*v)) }
            }
            set(cr, c, 1.0);
            let _ = cr.stroke();
        }
    });
    LineChart { area, series }
}

// ---------- Meter ----------

#[derive(Clone)]
pub struct Meter {
    pub area: gtk::DrawingArea,
    parts: Rc<RefCell<Vec<f64>>>,
    /// Shares of a whole, never a warning when full.
    calm: Rc<RefCell<bool>>,
}

impl Meter {
    /// Stacked fractions: the first in the accent colour, the rest fainter.
    pub fn set(&self, parts: Vec<f64>) {
        *self.parts.borrow_mut() = parts;
        self.area.queue_draw();
    }

    pub fn never_warn(&self) {
        *self.calm.borrow_mut() = true;
    }
}

pub fn meter(height: i32) -> Meter {
    let area = gtk::DrawingArea::new();
    area.set_content_height(height);
    area.set_hexpand(true);
    area.set_valign(gtk::Align::Center);
    let parts: Rc<RefCell<Vec<f64>>> = Rc::default();
    let calm: Rc<RefCell<bool>> = Rc::default();
    let (p, c) = (parts.clone(), calm.clone());
    area.set_draw_func(move |_, cr, w, h| {
        let pal = palette();
        let (w, h) = (w as f64, h as f64);
        let r = h / 2.0;
        set(cr, crate::fangraph::rgb(&pal.dim_text), 0.18);
        rounded(cr, 0.0, 0.0, w, h, r);
        let _ = cr.fill();
        let parts = p.borrow();
        let total: f64 = parts.iter().sum();
        let main = crate::fangraph::rgb(if total >= DANGER && !*c.borrow() { &pal.danger } else { &pal.accent });
        cr.save().ok();
        rounded(cr, 0.0, 0.0, w, h, r);
        cr.clip();
        let mut x = 0.0;
        for (i, f) in parts.iter().enumerate() {
            let fw = w * f.clamp(0.0, 1.0);
            set(cr, main, if i == 0 { 1.0 } else { 0.45 });
            cr.rectangle(x, 0.0, fw, h);
            let _ = cr.fill();
            x += fw;
        }
        cr.restore().ok();
    });
    Meter { area, parts, calm }
}
