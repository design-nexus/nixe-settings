//! Disks: every drive with a map of its partitions; pick one to see what's on
//! it and to mount, rename, resize, format, check, encrypt or delete it. Free
//! space makes new partitions. UDisks does the work and asks for permission.
//!
//! Anything that erases needs the partition's name typed. Partitions in use
//! can't be changed until they're unmounted; the system drive and another
//! system's partitions (Windows) can, after a red warning.

use crate::backend::sysinfo::bytes_text;
use crate::backend::udisks::{self, Drive, Make, Part, Segment};
use crate::sketch::{self, Rgb, mix, rounded, set};
use crate::widgets::{self, Page};
use crate::{cmd, window};
use anyhow::Result;
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

/// What's picked on a drive's map: a partition (its block path) or free space
/// (its offset).
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Part(String),
    Free(u64),
}

thread_local! {
    /// The selection on each drive (by drive block), kept across rebuilds.
    static PICKED: RefCell<Vec<(String, Pick)>> = const { RefCell::new(Vec::new()) };
    /// A change running: (drive block, what it is).
    static BUSY: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// The progress bar showing it, on the page as last built.
    static PROGRESS: RefCell<Option<gtk::ProgressBar>> = const { RefCell::new(None) };
}

fn picked(drive: &str) -> Option<Pick> {
    PICKED.with(|p| p.borrow().iter().find(|(d, _)| d == drive).map(|(_, p)| p.clone()))
}

fn pick(drive: &str, p: Pick) {
    PICKED.with(|v| {
        let mut v = v.borrow_mut();
        v.retain(|(d, _)| d != drive);
        v.push((drive.to_string(), p));
    });
}

fn rebuild() {
    window::rebuild_if_built("disks");
}

/// Run a change on `drive`: show it as busy with progress, then say how it went.
fn run<F>(drive: &str, what: &str, done: &'static str, fut: F)
where
    F: Future<Output = Result<()>> + 'static,
{
    if BUSY.with(|b| b.borrow().is_some()) {
        window::toast("Wait for the change that's running to finish.");
        return;
    }
    BUSY.with(|b| *b.borrow_mut() = Some((drive.to_string(), what.to_string())));
    rebuild();
    let drive = drive.to_string();
    // Follow UDisks' progress for the drive's objects.
    let poll = glib::timeout_add_local(std::time::Duration::from_millis(700), move || {
        let fraction = udisks::load()
            .ok()
            .and_then(|ds| ds.into_iter().find(|d| d.block == drive))
            .map(|d| {
                let mut objs = vec![d.block.clone()];
                objs.extend(d.parts.iter().flat_map(|p| [p.path.clone(), p.content().path.clone()]));
                objs.iter().flat_map(|o| udisks::jobs_for(o)).find_map(|(_, f)| f)
            })
            .unwrap_or(None);
        PROGRESS.with(|p| {
            if let Some(bar) = p.borrow().as_ref() {
                match fraction {
                    Some(f) => bar.set_fraction(f),
                    None => bar.pulse(),
                }
            }
        });
        glib::ControlFlow::Continue
    });
    glib::spawn_future_local(async move {
        let r = fut.await;
        poll.remove();
        BUSY.with(|b| *b.borrow_mut() = None);
        match r {
            Ok(()) => window::toast(done),
            Err(e) => window::toast(&format!("{e}")),
        }
        rebuild();
    });
}

// ---------- Colours and the map ----------

fn fs_colour(p: &Part) -> Rgb {
    let fs = if p.encrypted { "crypto_LUKS" } else { p.fs.as_str() };
    match fs {
        "ext4" | "ext3" | "ext2" => (0.42, 0.62, 0.96),
        "btrfs" => (0.68, 0.55, 0.96),
        "vfat" => (0.90, 0.69, 0.38),
        "exfat" => (0.55, 0.80, 0.42),
        "ntfs" => (0.38, 0.78, 0.80),
        "xfs" => (0.95, 0.55, 0.40),
        "crypto_LUKS" => (0.93, 0.47, 0.65),
        "swap" => (0.62, 0.64, 0.70),
        _ => (0.50, 0.52, 0.60),
    }
}

/// Lay segments of `sizes` across `width`: each at least `min` wide, the rest
/// in proportion. Returns (x, width) per segment.
fn layout(sizes: &[u64], width: f64, gap: f64, min: f64) -> Vec<(f64, f64)> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }
    let room = (width - gap * (n as f64 - 1.0)).max(1.0);
    let total: f64 = sizes.iter().map(|s| *s as f64).sum::<f64>().max(1.0);
    let mut w: Vec<f64> = sizes.iter().map(|s| *s as f64 / total * room).collect();
    // Small segments get the minimum; the others share what's left.
    for _ in 0..3 {
        let small: f64 = w.iter().filter(|x| **x < min).count() as f64 * min;
        let big: f64 = w.iter().filter(|x| **x >= min).sum();
        if big <= 0.0 {
            break;
        }
        let k = ((room - small) / big).max(0.0);
        w = w.iter().map(|x| if *x < min { min } else { x * k }).collect();
    }
    let mut x = 0.0;
    w.iter()
        .map(|wi| {
            let out = (x, *wi);
            x += wi + gap;
            out
        })
        .collect()
}

