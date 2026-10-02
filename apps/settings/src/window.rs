//! The main window: navigation sidebar with search, and a stack of
//! section pages that are built the first time they're shown.

use crate::sections::{self, Section};
use crate::widgets::{self, SEARCH};
use crate::{prefs, theme};
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

struct Ui {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    nav_list: gtk::Box,
    nav_items: HashMap<&'static str, gtk::Button>,
    nav_groups: Vec<(gtk::Label, Vec<&'static str>)>,
    /// Hidden in the icon-only sidebar: fixed parts, and the labels of the current items.
    compact_fixed: Vec<gtk::Widget>,
    compact_items: Vec<gtk::Widget>,
    /// A Cell: it's set while the window is being shown, when the UI is already borrowed.
    compact: std::cell::Cell<bool>,
    pages: HashMap<&'static str, gtk::ScrolledWindow>,
    sections: Vec<Section>,
    current: &'static str,
    overlay: gtk::Overlay,
}

thread_local! {
    static UI: RefCell<Option<Rc<RefCell<Ui>>>> = const { RefCell::new(None) };
}

fn ui() -> Option<Rc<RefCell<Ui>>> {
    UI.with(|u| u.borrow().clone())
}

pub fn present(app: &gtk::Application, section: Option<&str>) {
    if let Some(ui) = ui() {
        let window = ui.borrow().window.clone();
        if let Some(s) = section {
            navigate(s);
        }
        window.present();
        return;
    }
    theme::install();
    build(app);
    let start = section.map(String::from).unwrap_or_else(|| prefs::get().last_section);
    navigate(&start);
    // Developer aid: SETTINGS_SNAPSHOT=/path.png renders the window to a PNG
    // (invisibly) and quits, so layouts can be checked without a visible window.
    if let Some(out) = std::env::var_os("SETTINGS_SNAPSHOT") {
        snapshot_and_quit(app, std::path::PathBuf::from(out));
        return;
    }
    // Clone first: presenting realizes the window, and its handlers borrow the UI.
    if let Some(window) = ui().map(|u| u.borrow().window.clone()) {
        window.present();
    }
    // Bring the keyboard backlight timeout helper back if it isn't running.
    crate::cmd::background(crate::backend::kbdidle::ensure_running, |_| {});
    // Keep the theme hook that re-applies the chosen icon theme and extension settings in place.
    crate::cmd::background(crate::backend::themehook::ensure, |_| {});
    // Look for updates (at most every 20 hours); announce a new Settings once, and
    // install extension updates if that's switched on.
    if std::env::var_os("SETTINGS_SNAPSHOT").is_none() {
        crate::cmd::background(|| crate::backend::updates::check(false), after_update_check);
    }
    // The sidebar used what extensions said last time; ask again in case devices came or went.
    crate::cmd::background(crate::ext::refresh_pages, |changed| {
        if changed {
            reload_sections(false);
        }
    });
}

fn build(app: &gtk::Application) {
    let window =
        gtk::ApplicationWindow::builder().application(app).title("Settings").default_width(1120).default_height(800).build();
    window.add_css_class("settings-window");
    // No client-side titlebar: Hyprland manages the window.
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_icon_name(Some("io.github.design_nexus.Settings"));

    let sections = sections::all();

    // ----- Sidebar -----
    let nav = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav.add_css_class("settings-navigation");
    // Labels inside expand; don't let that widen the sidebar itself.
    nav.set_hexpand(false);
    let heading = widgets::label("SETTINGS", "menu-heading");
    let mut compact_hide: Vec<gtk::Widget> = vec![heading.clone().upcast()];
    nav.append(&heading);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search settings"));
    search.add_css_class("settings-search");
    nav.append(&search);
    compact_hide.push(search.clone().upcast());

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let (nav_items, nav_groups, compact_items) = fill_nav(&list, &sections);
    let nav_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        // Scrolls with wheel, trackpad and keyboard; no visible scrollbar.
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    nav.append(&nav_scroll);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    footer.add_css_class("nav-footer");
    let version = widgets::label(concat!("Settings ", env!("CARGO_PKG_VERSION")), "dim");
    version.set_hexpand(true);
    footer.append(&version);
    nav.append(&footer);
    compact_hide.push(footer.clone().upcast());

    // ----- Content -----
    let stack = gtk::Stack::new();
    stack.add_css_class("settings-content");
    stack.set_hexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(if prefs::get().reduce_motion { 0 } else { 160 });

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&nav);
    body.append(&stack);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&body));
    window.set_child(Some(&overlay));

    // ----- Keys -----
    let keys = gtk::EventControllerKey::new();
    let s2 = search.clone();
    let w2 = window.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::f if ctrl => {
                s2.grab_focus();
                glib::Propagation::Stop
            }
            gdk::Key::q | gdk::Key::w if ctrl => {
                w2.close();
                glib::Propagation::Stop
            }
            gdk::Key::Escape if !s2.text().is_empty() => {
                s2.set_text("");
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(keys);
    search.connect_search_changed(|e| filter(&e.text()));
    search.connect_activate(|_| focus_first_hit());

    // Narrow windows (a tiled half-screen) get an icon-only sidebar.
    let apply_width = {
        let nav = nav.clone();
        move |w: &gtk::ApplicationWindow| {
            let width = if w.width() > 0 { w.width() } else { w.default_width() };
            let compact = width > 0 && width < 980;
            if compact == nav.has_css_class("compact") && nav.has_css_class("sized") {
                return;
            }
            nav.add_css_class("sized");
            if compact {
                nav.add_css_class("compact");
            } else {
                nav.remove_css_class("compact");
            }
            if let Some(ui) = ui() {
                let u = ui.borrow();
                u.compact.set(compact);
                for wdg in u.compact_fixed.iter().chain(&u.compact_items) {
                    wdg.set_visible(!compact);
                }
            }
            set_narrow(compact);
        }
    };
    let aw = apply_width.clone();
    window.connect_default_width_notify(move |w| aw(w));
    let aw = apply_width.clone();
    window.connect_realize(move |w| aw(w));
    // Tiled windows are resized by the compositor; watch the real size too.
    let w2 = window.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
        apply_width(&w2);
        glib::ControlFlow::Continue
    });

    window.connect_close_request(|_| {
        crate::backend::store::flush();
        glib::Propagation::Proceed
    });

    let ui = Ui {
        window,
        stack,
        nav_list: list,
        nav_items,
        nav_groups,
        compact_fixed: compact_hide,
        compact_items,
        compact: std::cell::Cell::new(false),
        pages: HashMap::new(),
        sections,
        current: "",
        overlay,
    };
    UI.with(|u| *u.borrow_mut() = Some(Rc::new(RefCell::new(ui))));
}

