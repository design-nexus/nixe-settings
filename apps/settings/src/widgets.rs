//! Nexus-style building blocks: pages, groups and option rows, plus rows bound
//! directly to Hyprland options.

use crate::backend::{hypr, store};
use crate::{cmd, paths};
use gtk::prelude::*;
use gtk::{glib, pango};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

// ---------- Search registry ----------

pub struct SearchItem {
    pub section: String,
    pub text: String,
    pub row: gtk::Widget,
    pub group: Option<gtk::Widget>,
}

thread_local! {
    static CURRENT_SECTION: RefCell<String> = const { RefCell::new(String::new()) };
    static CURRENT_GROUP: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
    pub static SEARCH: RefCell<Vec<SearchItem>> = const { RefCell::new(Vec::new()) };
}

fn register(row: &impl IsA<gtk::Widget>, title: &str, desc: &str, keywords: &str) {
    let section = CURRENT_SECTION.with(|s| s.borrow().clone());
    let group = CURRENT_GROUP.with(|g| g.borrow().clone());
    SEARCH.with(|s| {
        s.borrow_mut().push(SearchItem {
            section,
            text: format!("{title} {desc} {keywords}").to_lowercase(),
            row: row.clone().upcast(),
            group,
        })
    });
}

/// Extra search words for the most recently added row.
pub fn keywords(words: &str) {
    SEARCH.with(|s| {
        if let Some(last) = s.borrow_mut().last_mut() {
            last.text.push(' ');
            last.text.push_str(&words.to_lowercase());
        }
    });
}

/// Rows made from now on belong to this section (for pages that fill in later).
pub fn begin_section(section: &str) {
    CURRENT_SECTION.with(|s| *s.borrow_mut() = section.to_string());
    CURRENT_GROUP.with(|g| *g.borrow_mut() = None);
}

/// Drop the search entries for rows inside `within` (before they're replaced).
pub fn forget_rows(within: &impl IsA<gtk::Widget>) {
    let within = within.as_ref();
    SEARCH.with(|s| s.borrow_mut().retain(|item| !item.row.is_ancestor(within)));
}

// ---------- Page / group ----------

pub struct Page {
    pub root: gtk::ScrolledWindow,
    pub body: gtk::Box,
}

pub fn page(section: &str, title: &str, description: &str, files: &[PathBuf]) -> Page {
    CURRENT_SECTION.with(|s| *s.borrow_mut() = section.to_string());
    CURRENT_GROUP.with(|g| *g.borrow_mut() = None);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("settings-page");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("section-header");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("section-title");
    t.set_xalign(0.0);
    let d = gtk::Label::new(Some(description));
    d.add_css_class("section-description");
    d.set_xalign(0.0);
    d.set_wrap(true);
    text.append(&t);
    text.append(&d);
    header.append(&text);
    if !files.is_empty() {
        header.append(&open_config_button(files));
    }
    body.append(&header);

    body.set_hexpand(true);

    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        // Scrolls with wheel, trackpad and keyboard; no visible scrollbar.
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&body)
        .vexpand(true)
        .build();
    Page { root, body }
}

impl Page {
    pub fn group(&self, title: &str) -> Group {
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.add_css_class("settings-group");
        if !title.is_empty() {
            let l = gtk::Label::new(Some(&title.to_uppercase()));
            l.add_css_class("group-title");
            l.set_xalign(0.0);
            wrapper.append(&l);
        }
        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        wrapper.append(&list);
        self.body.append(&wrapper);
        CURRENT_GROUP.with(|g| *g.borrow_mut() = Some(wrapper.clone().upcast()));
        Group { wrapper, list }
    }

    pub fn banner(&self, text: &str, warning: bool) -> gtk::Box {
        let b = banner(text, warning);
        self.body.append(&b);
        b
    }
}

pub fn banner(text: &str, warning: bool) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    b.add_css_class("banner");
    if warning {
        b.add_css_class("warning");
    }
    let icon = gtk::Image::from_icon_name(if warning { "dialog-warning-symbolic" } else { "dialog-information-symbolic" });
    icon.set_valign(gtk::Align::Start);
    let l = gtk::Label::new(None);
    l.set_markup(text);
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.set_hexpand(true);
    b.append(&icon);
    b.append(&l);
    b
}