fn map(d: &Drive) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(66);
    area.set_hexpand(true);
    area.add_css_class("disk-map");
    let segs = d.segments();
    let sizes: Vec<u64> = segs
        .iter()
        .map(|s| match s {
            Segment::Part(i) => d.parts[*i].size,
            Segment::Free { size, .. } => *size,
        })
        .collect();
    let d2 = d.clone();
    let (segs2, sizes2) = (segs.clone(), sizes.clone());
    area.set_draw_func(move |_, cr, w, h| {
        let p = sketch::pal();
        let (w, h) = (w as f64, h as f64);
        let picked = picked(&d2.block);
        for ((x, sw), seg) in layout(&sizes2, w, 3.0, 30.0).into_iter().zip(&segs2) {
            let (fill, title, size, chosen) = match seg {
                Segment::Part(i) => {
                    let part = &d2.parts[*i];
                    (Some(fs_colour(part)), part.title(), part.size, picked == Some(Pick::Part(part.path.clone())))
                }
                Segment::Free { offset, size } => (None, "Free".to_string(), *size, picked == Some(Pick::Free(*offset))),
            };
            rounded(cr, x + 1.0, 1.0, sw - 2.0, h - 2.0, 7.0);
            match fill {
                Some(c) => {
                    set(cr, mix(p.surface, c, 0.30), 1.0);
                    let _ = cr.fill_preserve();
                    set(cr, c, 0.9);
                    cr.set_line_width(1.0);
                    let _ = cr.stroke();
                    // A strip of the filesystem's colour along the top.
                    cr.save().ok();
                    rounded(cr, x + 1.0, 1.0, sw - 2.0, h - 2.0, 7.0);
                    cr.clip();
                    set(cr, c, 1.0);
                    cr.rectangle(x, 0.0, sw, 5.0);
                    let _ = cr.fill();
                    cr.restore().ok();
                }
                None => {
                    // Free space: hatched.
                    cr.save().ok();
                    cr.clip_preserve();
                    set(cr, p.dim, 0.18);
                    cr.set_line_width(1.0);
                    let mut k = -h;
                    while k < sw {
                        cr.move_to(x + k, h);
                        cr.line_to(x + k + h, 0.0);
                        k += 7.0;
                    }
                    let _ = cr.stroke();
                    cr.restore().ok();
                    rounded(cr, x + 1.0, 1.0, sw - 2.0, h - 2.0, 7.0);
                    set(cr, p.dim, 0.5);
                    cr.set_dash(&[3.0, 3.0], 0.0);
                    let _ = cr.stroke();
                    cr.set_dash(&[], 0.0);
                }
            }
            if chosen {
                rounded(cr, x + 1.0, 1.0, sw - 2.0, h - 2.0, 7.0);
                set(cr, p.accent, 1.0);
                cr.set_line_width(2.5);
                let _ = cr.stroke();
            }
            if sw > 64.0 {
                cr.save().ok();
                cr.rectangle(x + 6.0, 0.0, sw - 12.0, h);
                cr.clip();
                cr.select_font_face("sans-serif", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
                cr.set_font_size(12.0);
                set(cr, p.text, 1.0);
                cr.move_to(x + 9.0, h / 2.0 + 1.0);
                let _ = cr.show_text(&title);
                cr.select_font_face("sans-serif", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
                cr.set_font_size(11.0);
                set(cr, p.dim, 1.0);
                cr.move_to(x + 9.0, h / 2.0 + 17.0);
                let _ = cr.show_text(&bytes_text(size));
                cr.restore().ok();
            }
        }
    });
    let click = gtk::GestureClick::new();
    let (d3, a2) = (d.clone(), area.clone());
    click.connect_pressed(move |_, _, x, _| {
        let w = a2.width() as f64;
        for ((sx, sw), seg) in layout(&sizes, w, 3.0, 30.0).into_iter().zip(&segs) {
            if x >= sx && x <= sx + sw {
                let p = match seg {
                    Segment::Part(i) => Pick::Part(d3.parts[*i].path.clone()),
                    Segment::Free { offset, .. } => Pick::Free(*offset),
                };
                pick(&d3.block, p);
                rebuild();
                return;
            }
        }
    });
    area.add_controller(click);
    area.set_cursor_from_name(Some("pointer"));
    area
}

// ---------- Dialogs ----------

/// A form in a dialog. `on_ok` returns an error to show, or None to close.
pub(super) fn form(
    title: &str,
    intro: &str,
    body: &gtk::Box,
    ok: &str,
    destructive: bool,
    on_ok: impl Fn() -> Option<String> + 'static,
) {
    let (dialog, card) = widgets::dialog(title, 460);
    if !intro.is_empty() {
        let l = gtk::Label::new(None);
        l.set_markup(intro);
        l.set_wrap(true);
        l.set_xalign(0.0);
        l.set_max_width_chars(52);
        l.add_css_class("dim");
        card.append(&l);
    }
    card.append(body);
    let error = widgets::label("", "error-text");
    error.set_wrap(true);
    error.set_visible(false);
    card.append(&error);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let go = gtk::Button::with_label(ok);
    go.add_css_class(if destructive { "destructive-action" } else { "suggested-action" });
    buttons.append(&cancel);
    buttons.append(&go);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let d = dialog.clone();
    go.connect_clicked(move |_| match on_ok() {
        None => d.close(),
        Some(e) => {
            error.set_text(&e);
            error.set_visible(true);
        }
    });
    dialog.present();
}

/// The last step before erasing: what goes, warnings, and the name to type.
fn confirm_erase(what: &str, word: &str, warnings: Vec<String>, ok: &str, on_ok: impl Fn() + 'static) {
    let body = widgets::vbox(10);
    for w in &warnings {
        body.append(&widgets::banner(w, true));
    }
    let l = gtk::Label::new(None);
    l.set_markup(&format!("Type <b>{}</b> to confirm.", glib::markup_escape_text(word)));
    l.set_xalign(0.0);
    body.append(&l);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some(word));
    body.append(&entry);
    let word = word.to_string();
    let e2 = entry.clone();
    form(
        "This can't be undone",
        &format!("{}. Everything on it is lost.", glib::markup_escape_text(what)),
        &body,
        ok,
        true,
        move || {
            if e2.text().trim() != word {
                return Some(format!("Type {word} exactly, or Cancel."));
            }
            on_ok();
            None
        },
    );
    entry.grab_focus();
}