type NavParts = (HashMap<&'static str, gtk::Button>, Vec<(gtk::Label, Vec<&'static str>)>, Vec<gtk::Widget>);

/// The sidebar entries: a heading per group, a button per page.
fn fill_nav(list: &gtk::Box, sections: &[Section]) -> NavParts {
    let mut nav_items = HashMap::new();
    let mut nav_groups: Vec<(gtk::Label, Vec<&'static str>)> = Vec::new();
    let mut compact_items: Vec<gtk::Widget> = Vec::new();
    let mut last_group = "";
    for s in sections {
        if !(s.visible)() {
            continue;
        }
        if s.group != last_group {
            let g = widgets::label(&s.group.to_uppercase(), "nav-group");
            compact_items.push(g.clone().upcast());
            list.append(&g);
            nav_groups.push((g, Vec::new()));
            last_group = s.group;
        }
        let button = gtk::Button::new();
        button.add_css_class("nav-item");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.append(&gtk::Image::from_icon_name(s.icon));
        let l = widgets::label(s.title, "nav-label");
        l.set_hexpand(true);
        compact_items.push(l.clone().upcast());
        content.append(&l);
        button.set_child(Some(&content));
        button.set_tooltip_text(Some(s.description));
        let id = s.id;
        button.connect_clicked(move |_| navigate(id));
        list.append(&button);
        nav_items.insert(s.id, button);
        if let Some(g) = nav_groups.last_mut() {
            g.1.push(s.id);
        }
    }
    (nav_items, nav_groups, compact_items)
}

/// What to do with a finished update check.
pub fn after_update_check(s: crate::backend::updates::Status) {
    if s.settings_update() && prefs::get().announced_version != s.latest {
        toast(&format!("Settings {} is available: see Updates & About", s.latest));
        prefs::update(|p| p.announced_version = s.latest.clone());
    }
    if prefs::get().auto_update_extensions && !s.extensions.is_empty() {
        crate::cmd::background(
            || crate::backend::updates::update_extensions().map_err(|e| format!("{e:#}")),
            |r| match r {
                Ok(names) if !names.is_empty() => {
                    toast(&format!("Updated {}", names.join(", ")));
                    reload_sections(false);
                }
                Ok(_) => {}
                Err(e) => toast(&format!("Couldn't update extensions: {e}")),
            },
        );
    }
    rebuild_if_built("about");
    rebuild_if_built("extensions");
}

/// Start the installed Settings again on `section`, and quit this one.
pub fn restart(section: &str) {
    use std::os::unix::process::CommandExt;
    let exe = crate::paths::home().join(".local/bin/settings");
    let script = format!("sleep 1; exec '{}' --section '{}'", exe.display(), section.replace('\'', ""));
    let started = std::process::Command::new("sh")
        .args(["-c", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = started {
        toast(&format!("Couldn't restart: {e}"));
        return;
    }
    crate::backend::store::flush();
    if let Some(app) = window().and_then(|w| w.application()) {
        app.quit();
    }
}

/// Re-read the list of pages (after an extension is installed, updated or removed,
/// or reports different hardware). Extension pages are built again when next shown.
/// `fresh` asks every extension again instead of using what it said last time.
pub fn reload_sections(fresh: bool) {
    let Some(ui) = ui() else { return };
    let sections = sections::all_with(fresh);
    let mut u = ui.borrow_mut();
    while let Some(c) = u.nav_list.first_child() {
        u.nav_list.remove(&c);
    }
    let (items, groups, compact_items) = fill_nav(&u.nav_list, &sections);
    for w in &compact_items {
        w.set_visible(!u.compact.get());
    }
    // Drop built extension pages (their page may have changed) and pages that are gone.
    let stale: Vec<&'static str> = u
        .pages
        .keys()
        .copied()
        .filter(|id| {
            sections.iter().find(|s| s.id == *id).is_none_or(|s| matches!(s.build, sections::Build::Extension(_)))
        })
        .collect();
    for id in &stale {
        if let Some(p) = u.pages.remove(id) {
            SEARCH.with(|s| s.borrow_mut().retain(|item| item.section != *id));
            u.stack.remove(&p);
        }
    }
    u.nav_items = items;
    u.nav_groups = groups;
    u.compact_items = compact_items;
    u.sections = sections;
    let current = u.current;
    u.current = "";
    drop(u);
    navigate(current);
}

thread_local! {
    static NARROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn set_narrow(narrow: bool) {
    NARROW.with(|n| n.set(narrow));
    let Some(ui) = ui() else { return };
    for page in ui.borrow().pages.values() {
        mark_page(page, narrow);
    }
}

fn mark_page(page: &gtk::ScrolledWindow, narrow: bool) {
    if let Some(body) = page.child().and_then(|v| v.first_child()) {
        if narrow {
            body.add_css_class("narrow");
        } else {
            body.remove_css_class("narrow");
        }
    }
}

fn ensure_built(id: &'static str) {
    let Some(ui) = ui() else { return };
    if ui.borrow().pages.contains_key(id) {
        return;
    }
    let section = {
        let u = ui.borrow();
        u.sections.iter().find(|s| s.id == id).cloned()
    };
    let Some(section) = section else { return };
    let sid = section.id;
    let page = widgets::page(sid, section.title, section.description, &section.config_files());
    section.run(&page);
    mark_page(&page.root, NARROW.with(|n| n.get()));
    let stack = ui.borrow().stack.clone();
    stack.add_named(&page.root, Some(sid));
    ui.borrow_mut().pages.insert(sid, page.root);
}

pub fn navigate(id: &str) {
    let Some(ui) = ui() else { return };
    let resolved = {
        let u = ui.borrow();
        let shown = |s: &&Section| (s.visible)();
        u.sections
            .iter()
            .filter(shown)
            .find(|s| s.id == id)
            // An extension page asked for by its own id (`--section aura` finds `asus.aura`).
            .or_else(|| u.sections.iter().filter(shown).find(|s| s.id.split_once('.').is_some_and(|(_, p)| p == id)))
            // An extension's own id opens its first page (`--section webcam`).
            .or_else(|| u.sections.iter().filter(shown).find(|s| s.id.split_once('.').is_some_and(|(e, _)| e == id)))
            .or_else(|| u.sections.first())
            .map(|s| s.id)
    };
    let Some(id) = resolved else { return };
    ensure_built(id);
    let mut u = ui.borrow_mut();
    if let Some(prev) = u.nav_items.get(u.current) {
        prev.remove_css_class("active");
    }
    if let Some(b) = u.nav_items.get(id) {
        b.add_css_class("active");
    }
    u.stack.set_visible_child_name(id);
    u.current = id;
    drop(u);
    prefs::update(|p| p.last_section = id.to_string());
}

/// Rebuild a section page from scratch (after a change that alters its layout).
pub fn rebuild(id: &'static str) {
    let Some(ui) = ui() else { return };
    let old = ui.borrow_mut().pages.remove(id);
    if let Some(old) = old {
        SEARCH.with(|s| s.borrow_mut().retain(|item| item.section != id));
        ui.borrow().stack.remove(&old);
    }
    let current = ui.borrow().current;
    ensure_built(id);
    if current == id {
        ui.borrow().stack.set_visible_child_name(id);
    }
}

/// Rebuild a page only if it has been opened already (its content is stale otherwise).
pub fn rebuild_if_built(id: &'static str) {
    let built = ui().is_some_and(|u| u.borrow().pages.contains_key(id));
    if built {
        rebuild(id);
    }
}

fn filter(query: &str) {
    let Some(ui) = ui() else { return };
    let q = query.trim().to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();

    if !terms.is_empty() {
        let ids: Vec<&'static str> = ui.borrow().sections.iter().filter(|s| (s.visible)()).map(|s| s.id).collect();
        for id in ids {
            ensure_built(id);
        }
    }

    let mut section_hits: HashMap<String, usize> = HashMap::new();
    let title_hits: Vec<&'static str> = {
        let u = ui.borrow();
        u.sections
            .iter()
            .filter(|s| {
                let hay = format!("{} {} {}", s.title, s.description, s.keywords).to_lowercase();
                !terms.is_empty() && terms.iter().all(|t| hay.contains(t))
            })
            .map(|s| s.id)
            .collect()
    };

    SEARCH.with(|s| {
        let items = s.borrow();
        let mut groups_visible: HashMap<gtk::Widget, bool> = HashMap::new();
        for item in items.iter() {
            item.row.remove_css_class("search-hit");
            let hit = !terms.is_empty() && terms.iter().all(|t| item.text.contains(t));
            let whole_section = title_hits.iter().any(|id| *id == item.section);
            let show = terms.is_empty() || hit || whole_section;
            item.row.set_visible(show);
            if hit {
                *section_hits.entry(item.section.clone()).or_default() += 1;
            }
            if let Some(g) = &item.group {
                let e = groups_visible.entry(g.clone()).or_insert(false);
                *e |= show;
            }
        }
        for (g, visible) in groups_visible {
            g.set_visible(visible);
        }
    });

    let u = ui.borrow();
    let mut first_match: Option<&'static str> = None;
    for s in &u.sections {
        let Some(button) = u.nav_items.get(s.id) else { continue };
        let visible = terms.is_empty() || section_hits.contains_key(s.id) || title_hits.contains(&s.id);
        button.set_visible(visible);
        if visible && first_match.is_none() && !terms.is_empty() {
            first_match = Some(s.id);
        }
    }
    for (label, ids) in &u.nav_groups {
        label.set_visible(ids.iter().any(|id| u.nav_items.get(id).is_some_and(|b| b.is_visible())));
    }
    let current = u.current;
    let current_visible = u.nav_items.get(current).is_some_and(|b| b.is_visible());
    drop(u);
    if let Some(first) = first_match
        && (!current_visible || !section_hits.contains_key(current))
    {
        navigate(first);
    }
    highlight_first_hit(&terms);
}

fn highlight_first_hit(terms: &[&str]) {
    if terms.is_empty() {
        return;
    }
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let row = SEARCH.with(|s| {
        s.borrow().iter().find(|i| i.section == current && terms.iter().all(|t| i.text.contains(t))).map(|i| i.row.clone())
    });
    if let Some(row) = row {
        row.add_css_class("search-hit");
        scroll_to(&row);
    }
}

fn scroll_to(row: &gtk::Widget) {
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let Some(page) = ui.borrow().pages.get(current).cloned() else { return };
    let row = row.clone();
    glib::idle_add_local_once(move || {
        if let Some(child) = page.child()
            && let Some(p) = row.compute_point(&child, &gtk::graphene::Point::new(0.0, 0.0))
        {
            let adj = page.vadjustment();
            adj.set_value((p.y() as f64 - 80.0).max(0.0));
        }
    });
}

fn focus_first_hit() {
    let Some(ui) = ui() else { return };
    let current = ui.borrow().current;
    let row = SEARCH
        .with(|s| s.borrow().iter().find(|i| i.section == current && i.row.has_css_class("search-hit")).map(|i| i.row.clone()));
    if let Some(row) = row {
        row.child_focus(gtk::DirectionType::TabForward);
    }
}

/// Show a short message at the bottom of the window.
pub fn toast(message: &str) {
    let Some(ui) = ui() else {
        eprintln!("settings: {message}");
        return;
    };
    let overlay = ui.borrow().overlay.clone();
    let label = gtk::Label::new(Some(message));
    label.set_wrap(true);
    label.set_max_width_chars(70);
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bx.add_css_class("toast");
    bx.append(&label);
    bx.set_halign(gtk::Align::Center);
    bx.set_valign(gtk::Align::End);
    overlay.add_overlay(&bx);
    glib::timeout_add_local_once(std::time::Duration::from_millis(3500), move || {
        overlay.remove_overlay(&bx);
    });
}

pub fn window() -> Option<gtk::ApplicationWindow> {
    ui().map(|u| u.borrow().window.clone())
}

fn snapshot_and_quit(app: &gtk::Application, out: std::path::PathBuf) {
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    window.set_opacity(0.01);
    // A distinct title lets a window rule float it at a set size for screenshots.
    window.set_title(Some("Settings snapshot"));
    window.set_default_size(
        std::env::var("SETTINGS_SNAPSHOT_W").ok().and_then(|v| v.parse().ok()).unwrap_or(1120),
        std::env::var("SETTINGS_SNAPSHOT_H").ok().and_then(|v| v.parse().ok()).unwrap_or(820),
    );
    window.present();
    let app = app.clone();
    let wait = std::env::var("SETTINGS_SNAPSHOT_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(1800);
    glib::timeout_add_local_once(std::time::Duration::from_millis(wait), move || {
        // SETTINGS_SNAPSHOT_PAGE=1 renders the whole current page, not just what fits.
        let target = if std::env::var_os("SETTINGS_SNAPSHOT_PAGE").is_some() {
            UI.with(|cell| cell.borrow().clone()).and_then(|u| {
                let u = u.borrow();
                u.pages.get(u.current).and_then(|p| p.child()).and_then(|v| v.first_child())
            })
        } else {
            window.child()
        };
        if let Some(child) = target {
            let paintable = gtk::WidgetPaintable::new(Some(&child));
            let (w, h) = (child.width(), child.height());
            let snapshot = gtk::Snapshot::new();
            snapshot.append_color(&gdk::RGBA::BLACK, &gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
            paintable.snapshot(&snapshot, w as f64, h as f64);
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer()) {
                let texture = renderer.render_texture(node, None);
                match texture.save_to_png(&out) {
                    Ok(()) => println!("snapshot {w}x{h} -> {}", out.display()),
                    Err(e) => eprintln!("snapshot failed: {e}"),
                }
            }
        }
        app.quit();
    });
}
