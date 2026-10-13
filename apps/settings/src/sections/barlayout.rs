//! Bar layout: reorder widgets and move them between the left, center and
//! right sections by dragging, or with the buttons on each row.
//!
//! Moves and spacer sizes go through `omarchy-bar`, which asks the running
//! shell (it owns the bar config in memory). Adding or removing one spacer
//! among several has no shell command, so those edit shell.json the way
//! `omarchy-bar` itself does for whole-bar changes: an atomic write followed by
//! `omarchy-shell shell reloadConfig`.

use crate::backend::shell;
use crate::widgets::{self, opts};
use crate::{cmd, window};
use gtk::prelude::*;
use gtk::{gdk, glib};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const SECTIONS: [&str; 3] = ["left", "center", "right"];

fn section_title(s: &str) -> &'static str {
    match s {
        "left" => "Left",
        "center" => "Center",
        _ => "Right",
    }
}

/// A widget's place on the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub section: String,
    pub index: usize,
    pub id: String,
}

pub fn read_layout() -> Vec<Vec<String>> {
    SECTIONS
        .iter()
        .map(|s| {
            shell::get(&["bar", "layout", s])
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|e| match e {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(o) => o.get("id").and_then(|v| v.as_str()).map(String::from),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

pub const SPACER: &str = "omarchy.spacer";
pub const SPACER_DEFAULT: i64 = 12;

/// Each section's raw entries (so per-widget settings like a spacer's size are kept).
fn read_entries() -> Vec<Vec<Value>> {
    SECTIONS.iter().map(|s| shell::get(&["bar", "layout", s]).and_then(|v| v.as_array().cloned()).unwrap_or_default()).collect()
}

fn entry_size(e: &Value) -> i64 {
    e.get("size").and_then(|v| v.as_i64()).unwrap_or(SPACER_DEFAULT)
}

fn entry_id(e: &Value) -> Option<&str> {
    match e {
        Value::String(s) => Some(s),
        Value::Object(o) => o.get("id").and_then(|v| v.as_str()),
        _ => None,
    }
}

/// Add a spacer at the end of a section.
pub fn with_spacer_added(mut config: Value, section: &str, size: i64) -> Option<Value> {
    let list = config.get_mut("bar")?.get_mut("layout")?.get_mut(section)?.as_array_mut()?;
    list.push(serde_json::json!({ "id": SPACER, "size": size }));
    Some(config)
}

/// Remove the entry at `section[index]`, but only if it is `id` (so a stale
/// view can never remove the wrong widget).
pub fn with_entry_removed(mut config: Value, section: &str, index: usize, id: &str) -> Option<Value> {
    let list = config.get_mut("bar")?.get_mut("layout")?.get_mut(section)?.as_array_mut()?;
    if list.get(index).and_then(entry_id) != Some(id) {
        return None;
    }
    list.remove(index);
    Some(config)
}

/// Write shell.json and have the shell reload it, like `omarchy-bar` does.
fn commit_config(config: &Value) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(config)? + "\n";
    cmd::atomic_write(&crate::paths::shell_json(), &text)?;
    if cmd::run(&["omarchy-shell", "shell", "reloadConfig"]).is_err() {
        let _ = cmd::run(&["omarchy-shell", "-q", "shell", "rescanPlugins"]);
    }
    Ok(())
}

/// The `--index` to send so the widget ends up in front of `before` (or at the
/// end when `before` is None). The shell takes the widget out first, so a
/// later position in the same section moves up by one.
pub fn target_index(from: &Slot, to_section: &str, before: Option<usize>, len_to: usize) -> usize {
    let raw = before.unwrap_or(len_to);
    if from.section == to_section && from.index < raw { raw - 1 } else { raw }
}

fn move_args(from: &Slot, to_section: &str, index: usize) -> Vec<String> {
    vec![
        "omarchy-bar".into(),
        "move".into(),
        from.id.clone(),
        "--section".into(),
        to_section.into(),
        "--index".into(),
        index.to_string(),
        "--from-section".into(),
        from.section.clone(),
        "--from-index".into(),
        from.index.to_string(),
    ]
}

/// Widget names and which bar widgets aren't on the bar, from the plugin list.
fn plugins() -> (HashMap<String, String>, Vec<(String, String)>) {
    let list: Vec<Value> = cmd::output(&["omarchy-plugin-list", "--json"])
        .and_then(|t| serde_json::from_str(&t).ok())
        .and_then(|v: Value| v.as_array().cloned())
        .unwrap_or_default();
    let mut names = HashMap::new();
    let mut off_bar = Vec::new();
    for p in &list {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
        let is_widget = p.get("kinds").and_then(|k| k.as_array()).is_some_and(|k| k.iter().any(|x| x == "bar-widget"));
        if is_widget && id != SPACER && !p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            off_bar.push((id.clone(), name.clone()));
        }
        names.insert(id, name);
    }
    names.entry("omarchy.spacer".into()).or_insert_with(|| "Spacer".into());
    off_bar.sort_by_key(|(_, n)| n.to_lowercase());
    (names, off_bar)
}

struct Editor {
    root: gtk::Box,
    busy: std::cell::Cell<bool>,
}

type Ed = Rc<Editor>;

fn run_then_refresh(ed: &Ed, args: Vec<String>) {
    if ed.busy.replace(true) {
        return;
    }
    let ed = ed.clone();
    let label = args[..3.min(args.len())].join(" ");
    cmd::background(
        move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            cmd::run(&refs)
        },
        move |r| {
            if let Err(e) = r {
                window::toast(&format!("{label}: {e}"));
            }
            // The shell writes its config a moment after answering.
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                ed.busy.set(false);
                refresh(&ed);
            });
        },
    );
}