#[derive(Clone)]
pub struct Group {
    pub wrapper: gtk::Box,
    pub list: gtk::Box,
}

impl Group {
    pub fn add(&self, w: &impl IsA<gtk::Widget>) {
        self.list.append(w);
    }

    pub fn note(&self, text: &str) {
        let l = gtk::Label::new(None);
        l.set_markup(text);
        l.add_css_class("group-note");
        l.set_xalign(0.0);
        l.set_wrap(true);
        // Notes sit just under the group title.
        self.wrapper.insert_child_after(&l, self.wrapper.first_child().as_ref());
    }
}

// ---------- Open config ----------

pub fn open_config_button(files: &[PathBuf]) -> gtk::Widget {
    let make_content = || {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        b.append(&gtk::Image::from_icon_name("text-editor-symbolic"));
        b.append(&gtk::Label::new(Some("Open config")));
        b
    };
    if files.len() == 1 {
        let path = files[0].clone();
        let button = gtk::Button::new();
        button.set_child(Some(&make_content()));
        button.add_css_class("open-config");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some(&paths::pretty(&path)));
        button.connect_clicked(move |_| cmd::open_in_editor(&path));
        return button.upcast();
    }
    let menu = gtk::MenuButton::new();
    menu.set_child(Some(&make_content()));
    menu.add_css_class("open-config");
    menu.set_valign(gtk::Align::Center);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let popover = gtk::Popover::new();
    for path in files {
        let b = gtk::Button::with_label(&paths::pretty(path));
        b.add_css_class("flat");
        if let Some(label) = b.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
            label.add_css_class("mono");
        }
        let p = path.clone();
        let pop = popover.clone();
        b.connect_clicked(move |_| {
            pop.popdown();
            cmd::open_in_editor(&p);
        });
        list.append(&b);
    }
    popover.set_child(Some(&list));
    menu.set_popover(Some(&popover));
    menu.upcast()
}

// ---------- Rows ----------

/// An option card: title and description on the left, control on the right.
pub fn row(title: &str, desc: &str, control: Option<&gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("settings-option");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("settings-option-title");
    t.set_xalign(0.0);
    t.set_wrap(true);
    text.append(&t);
    if !desc.is_empty() {
        let d = gtk::Label::new(None);
        d.set_markup(desc);
        d.add_css_class("settings-option-description");
        d.set_xalign(0.0);
        d.set_wrap(true);
        d.set_wrap_mode(pango::WrapMode::WordChar);
        text.append(&d);
    }
    row.append(&text);
    if let Some(c) = control {
        c.set_valign(gtk::Align::Center);
        row.append(c);
    }
    register(&row, title, desc, "");
    row
}

/// A row whose control sits underneath the text (for wide controls).
pub fn stacked_row(title: &str, desc: &str, control: &gtk::Widget) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.add_css_class("settings-option");
    outer.add_css_class("tall");
    if !title.is_empty() {
        let t = gtk::Label::new(Some(title));
        t.add_css_class("settings-option-title");
        t.set_xalign(0.0);
        outer.append(&t);
    }
    if !desc.is_empty() {
        let d = gtk::Label::new(None);
        d.set_markup(desc);
        d.add_css_class("settings-option-description");
        d.set_xalign(0.0);
        d.set_wrap(true);
        outer.append(&d);
    }
    outer.append(control);
    register(&outer, title, desc, "");
    outer
}

pub fn switch_row(title: &str, desc: &str, active: bool, on_change: impl Fn(bool) + 'static) -> (gtk::Box, gtk::Switch) {
    let sw = gtk::Switch::new();
    sw.set_active(active);
    sw.connect_active_notify(move |s| on_change(s.is_active()));
    let r = row(title, desc, Some(sw.upcast_ref()));
    (r, sw)
}

pub struct Slider {
    pub scale: gtk::Scale,
    pub readout: gtk::Label,
}