/// Red warnings for changing a partition: the system, another OS.
fn warnings_for(p: &Part, d: &Drive) -> Vec<String> {
    let mut w = Vec::new();
    if p.system {
        w.push("This partition holds the system you're running. Changing it can leave the computer unable to start.".into());
    } else if d.system() {
        w.push("This drive holds the system you're running. Make sure this is the partition you mean.".into());
    }
    if p.other_os {
        w.push("This partition belongs to Windows. Changing it can stop Windows from starting, or lose its files.".into());
    }
    w
}

/// Offer to install the package with `program` in a terminal.
pub(super) fn offer_install(program: &str) {
    match udisks::package_for(program) {
        Some(pkg) => {
            let pkg = pkg.to_string();
            window::toast_action(&format!("This needs {program}, from the {pkg} package."), "Install", move || {
                super::accounts::terminal(&format!("sudo pacman -S --needed -- {pkg}"))
            });
        }
        None => window::toast(&format!("This needs {program}, which isn't installed.")),
    }
}

/// Filesystems to make, with what each is for.
const KINDS: &[(&str, &str)] = &[
    ("ext4", "ext4: Linux, reliable and fast"),
    ("btrfs", "btrfs: Linux, with snapshots and compression"),
    ("exfat", "exFAT: every computer, files of any size"),
    ("vfat", "FAT32: every computer and device, files up to 4 GB"),
    ("ntfs", "NTFS: Windows"),
];

/// Fields for a new filesystem; returns the body and a reader.
fn make_fields(default_fs: &str) -> (gtk::Box, Rc<dyn Fn() -> std::result::Result<Make, String>>) {
    let body = widgets::vbox(10);
    let options: Vec<(String, String)> = KINDS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let kind = widgets::dropdown(&options, default_fs);
    body.append(&kind);
    let label = gtk::Entry::new();
    label.set_placeholder_text(Some("Name (optional), e.g. Backup"));
    body.append(&label);
    let (enc_row, enc) =
        widgets::switch_row("Encrypt with a passphrase", "It can't be read without the passphrase (LUKS2).", false, |_| {});
    body.append(&enc_row);
    let pass = gtk::PasswordEntry::new();
    pass.set_placeholder_text(Some("Passphrase"));
    pass.set_show_peek_icon(true);
    let pass2 = gtk::PasswordEntry::new();
    pass2.set_placeholder_text(Some("Passphrase again"));
    pass.set_visible(false);
    pass2.set_visible(false);
    body.append(&pass);
    body.append(&pass2);
    let (p1, p2) = (pass.clone(), pass2.clone());
    enc.connect_active_notify(move |s| {
        p1.set_visible(s.is_active());
        p2.set_visible(s.is_active());
    });
    let (erase_row, erase) = widgets::switch_row(
        "Overwrite the old data",
        "Slow: writes over every byte, so nothing can be recovered.",
        false,
        |_| {},
    );
    body.append(&erase_row);
    let read: Rc<dyn Fn() -> std::result::Result<Make, String>> = Rc::new(move || {
        let fs = KINDS.get(kind.selected() as usize).map(|(a, _)| a.to_string()).unwrap_or_else(|| "ext4".into());
        let label_text = label.text().trim().to_string();
        let max = if fs == "vfat" { 11 } else { 32 };
        if label_text.chars().count() > max {
            return Err(format!("A {} name can be at most {max} characters.", udisks::fs_name(&fs)));
        }
        let (ok, missing) = udisks::can_format(&fs);
        if !ok {
            offer_install(&missing);
            return Err(format!("Making {} needs {missing}, which isn't installed.", udisks::fs_name(&fs)));
        }
        let passphrase = if enc.is_active() {
            let (a, b) = (pass.text().to_string(), pass2.text().to_string());
            if a.len() < 8 {
                return Err("Use a passphrase of at least 8 characters.".into());
            }
            if a != b {
                return Err("The two passphrases are different.".into());
            }
            Some(a)
        } else {
            None
        };
        Ok(Make { fs, label: label_text, passphrase, erase: erase.is_active() })
    });
    (body, read)
}

fn format_dialog(d: &Drive, p: &Part) {
    let (body, read) = make_fields(if p.fs.is_empty() || p.fs == "crypto_LUKS" { "ext4" } else { &p.fs });
    let (d, p) = (d.clone(), p.clone());
    form(
        &format!("Format {}", p.title()),
        &format!("Make a new, empty filesystem on <b>{}</b> ({}).", p.short(), bytes_text(p.size)),
        &body,
        "Continue",
        false,
        move || {
            let m = match read() {
                Ok(m) => m,
                Err(e) => return Some(e),
            };
            let (d2, p2) = (d.clone(), p.clone());
            confirm_erase(
                &format!("Formatting erases {} ({}, {})", p.title(), bytes_text(p.size), p.kind()),
                &p.short(),
                warnings_for(&p, &d),
                "Format",
                move || {
                    let (block, m) = (p2.path.clone(), m.clone());
                    run(&d2.block, &format!("Formatting {}", p2.short()), "Formatted.", async move {
                        udisks::format(&block, &m).await
                    });
                },
            );
            None
        },
    );
}