/// Like [`run_then_refresh`], for a shell.json edit.
fn edit_then_refresh(ed: &Ed, change: impl FnOnce(Value) -> Option<Value> + Send + 'static) {
    if ed.busy.replace(true) {
        return;
    }
    let ed = ed.clone();
    cmd::background(
        move || {
            let next = change(shell::read()).ok_or_else(|| "the bar changed meanwhile; try again".to_string())?;
            commit_config(&next).map_err(|e| format!("{e:#}"))
        },
        move |r| {
            if let Err(e) = r {
                window::toast(&format!("Couldn't change the bar: {e}"));
            }
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                ed.busy.set(false);
                refresh(&ed);
            });
        },
    );
}

fn encode(s: &Slot) -> String {
    format!("{}\t{}\t{}", s.section, s.index, s.id)
}

fn decode(v: &str) -> Option<Slot> {
    let mut p = v.splitn(3, '\t');
    Some(Slot { section: p.next()?.to_string(), index: p.next()?.parse().ok()?, id: p.next()?.to_string() })
}

fn clear_drop_marks(w: &gtk::Widget) {
    w.remove_css_class("drop-before");
    w.remove_css_class("drop-after");
}

fn widget_row(ed: &Ed, slot: Slot, name: &str, len: usize, anchor: bool, size: i64) -> gtk::Box {
    let row = widgets::hbox(10);
    row.add_css_class("bar-item");
    let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    handle.add_css_class("dim");
    row.append(&handle);
    let text = widgets::vbox(0);
    text.set_hexpand(true);
    let title_line = widgets::hbox(8);
    title_line.append(&widgets::label(name, "bar-item-name"));
    if anchor {
        let t = widgets::tag("Centered");
        t.set_tooltip_text(Some("The bar keeps this widget in the middle of the screen."));
        title_line.append(&t);
    }
    text.append(&title_line);
    let id = widgets::label(&slot.id, "dim");
    id.add_css_class("mono");
    id.add_css_class("bar-item-id");
    text.append(&id);
    row.append(&text);

    // A spacer's width, right in its row.
    if slot.id == SPACER {
        let spin = gtk::SpinButton::with_range(0.0, 400.0, 2.0);
        spin.set_value(size as f64);
        spin.set_width_chars(4);
        spin.set_valign(gtk::Align::Center);
        spin.set_tooltip_text(Some("Width of the gap, in pixels"));
        let debounce = widgets::Debounce::default();
        let slot2 = slot.clone();
        spin.connect_value_changed(move |s| {
            let (v, slot) = (s.value().round() as i64, slot2.clone());
            debounce.call(400, move || {
                let args = [
                    "omarchy-bar".to_string(),
                    "set".into(),
                    SPACER.into(),
                    "size".into(),
                    v.to_string(),
                    "--json".into(),
                    "--from-section".into(),
                    slot.section.clone(),
                    "--from-index".into(),
                    slot.index.to_string(),
                ];
                cmd::background(
                    move || {
                        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                        cmd::run(&refs)
                    },
                    |r| {
                        if let Err(e) = r {
                            window::toast(&format!("Couldn't resize the spacer: {e}"));
                        }
                    },
                );
            });
        });
        row.append(&spin);
        row.append(&widgets::label("px", "dim"));
    }

    // Up / down within the section.
    for (icon, tip, delta) in [("go-up-symbolic", "Move up", -1i64), ("go-down-symbolic", "Move down", 1)] {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_tooltip_text(Some(tip));
        let target = slot.index as i64 + delta;
        b.set_sensitive(target >= 0 && target < len as i64);
        let (ed, slot) = (ed.clone(), slot.clone());
        b.connect_clicked(move |_| {
            let idx = (slot.index as i64 + delta) as usize;
            run_then_refresh(&ed, move_args(&slot, &slot.section.clone(), idx));
        });
        row.append(&b);
    }

    // Menu: move to another section, or take it off the bar.
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.set_has_frame(false);
    menu.set_tooltip_text(Some("More"));
    let pop = gtk::Popover::new();
    let items = widgets::vbox(2);
    for s in SECTIONS.iter().filter(|s| **s != slot.section) {
        let b = gtk::Button::with_label(&format!("Move to {}", section_title(s)));
        b.add_css_class("flat");
        let (ed, slot, pop, s) = (ed.clone(), slot.clone(), pop.clone(), s.to_string());
        b.connect_clicked(move |_| {
            pop.popdown();
            // To the end of the other section.
            let len_to = read_layout()[SECTIONS.iter().position(|x| *x == s).unwrap_or(0)].len();
            run_then_refresh(&ed, move_args(&slot, &s, len_to));
        });
        items.append(&b);
    }
    let remove = gtk::Button::with_label("Remove from bar");
    remove.add_css_class("flat");
    {
        let (ed, slot, pop) = (ed.clone(), slot.clone(), pop.clone());
        remove.connect_clicked(move |_| {
            pop.popdown();
            if slot.id == SPACER {
                // Turning the spacer plugin off would take every spacer away; remove just this one.
                let slot = slot.clone();
                edit_then_refresh(&ed, move |c| with_entry_removed(c, &slot.section, slot.index, SPACER));
            } else {
                run_then_refresh(&ed, vec!["omarchy-plugin-disable".into(), slot.id.clone()]);
            }
        });
    }
    items.append(&remove);
    pop.set_child(Some(&items));
    menu.set_popover(Some(&pop));
    row.append(&menu);

    // Drag this row…
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    {
        let payload = encode(&slot);
        drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&payload.to_value())));
        let r = row.clone();
        drag.connect_drag_begin(move |src, _| {
            src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&r))), 20, 20);
            r.add_css_class("dragging");
        });
        let r = row.clone();
        drag.connect_drag_end(move |_, _, _| r.remove_css_class("dragging"));
    }
    row.add_controller(drag);

    // …and drop others onto it: above the middle goes before, below goes after.
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gdk::DragAction::MOVE);
    {
        let r = row.clone();
        drop.connect_motion(move |_, _, y| {
            clear_drop_marks(r.upcast_ref());
            r.add_css_class(if y < r.height() as f64 / 2.0 { "drop-before" } else { "drop-after" });
            gdk::DragAction::MOVE
        });
        let r = row.clone();
        drop.connect_leave(move |_| clear_drop_marks(r.upcast_ref()));
        let (r, ed, slot) = (row.clone(), ed.clone(), slot.clone());
        drop.connect_drop(move |_, value, _, y| {
            clear_drop_marks(r.upcast_ref());
            let Some(from) = value.get::<String>().ok().as_deref().and_then(decode) else { return false };
            let before = if y < r.height() as f64 / 2.0 { slot.index } else { slot.index + 1 };
            if from.section == slot.section && (before == from.index || before == from.index + 1) {
                return true; // dropped where it already is
            }
            let len_to = read_layout()[SECTIONS.iter().position(|x| *x == slot.section).unwrap_or(0)].len();
            let idx = target_index(&from, &slot.section, Some(before), len_to);
            run_then_refresh(&ed, move_args(&from, &slot.section, idx));
            true
        });
    }
    row.add_controller(drop);
    row
}