pub fn format_value(v: f64, digits: u32, unit: &str) -> String {
    let n = if digits == 0 { format!("{}", v.round() as i64) } else { format!("{:.*}", digits as usize, v) };
    format!("{n}{unit}")
}

pub fn slider(min: f64, max: f64, step: f64, value: f64, digits: u32, unit: &str) -> (gtk::Box, Slider) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    scale.set_value(value);
    scale.set_draw_value(false);
    scale.set_digits(digits as i32);
    scale.set_width_request(220);
    let readout = gtk::Label::new(Some(&format_value(value, digits, unit)));
    readout.add_css_class("value-readout");
    readout.set_xalign(1.0);
    let unit = unit.to_string();
    let r = readout.clone();
    scale.connect_value_changed(move |s| r.set_text(&format_value(s.value(), digits, &unit)));
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    b.append(&scale);
    b.append(&readout);
    (b, Slider { scale, readout })
}

pub fn slider_row(
    title: &str,
    desc: &str,
    (min, max, step): (f64, f64, f64),
    value: f64,
    digits: u32,
    unit: &str,
    on_change: impl Fn(f64) + 'static,
) -> (gtk::Box, Slider) {
    let (b, s) = slider(min, max, step, value, digits, unit);
    s.scale.connect_value_changed(move |sc| on_change(sc.value()));
    let r = row(title, desc, Some(b.upcast_ref()));
    (r, s)
}

pub fn dropdown(options: &[(String, String)], current: &str) -> gtk::DropDown {
    let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    if let Some(i) = options.iter().position(|(id, _)| id == current) {
        dd.set_selected(i as u32);
    } else {
        dd.set_selected(gtk::INVALID_LIST_POSITION);
    }
    dd
}

pub fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

pub fn choice_row(
    title: &str,
    desc: &str,
    options: Vec<(String, String)>,
    current: &str,
    on_change: impl Fn(String) + 'static,
) -> (gtk::Box, gtk::DropDown) {
    let dd = dropdown(&options, current);
    dd.connect_selected_notify(move |d| {
        if let Some((id, _)) = options.get(d.selected() as usize) {
            on_change(id.clone());
        }
    });
    let r = row(title, desc, Some(dd.upcast_ref()));
    (r, dd)
}

pub fn entry_row(
    title: &str,
    desc: &str,
    value: &str,
    placeholder: &str,
    on_apply: impl Fn(String) + 'static,
) -> (gtk::Box, gtk::Entry) {
    let e = gtk::Entry::new();
    e.set_text(value);
    e.set_placeholder_text(Some(placeholder));
    e.set_width_chars(22);
    let apply = Rc::new(on_apply);
    let last = Rc::new(RefCell::new(value.to_string()));
    let fire = {
        let apply = apply.clone();
        let last = last.clone();
        move |e: &gtk::Entry| {
            let t = e.text().to_string();
            if *last.borrow() != t {
                *last.borrow_mut() = t.clone();
                apply(t);
            }
        }
    };
    let f1 = fire.clone();
    e.connect_activate(move |e| f1(e));
    let focus = gtk::EventControllerFocus::new();
    let e2 = e.clone();
    focus.connect_leave(move |_| fire(&e2));
    e.add_controller(focus);
    let r = row(title, desc, Some(e.upcast_ref()));
    (r, e)
}

pub fn button_row(title: &str, desc: &str, label: &str, on_click: impl Fn(&gtk::Button) + 'static) -> (gtk::Box, gtk::Button) {
    let b = gtk::Button::with_label(label);
    b.connect_clicked(on_click);
    let r = row(title, desc, Some(b.upcast_ref()));
    (r, b)
}

pub fn info_row(title: &str, value: &str) -> (gtk::Box, gtk::Label) {
    let l = gtk::Label::new(Some(value));
    l.add_css_class("dim");
    l.set_selectable(true);
    l.set_wrap(true);
    l.set_xalign(1.0);
    l.set_max_width_chars(48);
    let r = row(title, "", Some(l.upcast_ref()));
    (r, l)
}

pub fn command_button(label: &str, args: &'static [&'static str]) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.connect_clicked(move |_| cmd::spawn(args));
    b
}