fn create_dialog(d: &Drive, offset: u64, size: u64) {
    let (body, read) = make_fields("ext4");
    let max_mib = (size >> 20).max(1) as f64;
    let (scale_box, slider) = widgets::slider(1.0, max_mib, 1.0, max_mib, 0, " MiB");
    let size_l = widgets::label(&bytes_text(size), "dim");
    let sl = size_l.clone();
    slider.scale.connect_value_changed(move |s| sl.set_text(&bytes_text((s.value() as u64) << 20)));
    let size_row = widgets::vbox(4);
    size_row.append(&widgets::label("Size", ""));
    size_row.append(&scale_box);
    size_row.append(&size_l);
    body.prepend(&size_row);
    let d = d.clone();
    let scale = slider.scale.clone();
    form(
        "New partition",
        &format!("In the free space on <b>{}</b>.", glib::markup_escape_text(&d.title())),
        &body,
        "Create",
        false,
        move || {
            let m = match read() {
                Ok(m) => m,
                Err(e) => return Some(e),
            };
            let bytes = udisks::align((scale.value() as u64) << 20).min(size);
            let (block, name) = (d.block.clone(), m.label.clone());
            run(&d.block, "Making a new partition", "Partition made.", async move {
                udisks::create(&block, offset, bytes, &name, &m).await.map(|_| ())
            });
            None
        },
    );
}

fn resize_dialog(d: &Drive, i: usize) {
    let p = d.parts[i].clone();
    let fs = p.content().fs.clone();
    let mounted = !p.content().mounts.is_empty();
    let (modes, missing) = if fs.is_empty() { (Some(udisks::modes(30)), String::new()) } else { udisks::can_resize(&fs) };
    let Some(modes) = modes else {
        if !missing.is_empty() {
            offer_install(&missing);
        } else {
            window::toast(&format!("{} can't be resized.", udisks::fs_name(&fs)));
        }
        return;
    };
    let used = p.content().mounts.first().and_then(|m| udisks::used(m)).map(|(u, _)| u);
    let (can_shrink, can_grow) =
        if mounted { (modes.online_shrink, modes.online_grow) } else { (modes.offline_shrink, modes.offline_grow) };
    let min = if can_shrink { udisks::smallest(&p, used) } else { p.size };
    let max = if can_grow { p.size + d.free_after(i) } else { p.size };
    if max <= min + (16 << 20) {
        let why = if !can_shrink && !can_grow {
            if mounted {
                format!("{} can't be resized while it's mounted. Unmount it first.", udisks::fs_name(&fs))
            } else {
                format!("{} has to be mounted to be resized. Mount it first.", udisks::fs_name(&fs))
            }
        } else if !can_shrink && d.free_after(i) == 0 {
            "There's no free space after it to grow into, and it can't shrink while it's mounted.".into()
        } else if used.is_none() && d.free_after(i) == 0 {
            "There's no free space after it. Mount it so Settings can see how much it can shrink.".into()
        } else {
            "There's no room to change its size.".into()
        };
        window::toast(&why);
        return;
    }
    let body = widgets::vbox(10);
    for w in warnings_for(&p, d) {
        body.append(&widgets::banner(&w, true));
    }
    let (scale_box, slider) = widgets::slider((min >> 20) as f64, (max >> 20) as f64, 1.0, (p.size >> 20) as f64, 0, " MiB");
    body.append(&scale_box);
    let readout = widgets::label("", "dim");
    readout.set_xalign(0.0);
    body.append(&readout);
    let total = p.size;
    let r2 = readout.clone();
    let show = move |v: f64| {
        let new = (v as u64) << 20;
        let change = if new >= total {
            format!("{} bigger", bytes_text(new - total))
        } else {
            format!("{} smaller", bytes_text(total - new))
        };
        r2.set_text(&format!("{} · {}", bytes_text(new), change));
    };
    show((p.size >> 20) as f64);
    slider.scale.connect_value_changed(move |s| show(s.value()));
    let mut intro = format!(
        "From {} up to {}{}.",
        bytes_text(min),
        bytes_text(max),
        if used.is_some() { format!(" ({} is in use)", bytes_text(used.unwrap_or(0))) } else { String::new() }
    );
    intro.push_str(" Back up anything important first: a power cut while resizing can lose data.");
    let (d, scale) = (d.clone(), slider.scale.clone());
    form(&format!("Resize {}", p.title()), &intro, &body, "Resize", false, move || {
        let size = udisks::align((scale.value() as u64) << 20);
        if size.abs_diff(p.size) < (1 << 20) {
            return Some("That's the size it is now.".into());
        }
        let p2 = p.clone();
        run(&d.block, &format!("Resizing {}", p.short()), "Resized.", async move { udisks::resize(&p2, size).await });
        None
    });
}