fn section_card(
    ed: &Ed,
    section: &str,
    ids: &[String],
    entries: &[Value],
    names: &HashMap<String, String>,
    anchor: &str,
) -> gtk::Box {
    let card = widgets::vbox(4);
    card.add_css_class("bar-section");
    let top = widgets::hbox(8);
    let head = widgets::label(&format!("{} · {}", section_title(section).to_uppercase(), ids.len()), "bar-section-title");
    head.set_hexpand(true);
    top.append(&head);
    let add_spacer = gtk::Button::with_label("+ Spacer");
    add_spacer.add_css_class("chip");
    add_spacer.set_tooltip_text(Some("Add a gap at the end of this section; drag it where you want it"));
    {
        let (ed, section) = (ed.clone(), section.to_string());
        add_spacer.connect_clicked(move |_| {
            let section = section.clone();
            edit_then_refresh(&ed, move |c| with_spacer_added(c, &section, SPACER_DEFAULT));
        });
    }
    top.append(&add_spacer);
    card.append(&top);
    let list = widgets::vbox(4);
    for (i, id) in ids.iter().enumerate() {
        let name = names.get(id).cloned().unwrap_or_else(|| id.clone());
        let slot = Slot { section: section.to_string(), index: i, id: id.clone() };
        let size = entries.get(i).map(entry_size).unwrap_or(SPACER_DEFAULT);
        list.append(&widget_row(ed, slot, &name, ids.len(), section == "center" && id == anchor, size));
    }
    card.append(&list);

    // An empty strip at the end takes drops "at the end of this section".
    let end = gtk::Label::new(Some(if ids.is_empty() { "Drop widgets here" } else { "" }));
    end.add_css_class("bar-drop-end");
    end.add_css_class("dim");
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gdk::DragAction::MOVE);
    {
        let e = end.clone();
        drop.connect_enter(move |_, _, _| {
            e.add_css_class("drop-before");
            gdk::DragAction::MOVE
        });
        let e = end.clone();
        drop.connect_leave(move |_| e.remove_css_class("drop-before"));
        let (e, ed, section, len) = (end.clone(), ed.clone(), section.to_string(), ids.len());
        drop.connect_drop(move |_, value, _, _| {
            e.remove_css_class("drop-before");
            let Some(from) = value.get::<String>().ok().as_deref().and_then(decode) else { return false };
            let idx = target_index(&from, &section, None, len);
            run_then_refresh(&ed, move_args(&from, &section, idx));
            true
        });
    }
    end.add_controller(drop);
    card.append(&end);
    card
}