/// A button that needs a second click within a few seconds, for things that
/// can't be undone (restart, shut down, removing something).
pub fn confirm_button(label: &str, confirm: &str, on_confirm: impl Fn(&gtk::Button) + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    let (label, confirm) = (label.to_string(), confirm.to_string());
    b.connect_clicked(move |b| {
        if !b.has_css_class("armed") {
            b.add_css_class("armed");
            b.set_label(&confirm);
            let (weak, label) = (b.downgrade(), label.clone());
            glib::timeout_add_seconds_local_once(4, move || {
                if let Some(b) = weak.upgrade() {
                    b.remove_css_class("armed");
                    b.set_label(&label);
                }
            });
            return;
        }
        b.remove_css_class("armed");
        on_confirm(b);
    });
    b
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l.set_xalign(0.0);
    l
}

// ---------- Rows bound to Hyprland options ----------

/// Small circular "reset" button that hands the option back to the user's config.
fn reset_button(key: &str, on_reset: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::from_icon_name("edit-undo-symbolic");
    b.add_css_class("reset-button");
    b.add_css_class("flat");
    b.set_tooltip_text(Some("Reset — use the value from your own config or Omarchy's default"));
    b.set_visible(store::is_managed(key));
    b.set_valign(gtk::Align::Center);
    let k = key.to_string();
    b.connect_clicked(move |btn| {
        store::reset_option(&k);
        btn.set_visible(false);
        on_reset();
    });
    b
}

/// Wait for Hyprland to reload, then re-read a value.
fn after_reload(f: impl Fn() + 'static) {
    glib::timeout_add_local_once(std::time::Duration::from_millis(700), f);
}

pub fn hypr_switch(key: &'static str, title: &str, desc: &str) -> gtk::Box {
    let current = store::option(key).and_then(|v| v.as_bool()).or_else(|| hypr::get_bool(key)).unwrap_or(false);
    let guard = Rc::new(Cell::new(false));
    let reset_slot: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));
    let (r, sw) = {
        let guard = guard.clone();
        let reset_slot = reset_slot.clone();
        switch_row(title, desc, current, move |on| {
            if guard.get() {
                return;
            }
            store::set_option(key, json!(on));
            if let Some(b) = reset_slot.borrow().as_ref() {
                b.set_visible(true);
            }
        })
    };
    let sw2 = sw.clone();
    let reset = reset_button(key, move || {
        let sw = sw2.clone();
        let guard = guard.clone();
        after_reload(move || {
            guard.set(true);
            sw.set_active(hypr::get_bool(key).unwrap_or(false));
            guard.set(false);
        });
    });
    // Reset sits between the text and the control.
    r.insert_child_after(&reset, r.first_child().as_ref());
    *reset_slot.borrow_mut() = Some(reset);
    r
}

#[allow(clippy::too_many_arguments)]
pub fn hypr_slider(
    key: &'static str,
    title: &str,
    desc: &str,
    range: (f64, f64, f64),
    digits: u32,
    unit: &str,
    integer: bool,
) -> gtk::Box {
    let current = store::option(key).and_then(|v| v.as_f64()).or_else(|| hypr::get_f64(key)).unwrap_or(range.0);
    let guard = Rc::new(Cell::new(false));
    let reset_slot: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));
    let (r, s) = {
        let guard = guard.clone();
        let reset_slot = reset_slot.clone();
        slider_row(title, desc, range, current, digits, unit, move |v| {
            if guard.get() {
                return;
            }
            let value = if integer { json!(v.round() as i64) } else { json!((v * 1000.0).round() / 1000.0) };
            store::set_option(key, value);
            if let Some(b) = reset_slot.borrow().as_ref() {
                b.set_visible(true);
            }
        })
    };
    let scale = s.scale.clone();
    let reset = reset_button(key, move || {
        let scale = scale.clone();
        let guard = guard.clone();
        after_reload(move || {
            guard.set(true);
            if let Some(v) = hypr::get_f64(key) {
                scale.set_value(v);
            }
            guard.set(false);
        });
    });
    r.insert_child_after(&reset, r.first_child().as_ref());
    *reset_slot.borrow_mut() = Some(reset);
    r
}