fn label_dialog(d: &Drive, p: &Part) {
    let target = p.content().clone();
    let body = widgets::vbox(8);
    let e = gtk::Entry::new();
    e.set_text(&target.label);
    e.set_placeholder_text(Some("Name"));
    body.append(&e);
    let d = d.clone();
    let e2 = e.clone();
    form("Rename", "The name shown for it in Files and here.", &body, "Rename", false, move || {
        let name = e2.text().trim().to_string();
        let max = if target.fs == "vfat" { 11 } else { 32 };
        if name.chars().count() > max {
            return Some(format!("At most {max} characters."));
        }
        let path = target.path.clone();
        run(&d.block, "Renaming", "Renamed.", async move { udisks::set_label(&path, &name).await });
        None
    });
    e.grab_focus();
}

fn unlock_dialog(d: &Drive, p: &Part) {
    let body = widgets::vbox(8);
    let e = gtk::PasswordEntry::new();
    e.set_placeholder_text(Some("Passphrase"));
    e.set_show_peek_icon(true);
    body.append(&e);
    let (d, path) = (d.clone(), p.path.clone());
    form("Unlock", "Its passphrase opens it until you lock it or restart.", &body, "Unlock", false, move || {
        let (pw, path) = (e.text().to_string(), path.clone());
        run(&d.block, "Unlocking", "Unlocked.", async move { udisks::unlock(&path, &pw).await });
        None
    });
}

fn passphrase_dialog(d: &Drive, p: &Part) {
    let body = widgets::vbox(8);
    let old = gtk::PasswordEntry::new();
    old.set_placeholder_text(Some("Current passphrase"));
    let new = gtk::PasswordEntry::new();
    new.set_placeholder_text(Some("New passphrase"));
    let again = gtk::PasswordEntry::new();
    again.set_placeholder_text(Some("New passphrase again"));
    for e in [&old, &new, &again] {
        e.set_show_peek_icon(true);
        body.append(e);
    }
    let (d, path) = (d.clone(), p.path.clone());
    form("Change passphrase", "", &body, "Change", false, move || {
        let (o, n, a) = (old.text().to_string(), new.text().to_string(), again.text().to_string());
        if n.len() < 8 {
            return Some("Use a passphrase of at least 8 characters.".into());
        }
        if n != a {
            return Some("The two new passphrases are different.".into());
        }
        let path = path.clone();
        run(
            &d.block,
            "Changing the passphrase",
            "Passphrase changed.",
            async move { udisks::change_passphrase(&path, &o, &n).await },
        );
        None
    });
}

fn table_dialog(d: &Drive) {
    let body = widgets::vbox(8);
    let kind = widgets::dropdown(
        &[
            ("gpt".to_string(), "GPT: modern computers, any size".to_string()),
            ("dos".to_string(), "MBR: old computers and devices, up to 2 TB".to_string()),
        ],
        "gpt",
    );
    body.append(&kind);
    let d = d.clone();
    form(
        &format!("New partition table on {}", d.title()),
        "A new, empty table: every partition on the drive goes.",
        &body,
        "Continue",
        false,
        move || {
            if d.in_use() {
                return Some("Something on this drive is in use. Unmount or lock it first.".into());
            }
            let table = if kind.selected() == 1 { "dos" } else { "gpt" };
            let mut warn = Vec::new();
            if d.system() {
                warn.push("This drive holds the system you're running.".to_string());
            }
            if d.other_os() {
                warn.push("This drive holds Windows. Windows and its files will be gone.".to_string());
            }
            let word = d.device.trim_start_matches("/dev/").to_string();
            let d2 = d.clone();
            confirm_erase(
                &format!("A new table erases all of {} ({})", d.title(), bytes_text(d.size)),
                &word,
                warn,
                "Erase the drive",
                move || {
                    let block = d2.block.clone();
                    run(&d2.block, "Making a new partition table", "New partition table made.", async move {
                        udisks::new_table(&block, table).await
                    });
                },
            );
            None
        },
    );
}

fn health_dialog(d: &Drive) {
    let (dialog, card) = widgets::dialog(&format!("{} health", d.title()), 440);
    let list = widgets::vbox(0);
    list.add_css_class("settings-card");
    let add = |list: &gtk::Box, k: &str, v: &str| {
        let (r, _) = widgets::info_row(k, v);
        list.append(&r);
    };
    add(&list, "Status", if d.health.ok() { "Healthy" } else { "Needs attention" });
    for w in &d.health.warnings {
        add(&list, "Warning", &warning_text(w));
    }
    if let Some(t) = d.health.temperature {
        add(&list, "Temperature", &format!("{:.0} {}", crate::units::from_celsius(t), crate::units::symbol()));
    }
    if let Some(h) = d.health.power_on_hours {
        add(&list, "Powered on", &format!("{h} hours ({:.0} days)", h as f64 / 24.0));
    }
    card.append(&list);
    if d.nvme {
        let (path, list2) = (d.path.clone(), list.clone());
        glib::spawn_future_local(async move {
            let Ok(a) = udisks::nvme_details(&path).await else { return };
            let num = |k: &str| a.get(k).and_then(|v| v.get::<u8>().map(u64::from).or_else(|| v.get::<u64>()));
            if let Some(used) = num("percent_used") {
                let (r, _) = widgets::info_row("Wear", &format!("{used}% of its rated life used"));
                list2.append(&r);
            }
            if let Some(spare) = num("avail_spare") {
                let (r, _) = widgets::info_row("Spare blocks", &format!("{spare}% left"));
                list2.append(&r);
            }
            if let Some(w) = num("total_data_written") {
                let (r, _) = widgets::info_row("Written", &bytes_text(w));
                list2.append(&r);
            }
            if let Some(e) = num("media_errors") {
                let (r, _) = widgets::info_row("Media errors", &e.to_string());
                list2.append(&r);
            }
            if let Some(u) = num("unsafe_shutdowns") {
                let (r, _) = widgets::info_row("Unsafe shutdowns", &u.to_string());
                list2.append(&r);
            }
        });
    }
    let close = gtk::Button::with_label("Close");
    close.set_halign(gtk::Align::End);
    let d2 = dialog.clone();
    close.connect_clicked(move |_| d2.close());
    card.append(&close);
    dialog.present();
}