fn refresh(ed: &Ed) {
    while let Some(c) = ed.root.first_child() {
        ed.root.remove(&c);
    }
    let layout = read_layout();
    let entries = read_entries();
    let (names, off_bar) = plugins();
    let anchor = shell::get(&["bar", "centerAnchor"]).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    for (i, s) in SECTIONS.iter().enumerate() {
        ed.root.append(&section_card(ed, s, &layout[i], &entries[i], &names, &anchor));
    }

    // Add a widget that isn't on the bar.
    if !off_bar.is_empty() {
        let line = widgets::hbox(10);
        line.add_css_class("bar-add");
        let pick = widgets::dropdown(&off_bar, &off_bar[0].0);
        pick.set_hexpand(true);
        line.append(&pick);
        let section = Rc::new(RefCell::new("right".to_string()));
        let s2 = section.clone();
        let seg = widgets::segmented(&opts(&[("left", "Left"), ("center", "Center"), ("right", "Right")]), "right", move |v| {
            *s2.borrow_mut() = v;
        });
        line.append(&seg);
        let add = gtk::Button::with_label("Add");
        add.add_css_class("suggested-action");
        {
            let (ed, off_bar) = (ed.clone(), off_bar.clone());
            add.connect_clicked(move |_| {
                let Some((id, _)) = off_bar.get(pick.selected() as usize) else { return };
                run_then_refresh(
                    &ed,
                    vec!["omarchy-bar".into(), "put".into(), id.clone(), "--section".into(), section.borrow().clone()],
                );
            });
        }
        line.append(&add);
        let card = widgets::vbox(6);
        card.add_css_class("bar-section");
        let head = widgets::label("ADD A WIDGET", "bar-section-title");
        card.append(&head);
        card.append(&line);
        ed.root.append(&card);
    }
}