pub fn hypr_choice(key: &'static str, title: &str, desc: &str, options: Vec<(String, String)>) -> gtk::Box {
    hypr_choice_typed(key, title, desc, options, false)
}

/// `numeric`: the option is an integer in Hyprland, the ids are its values.
pub fn hypr_choice_typed(key: &'static str, title: &str, desc: &str, options: Vec<(String, String)>, numeric: bool) -> gtk::Box {
    let as_id = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
        other => other.to_string(),
    };
    let current = store::option(key).map(|v| as_id(&v)).or_else(|| hypr::get_option(key).map(|v| as_id(&v))).unwrap_or_default();
    let guard = Rc::new(Cell::new(false));
    let reset_slot: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));
    let opts2 = options.clone();
    let (r, dd) = {
        let guard = guard.clone();
        let reset_slot = reset_slot.clone();
        choice_row(title, desc, options, &current, move |id| {
            if guard.get() {
                return;
            }
            let value = if numeric { id.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(id)) } else { json!(id) };
            store::set_option(key, value);
            if let Some(b) = reset_slot.borrow().as_ref() {
                b.set_visible(true);
            }
        })
    };
    let dd2 = dd.clone();
    let reset = reset_button(key, move || {
        let dd = dd2.clone();
        let guard = guard.clone();
        let opts = opts2.clone();
        after_reload(move || {
            guard.set(true);
            let now = hypr::get_option(key).map(|v| as_id(&v)).unwrap_or_default();
            if let Some(i) = opts.iter().position(|(id, _)| *id == now) {
                dd.set_selected(i as u32);
            }
            guard.set(false);
        });
    });
    r.insert_child_after(&reset, r.first_child().as_ref());
    *reset_slot.borrow_mut() = Some(reset);
    r
}

pub fn hypr_entry(key: &'static str, title: &str, desc: &str, placeholder: &str) -> gtk::Box {
    let current =
        store::option(key).and_then(|v| v.as_str().map(String::from)).or_else(|| hypr::get_str(key)).unwrap_or_default();
    let reset_slot: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));
    let (r, e) = {
        let reset_slot = reset_slot.clone();
        entry_row(title, desc, &current, placeholder, move |t| {
            store::set_option(key, json!(t));
            if let Some(b) = reset_slot.borrow().as_ref() {
                b.set_visible(true);
            }
        })
    };
    let reset = reset_button(key, move || {
        let e = e.clone();
        after_reload(move || e.set_text(&hypr::get_str(key).unwrap_or_default()));
    });
    r.insert_child_after(&reset, r.first_child().as_ref());
    *reset_slot.borrow_mut() = Some(reset);
    r
}

// ---------- Shared helpers for hardware pages ----------

/// Runs the last call after things have been quiet for a while.
#[derive(Clone, Default)]
pub struct Debounce(Rc<Cell<Option<glib::SourceId>>>);

impl Debounce {
    pub fn call(&self, ms: u64, f: impl FnOnce() + 'static) {
        if let Some(id) = self.0.take() {
            id.remove();
        }
        let slot = self.0.clone();
        self.0.set(Some(glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
            slot.set(None);
            f();
        })));
    }
}

/// A colour picker button starting at `hex` (`#rrggbb`).
pub fn colour_button(hex: &str, on_change: impl Fn(String) + 'static) -> gtk::ColorDialogButton {
    let b = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    b.set_rgba(&gtk::gdk::RGBA::parse(hex).unwrap_or(gtk::gdk::RGBA::WHITE));
    b.connect_rgba_notify(move |b| {
        let c = b.rgba();
        on_change(format!(
            "#{:02x}{:02x}{:02x}",
            (c.red() * 255.0).round() as u8,
            (c.green() * 255.0).round() as u8,
            (c.blue() * 255.0).round() as u8
        ));
    });
    b
}