fn warning_text(w: &str) -> String {
    match w {
        "spare" => "Running out of spare blocks".into(),
        "temperature" => "Too hot".into(),
        "degraded" => "Reliability is degraded".into(),
        "readonly" => "Gone read-only to protect your data".into(),
        "volatile_mem" => "Its backup memory failed".into(),
        "pmr_readonly" => "Part of it is read-only".into(),
        "failing" => "SMART says the drive is failing: back up now".into(),
        other => other.to_string(),
    }
}

// ---------- The page ----------

fn drive_icon(d: &Drive) -> &'static str {
    if d.bus == "usb" || d.removable {
        "drive-removable-media-symbolic"
    } else if d.rotational {
        "drive-harddisk-symbolic"
    } else {
        "drive-harddisk-solidstate-symbolic"
    }
}

fn bus_name(d: &Drive) -> &str {
    match d.bus.as_str() {
        "nvme" => "NVMe",
        "usb" => "USB",
        "sata" => "SATA",
        "sdio" => "SD card",
        "loop" => "Disk image",
        other => other,
    }
}

fn button(label: &str, icon: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    let c = widgets::hbox(6);
    c.append(&gtk::Image::from_icon_name(icon));
    c.append(&gtk::Label::new(Some(label)));
    b.set_child(Some(&c));
    b.connect_clicked(move |_| f());
    b
}

fn drive_card(d: &Drive) -> gtk::Box {
    let card = widgets::vbox(12);
    card.add_css_class("drive-card");
    let head = widgets::hbox(12);
    let icon = gtk::Image::from_icon_name(drive_icon(d));
    icon.set_pixel_size(28);
    icon.add_css_class("drive-icon");
    head.append(&icon);
    let text = widgets::vbox(2);
    text.set_hexpand(true);
    let title = widgets::hbox(8);
    let t = widgets::label(&d.title(), "drive-title");
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.append(&t);
    if d.system() {
        title.append(&widgets::tag("System drive"));
    }
    if d.other_os() {
        let tag = widgets::tag("Windows");
        tag.add_css_class("warn-tag");
        title.append(&tag);
    }
    if d.removable {
        title.append(&widgets::tag("Removable"));
    }
    text.append(&title);
    let table = match d.table.as_str() {
        "gpt" => "GPT",
        "dos" => "MBR",
        _ => "no partition table",
    };
    let sub = widgets::label(&format!("{} · {} · {} · {}", bytes_text(d.size), bus_name(d), table, d.device), "dim");
    sub.set_xalign(0.0);
    text.append(&sub);
    head.append(&text);
    if d.health.supported {
        let (label, class) = if d.health.ok() {
            match d.health.temperature {
                Some(t) => (format!("Healthy · {:.0}{}", crate::units::from_celsius(t), crate::units::symbol()), "health-good"),
                None => ("Healthy".to_string(), "health-good"),
            }
        } else {
            ("Needs attention".to_string(), "health-bad")
        };
        let b = gtk::Button::with_label(&label);
        b.add_css_class("flat");
        b.add_css_class(class);
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some("Temperature, wear and errors"));
        let d2 = d.clone();
        b.connect_clicked(move |_| health_dialog(&d2));
        head.append(&b);
    }
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.add_css_class("flat");
    menu.set_valign(gtk::Align::Center);
    menu.set_tooltip_text(Some("Drive"));
    let pop = gtk::Popover::new();
    let items = widgets::vbox(2);
    let d2 = d.clone();
    let p2 = pop.clone();
    let new_table = button("New partition table…", "edit-clear-all-symbolic", move || {
        p2.popdown();
        table_dialog(&d2);
    });
    new_table.add_css_class("flat");
    items.append(&new_table);
    if d.ejectable || d.can_power_off {
        let d2 = d.clone();
        let p2 = pop.clone();
        let eject = button(if d.can_power_off { "Safely remove" } else { "Eject" }, "media-eject-symbolic", move || {
            p2.popdown();
            let (path, off) = (d2.path.clone(), d2.can_power_off);
            run(&d2.block, "Removing", "Safe to unplug.", async move {
                if off { udisks::power_off(&path).await } else { udisks::eject(&path).await }
            });
        });
        eject.add_css_class("flat");
        items.append(&eject);
    }
    pop.set_child(Some(&items));
    menu.set_popover(Some(&pop));
    head.append(&menu);
    card.append(&head);

    if let Some((_, what)) = BUSY.with(|b| b.borrow().clone()).filter(|(blk, _)| *blk == d.block) {
        let row = widgets::vbox(4);
        row.append(&widgets::label(&format!("{what}…"), ""));
        let bar = gtk::ProgressBar::new();
        bar.set_pulse_step(0.15);
        row.append(&bar);
        PROGRESS.with(|p| *p.borrow_mut() = Some(bar));
        card.append(&row);
    }

    card.append(&map(d));
    // What's picked: the first partition if nothing is yet.
    let chosen = picked(&d.block).filter(|p| match p {
        Pick::Part(path) => d.parts.iter().any(|x| x.path == *path),
        Pick::Free(off) => d.segments().iter().any(|s| matches!(s, Segment::Free { offset, .. } if offset == off)),
    });
    let chosen = chosen.or_else(|| {
        d.segments().first().map(|s| match s {
            Segment::Part(i) => Pick::Part(d.parts[*i].path.clone()),
            Segment::Free { offset, .. } => Pick::Free(*offset),
        })
    });
    match chosen {
        Some(Pick::Part(path)) => {
            if let Some(i) = d.parts.iter().position(|p| p.path == path) {
                card.append(&part_panel(d, i));
            }
        }
        Some(Pick::Free(off)) => {
            if let Some(Segment::Free { offset, size }) =
                d.segments().into_iter().find(|s| matches!(s, Segment::Free { offset, .. } if *offset == off))
            {
                card.append(&free_panel(d, offset, size));
            }
        }
        None => {}
    }
    card
}