/// The editor: three section lists and an "add a widget" line.
pub fn editor() -> gtk::Box {
    let root = widgets::vbox(10);
    let ed: Ed = Rc::new(Editor { root: root.clone(), busy: std::cell::Cell::new(false) });
    refresh(&ed);
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(section: &str, index: usize) -> Slot {
        Slot { section: section.into(), index, id: "x".into() }
    }

    #[test]
    fn moving_down_in_the_same_section_accounts_for_removal() {
        // [a b c d]: drop b (1) before d (3) -> ends at index 2.
        assert_eq!(target_index(&slot("left", 1), "left", Some(3), 4), 2);
        // Drop b at the end -> index 3 (last).
        assert_eq!(target_index(&slot("left", 1), "left", None, 4), 3);
    }

    #[test]
    fn moving_up_or_across_sections_uses_the_position_as_is() {
        assert_eq!(target_index(&slot("left", 3), "left", Some(0), 4), 0);
        assert_eq!(target_index(&slot("left", 1), "right", Some(2), 5), 2);
        assert_eq!(target_index(&slot("left", 1), "right", None, 5), 5);
    }

    #[test]
    fn move_targets_the_exact_entry() {
        let a = move_args(&Slot { section: "left".into(), index: 2, id: "omarchy.spacer".into() }, "right", 0);
        assert_eq!(
            a,
            [
                "omarchy-bar",
                "move",
                "omarchy.spacer",
                "--section",
                "right",
                "--index",
                "0",
                "--from-section",
                "left",
                "--from-index",
                "2"
            ]
        );
    }

    fn config() -> Value {
        serde_json::json!({
            "version": 1,
            "bar": { "position": "top", "layout": {
                "left": [{ "id": "omarchy.menu" }, { "id": "omarchy.spacer", "size": 20 }],
                "center": [{ "id": "omarchy.clock" }],
                "right": []
            }},
            "plugins": []
        })
    }

    #[test]
    fn adds_a_spacer_at_the_end_keeping_everything_else() {
        let c = with_spacer_added(config(), "right", 12).unwrap();
        assert_eq!(c["bar"]["layout"]["right"][0], serde_json::json!({ "id": "omarchy.spacer", "size": 12 }));
        assert_eq!(c["bar"]["layout"]["left"].as_array().unwrap().len(), 2);
        assert_eq!(c["bar"]["position"], "top");
        // A second spacer in the same section is fine.
        let c = with_spacer_added(c, "left", 8).unwrap();
        assert_eq!(c["bar"]["layout"]["left"][2]["size"], 8);
    }

    #[test]
    fn removes_only_the_expected_entry() {
        let c = with_entry_removed(config(), "left", 1, "omarchy.spacer").unwrap();
        assert_eq!(c["bar"]["layout"]["left"].as_array().unwrap().len(), 1);
        assert_eq!(c["bar"]["layout"]["left"][0]["id"], "omarchy.menu");
        // Wrong widget at that index (the layout changed meanwhile): nothing is removed.
        assert!(with_entry_removed(config(), "left", 0, "omarchy.spacer").is_none());
        assert!(with_entry_removed(config(), "left", 9, "omarchy.spacer").is_none());
    }

    #[test]
    fn spacer_size_defaults() {
        assert_eq!(entry_size(&serde_json::json!({ "id": "omarchy.spacer" })), 12);
        assert_eq!(entry_size(&serde_json::json!({ "id": "omarchy.spacer", "size": 40 })), 40);
    }

    #[test]
    fn drag_payload_round_trips() {
        let s = Slot { section: "center".into(), index: 4, id: "omarchy.clock".into() };
        assert_eq!(decode(&encode(&s)), Some(s));
        assert_eq!(decode("junk"), None);
    }
}