/// A row of small on/off chips, e.g. "Boot / Awake / Sleep / Shutdown".
pub fn chip_toggles(labels: &[&str], states: &[bool], on_change: impl Fn(Vec<bool>) + 'static) -> gtk::Box {
    let bx = hbox(6);
    let buttons: Rc<Vec<gtk::ToggleButton>> = Rc::new(
        labels
            .iter()
            .zip(states)
            .map(|(l, on)| {
                let b = gtk::ToggleButton::with_label(l);
                b.add_css_class("chip");
                b.set_active(*on);
                if *on {
                    b.add_css_class("selected");
                }
                b
            })
            .collect(),
    );
    let on_change = Rc::new(on_change);
    for b in buttons.iter() {
        bx.append(b);
        let all = buttons.clone();
        let cb = on_change.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                b.add_css_class("selected");
            } else {
                b.remove_css_class("selected");
            }
            cb(all.iter().map(|x| x.is_active()).collect());
        });
    }
    bx
}

// ---------- Segmented control, swatches, disclosure, tags ----------

/// Buttons joined into one control; exactly one is selected.
pub fn segmented(options: &[(String, String)], current: &str, on_change: impl Fn(String) + 'static) -> gtk::Box {
    let bx = hbox(0);
    bx.add_css_class("segmented");
    bx.set_valign(gtk::Align::Center);
    let mut first: Option<gtk::ToggleButton> = None;
    let on_change = Rc::new(on_change);
    for (id, label) in options {
        let b = gtk::ToggleButton::with_label(label);
        b.add_css_class("segment");
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            first = Some(b.clone());
        }
        b.set_active(id == current);
        let id = id.clone();
        let cb = on_change.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                cb(id.clone());
            }
        });
        bx.append(&b);
    }
    bx
}

/// Set a widget's background to a colour (for swatches and previews).
pub fn paint(widget: &impl IsA<gtk::Widget>, css_colour: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("* {{ background: {css_colour}; }}"));
    #[allow(deprecated)]
    widget.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
}

pub const PRESET_COLOURS: &[(&str, &str)] = &[
    ("#ffffff", "White"),
    ("#ff3b30", "Red"),
    ("#ff8c1a", "Orange"),
    ("#ffd60a", "Yellow"),
    ("#34c759", "Green"),
    ("#32d7e6", "Cyan"),
    ("#0a84ff", "Blue"),
    ("#8e5cf7", "Purple"),
    ("#ff4fa3", "Pink"),
];

/// Inline colour picker: optional "Theme" swatch, presets, a custom picker and a hex field.
/// `theme` is (label, colour) for the theme swatch.
pub struct ColourPicker {
    pub widget: gtk::Box,
}