fn info(grid: &gtk::Grid, row: i32, k: &str, v: &str) {
    let a = widgets::label(k, "dim");
    a.set_xalign(0.0);
    let b = widgets::label(v, "");
    b.set_xalign(0.0);
    b.set_selectable(true);
    b.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    b.set_hexpand(true);
    grid.attach(&a, 0, row, 1, 1);
    grid.attach(&b, 1, row, 1, 1);
}

fn part_panel(d: &Drive, i: usize) -> gtk::Box {
    let p = &d.parts[i];
    let content = p.content();
    let panel = widgets::vbox(12);
    panel.add_css_class("part-panel");
    let head = widgets::hbox(8);
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(12);
    swatch.set_content_height(12);
    let c = fs_colour(p);
    swatch.set_draw_func(move |_, cr, w, h| {
        rounded(cr, 0.0, 0.0, w as f64, h as f64, 3.0);
        set(cr, c, 1.0);
        let _ = cr.fill();
    });
    swatch.set_valign(gtk::Align::Center);
    head.append(&swatch);
    head.append(&widgets::label(&p.title(), "part-title"));
    let kind = widgets::label(&format!("{} · {}", p.kind(), bytes_text(p.size)), "dim");
    head.append(&kind);
    panel.append(&head);

    if let Some(m) = content.mounts.first()
        && let Some((used, size)) = udisks::used(m)
        && size > 0
    {
        let bar = gtk::LevelBar::for_interval(0.0, 1.0);
        bar.set_value(used as f64 / size as f64);
        bar.add_css_class("part-usage");
        panel.append(&bar);
        let l = widgets::label(
            &format!("{} used of {} · {} free", bytes_text(used), bytes_text(size), bytes_text(size.saturating_sub(used))),
            "dim",
        );
        l.set_xalign(0.0);
        panel.append(&l);
    }

    let grid = gtk::Grid::new();
    grid.set_column_spacing(16);
    grid.set_row_spacing(4);
    let mut row = 0;
    info(&grid, row, "Device", &p.device);
    row += 1;
    if !content.mounts.is_empty() {
        info(&grid, row, "Mounted at", &content.mounts.join(", "));
        row += 1;
    }
    if !content.uuid.is_empty() {
        info(&grid, row, "UUID", &content.uuid);
        row += 1;
    }
    if p.partition && !p.name.is_empty() && p.name != p.title() {
        info(&grid, row, "Partition name", &p.name);
        row += 1;
    }
    if p.fstab || content.fstab {
        info(&grid, row, "At startup", "Mounted from /etc/fstab");
    }
    panel.append(&grid);

    for w in warnings_for(p, d) {
        panel.append(&widgets::banner(&w, true));
    }
    if p.in_use() && !p.system {
        panel.append(&widgets::banner("In use: unmount or lock it to format, delete or shrink it.", false));
    }

    let actions = gtk::FlowBox::new();
    actions.set_selection_mode(gtk::SelectionMode::None);
    actions.set_max_children_per_line(6);
    actions.set_column_spacing(6);
    actions.set_row_spacing(6);
    actions.add_css_class("part-actions");
    let add = |b: gtk::Button| actions.append(&b);
    let (dd, pp) = (d.clone(), p.clone());

    // Open, mount and unmount.
    if content.filesystem && !p.system {
        if let Some(m) = content.mounts.first().cloned() {
            add(button("Open", "folder-open-symbolic", move || cmd::spawn(&["xdg-open", &m])));
            let (d2, path) = (dd.clone(), content.path.clone());
            add(button("Unmount", "media-eject-symbolic", move || {
                let path = path.clone();
                run(&d2.block, "Unmounting", "Unmounted.", async move { udisks::unmount(&path).await });
            }));
        } else {
            let (d2, path) = (dd.clone(), content.path.clone());
            add(button("Mount", "folder-symbolic", move || {
                let path = path.clone();
                run(&d2.block, "Mounting", "Mounted.", async move { udisks::mount(&path).await.map(|_| ()) });
            }));
        }
    }
    if p.encrypted && !p.system {
        if p.unlocked.is_some() {
            if content.mounts.is_empty() {
                let (d2, path) = (dd.clone(), p.path.clone());
                add(button("Lock", "changes-prevent-symbolic", move || {
                    let path = path.clone();
                    run(&d2.block, "Locking", "Locked.", async move { udisks::lock(&path).await });
                }));
            }
        } else {
            let (d2, p2) = (dd.clone(), pp.clone());
            add(button("Unlock…", "changes-allow-symbolic", move || unlock_dialog(&d2, &p2)));
        }
    }
    if content.filesystem && ["ext4", "ext3", "btrfs", "vfat", "exfat", "ntfs", "xfs"].contains(&content.fs.as_str()) && !p.system
    {
        let (d2, p2) = (dd.clone(), pp.clone());
        add(button("Rename…", "document-edit-symbolic", move || label_dialog(&d2, &p2)));
    }
    if p.partition && !p.swap_active {
        let d2 = dd.clone();
        add(button("Resize…", "zoom-fit-best-symbolic", move || resize_dialog(&d2, i)));
    }
    if content.filesystem && content.mounts.is_empty() && udisks::can_check(&content.fs).0 {
        let (d2, path) = (dd.clone(), content.path.clone());
        add(button("Check", "emblem-ok-symbolic", move || {
            let path = path.clone();
            let block = d2.block.clone();
            run(&block, "Checking", "Check finished.", async move {
                let fine = udisks::check(&path, false).await?;
                if fine {
                    window::toast("No problems found.");
                    Ok(())
                } else {
                    let path2 = path.clone();
                    window::toast_action("Problems found.", "Repair", move || {
                        let path2 = path2.clone();
                        glib::spawn_future_local(async move {
                            match udisks::check(&path2, true).await {
                                Ok(true) => window::toast("Repaired."),
                                Ok(false) => window::toast("Some problems couldn't be repaired."),
                                Err(e) => window::toast(&format!("{e}")),
                            }
                        });
                    });
                    Ok(())
                }
            });
        }));
    }
    if !p.in_use() {
        let (d2, p2) = (dd.clone(), pp.clone());
        add(button("Format…", "edit-clear-symbolic", move || format_dialog(&d2, &p2)));
    }
    // Changing the system's own passphrase is fine: the old one stops working.
    if p.encrypted {
        let (d2, p2) = (dd.clone(), pp.clone());
        add(button("Passphrase…", "dialog-password-symbolic", move || passphrase_dialog(&d2, &p2)));
    }
    if content.filesystem && !p.system && !content.fstab {
        let p2 = pp.clone();
        add(button("Mount at startup…", "emblem-synchronizing-symbolic", move || {
            super::mounts::startup_dialog(Some(p2.content().clone()))
        }));
    }
    if p.partition && !p.in_use() {
        let (d2, p2) = (dd.clone(), pp.clone());
        let del = button("Delete…", "user-trash-symbolic", move || {
            let (d3, p3) = (d2.clone(), p2.clone());
            confirm_erase(
                &format!("Deleting removes {} ({}, {})", p2.title(), bytes_text(p2.size), p2.kind()),
                &p2.short(),
                warnings_for(&p2, &d2),
                "Delete",
                move || {
                    let path = p3.path.clone();
                    pick(&d3.block, Pick::Free(p3.offset));
                    run(&d3.block, &format!("Deleting {}", p3.short()), "Deleted.", async move { udisks::delete(&path).await });
                },
            );
        });
        del.add_css_class("destructive-action");
        add(del);
    }
    panel.append(&actions);
    panel
}

fn free_panel(d: &Drive, offset: u64, size: u64) -> gtk::Box {
    let panel = widgets::vbox(10);
    panel.add_css_class("part-panel");
    let head = widgets::hbox(8);
    head.append(&widgets::label("Free space", "part-title"));
    head.append(&widgets::label(&bytes_text(size), "dim"));
    panel.append(&head);
    if d.table.is_empty() {
        panel.append(&widgets::label("This drive has no partition table yet. Make one to add partitions.", "dim"));
        let d2 = d.clone();
        panel.append(&button("Make a partition table…", "list-add-symbolic", move || table_dialog(&d2)));
    } else {
        let l = widgets::label("Not part of any partition. Make a new one here.", "dim");
        l.set_xalign(0.0);
        panel.append(&l);
        let d2 = d.clone();
        let b = button("New partition…", "list-add-symbolic", move || create_dialog(&d2, offset, size));
        b.add_css_class("suggested-action");
        b.set_halign(gtk::Align::Start);
        panel.append(&b);
    }
    panel
}

pub fn build(page: &Page) {
    let drives = match udisks::load() {
        Ok(d) => d,
        Err(e) => {
            page.banner(&format!("Couldn't reach UDisks, which Settings uses for disks: {e}"), true);
            return;
        }
    };
    udisks::watch(&page.root, || {
        // Not mid-change: a running one rebuilds when it's done.
        if BUSY.with(|b| b.borrow().is_none()) {
            rebuild();
        }
    });
    PROGRESS.with(|p| *p.borrow_mut() = None);
    let g = page.plain_group("Drives");
    g.note("Pick a part of a drive's map to see what's on it and what you can do. Changes ask for your password.");
    widgets::keywords(
        "disk drive partition format resize shrink grow delete label rename mount unmount encrypt luks usb ssd nvme",
    );
    for d in &drives {
        g.add(&drive_card(d));
    }
    super::cleanup::build(page);
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn small_partitions_stay_visible() {
        let l = layout(&[100, 1, 899], 400.0, 3.0, 30.0);
        assert_eq!(l.len(), 3);
        assert!(l[1].1 >= 30.0 - 1e-6);
        let end = l[2].0 + l[2].1;
        assert!((end - 400.0).abs() < 1.0, "fills the width: {end}");
        assert!(l[0].0 == 0.0 && l[1].0 > l[0].1);
    }
}