pub fn colour_picker(current: &str, theme: Option<(&str, &str)>, on_change: impl Fn(String) + 'static) -> ColourPicker {
    let outer = hbox(6);
    outer.add_css_class("swatches");
    let swatches: Rc<RefCell<Vec<(String, gtk::Button)>>> = Rc::default();
    let entry = gtk::Entry::new();
    entry.add_css_class("mono");
    entry.set_width_chars(8);
    entry.set_max_length(7);
    let guard = Rc::new(Cell::new(false));
    let on_change: Rc<dyn Fn(String)> = Rc::new(on_change);

    let mark: Rc<dyn Fn(&str)> = {
        let swatches = swatches.clone();
        let entry = entry.clone();
        let guard = guard.clone();
        Rc::new(move |hex: &str| {
            let hex = hex.to_lowercase();
            for (c, b) in swatches.borrow().iter() {
                if *c == hex {
                    b.add_css_class("selected");
                } else {
                    b.remove_css_class("selected");
                }
            }
            guard.set(true);
            entry.set_text(&hex);
            guard.set(false);
        })
    };

    let pick: Rc<dyn Fn(String)> = {
        let mark = mark.clone();
        let on_change = on_change.clone();
        Rc::new(move |hex: String| {
            mark(&hex);
            on_change(hex);
        })
    };

    let add_swatch = |hex: &str, tip: &str, extra_class: Option<&str>| {
        let b = gtk::Button::new();
        b.add_css_class("swatch-button");
        if let Some(c) = extra_class {
            b.add_css_class(c);
        }
        b.set_tooltip_text(Some(tip));
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("swatch-dot");
        paint(&dot, hex);
        b.set_child(Some(&dot));
        let h = hex.to_lowercase();
        let pick = pick.clone();
        let h2 = h.clone();
        b.connect_clicked(move |_| pick(h2.clone()));
        outer.append(&b);
        swatches.borrow_mut().push((h, b));
    };
    if let Some((label, hex)) = theme {
        add_swatch(hex, &format!("{label} ({hex})"), Some("theme-swatch"));
    }
    for (hex, name) in PRESET_COLOURS {
        add_swatch(hex, name, None);
    }

    // Custom: the full picker.
    let custom = gtk::Button::from_icon_name("color-select-symbolic");
    custom.add_css_class("swatch-button");
    custom.set_tooltip_text(Some("Custom colour…"));
    {
        let pick = pick.clone();
        let entry = entry.clone();
        custom.connect_clicked(move |b| {
            let dialog = gtk::ColorDialog::new();
            dialog.set_with_alpha(false);
            let start = gtk::gdk::RGBA::parse(entry.text().as_str()).unwrap_or(gtk::gdk::RGBA::WHITE);
            let window = b.root().and_downcast::<gtk::Window>();
            let pick = pick.clone();
            dialog.choose_rgba(window.as_ref(), Some(&start), gtk::gio::Cancellable::NONE, move |r| {
                if let Ok(c) = r {
                    pick(format!(
                        "#{:02x}{:02x}{:02x}",
                        (c.red() * 255.0).round() as u8,
                        (c.green() * 255.0).round() as u8,
                        (c.blue() * 255.0).round() as u8
                    ));
                }
            });
        });
    }
    outer.append(&custom);

    {
        let pick = pick.clone();
        let guard = guard.clone();
        let apply = move |e: &gtk::Entry| {
            if guard.get() {
                return;
            }
            let t = e.text().trim().to_lowercase();
            let t = if t.starts_with('#') { t } else { format!("#{t}") };
            if t.len() == 7 && t[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                pick(t);
            } else {
                e.add_css_class("error");
            }
        };
        let a2 = apply.clone();
        entry.connect_activate(move |e| a2(e));
        let focus = gtk::EventControllerFocus::new();
        let e2 = entry.clone();
        focus.connect_leave(move |_| apply(&e2));
        entry.add_controller(focus);
        entry.connect_changed(|e| e.remove_css_class("error"));
    }
    outer.append(&entry);
    mark(current);
    ColourPicker { widget: outer }
}

/// A row that reveals more rows when clicked (collapsed by default).
pub fn disclosure(title: &str, desc: &str) -> (gtk::Box, gtk::Box) {
    let content = vbox(6);
    content.set_visible(false);
    let arrow = gtk::Image::from_icon_name("pan-end-symbolic");
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("disclosure");
    let inner = hbox(10);
    inner.append(&arrow);
    let text = vbox(2);
    text.set_hexpand(true);
    let t = label(title, "settings-option-title");
    text.append(&t);
    if !desc.is_empty() {
        let d = label(desc, "settings-option-description");
        d.set_wrap(true);
        text.append(&d);
    }
    inner.append(&text);
    button.set_child(Some(&inner));
    let c = content.clone();
    button.connect_clicked(move |_| {
        let open = !c.is_visible();
        c.set_visible(open);
        arrow.set_icon_name(Some(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }));
    });
    let wrapper = vbox(6);
    wrapper.append(&button);
    wrapper.append(&content);
    register(&button, title, desc, "");
    (wrapper, content)
}

/// A small pill label, e.g. "Restart needed".
pub fn tag(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("tag");
    l.set_valign(gtk::Align::Center);
    l
}

/// Put a tag right after a row's title.
pub fn tag_row(row: &gtk::Box, text: &str) {
    if let Some(title) = row.first_child().and_then(|t| t.first_child())
        && let (Some(parent), Some(t)) = (title.parent().and_downcast::<gtk::Box>(), title.downcast_ref::<gtk::Label>())
    {
        let line = hbox(8);
        parent.insert_child_after(&line, Some(t));
        parent.remove(t);
        line.append(t);
        line.append(&tag(text));
    }
}
