use crate::backend::{appearance, chroma};
use crate::widgets::{self, Page};
use crate::{cmd, paths, prefs, theme};
use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib};
use std::path::PathBuf;

/// Omarchy theme directory for a display name ("Catppuccin Latte" -> catppuccin-latte).
fn theme_dir(name: &str) -> Option<PathBuf> {
    let slug = name.to_lowercase().replace(' ', "-");
    [paths::omarchy_config().join("themes").join(&slug), PathBuf::from("/usr/share/omarchy/themes").join(&slug)]
        .into_iter()
        .find(|p| p.exists())
}

fn load_thumbnail(picture: &gtk::Picture, path: PathBuf) {
    let picture = picture.clone();
    cmd::background(
        move || {
            let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 360, 202, true).ok()?;
            Some((pb.read_pixel_bytes(), pb.width(), pb.height(), pb.rowstride(), pb.has_alpha()))
        },
        move |result| {
            if let Some((bytes, w, h, stride, alpha)) = result {
                let format = if alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
                let texture = gdk::MemoryTexture::new(w, h, format, &bytes, stride as usize);
                picture.set_paintable(Some(&texture));
            }
        },
    );
}

pub fn build(page: &Page) {
    // ----- Omarchy theme -----
    let g = page.group("Omarchy theme");
    g.note("Applies to the whole desktop: terminal, bar, editor, lock screen and wallpaper.");
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(4);
    flow.set_min_children_per_line(2);
    flow.set_row_spacing(10);
    flow.set_column_spacing(10);
    flow.set_homogeneous(true);
    let current = cmd::output(&["omarchy-theme-current"]).unwrap_or_default();
    let names: Vec<String> = cmd::output(&["omarchy-theme-list"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let cards: std::rc::Rc<std::cell::RefCell<Vec<(String, gtk::Button)>>> = Default::default();
    for name in &names {
        let card = gtk::Button::new();
        card.add_css_class("theme-card");
        let v = widgets::vbox(0);
        let pic = gtk::Picture::new();
        pic.set_content_fit(gtk::ContentFit::Cover);
        pic.set_size_request(160, 90);
        pic.set_can_shrink(true);
        if let Some(dir) = theme_dir(name) {
            let preview = dir.join("preview.png");
            if preview.exists() {
                load_thumbnail(&pic, preview);
            }
        }
        v.append(&pic);
        let l = widgets::label(name, "theme-card-name");
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        v.append(&l);
        card.set_child(Some(&v));
        if *name == current {
            card.add_css_class("selected");
        }
        let n = name.clone();
        let cards2 = cards.clone();
        card.connect_clicked(move |_| {
            for (other, b) in cards2.borrow().iter() {
                if *other == n {
                    b.add_css_class("selected");
                } else {
                    b.remove_css_class("selected");
                }
            }
            let n2 = n.clone();
            cmd::run_async(&["omarchy-theme-set", &n], move |r| match r {
                Ok(_) => crate::window::toast(&format!("Theme set to {n2}")),
                Err(e) => crate::window::toast(&format!("Couldn't set theme: {e}")),
            });
        });
        cards.borrow_mut().push((name.clone(), card.clone()));
        flow.insert(&card, -1);
    }
    let theme_row = widgets::stacked_row("Theme", "", flow.upcast_ref());
    widgets::keywords(&names.join(" "));
    g.add(&theme_row);
    icons_row(&g);

    apps_group(page);

    // ----- Wallpaper -----
    let g = page.group("Wallpaper");
    let buttons = widgets::hbox(8);
    buttons.append(&widgets::command_button("Next wallpaper", &["omarchy-theme-bg-next"]));
    buttons.append(&widgets::command_button("Choose…", &["omarchy-menu", "toggle", "background"]));
    g.add(&widgets::row("Background", "Cycle through the current theme's wallpapers or pick one.", Some(buttons.upcast_ref())));

    // ----- Font -----
    let g = page.group("Font");
    let fonts: Vec<(String, String)> = cmd::output(&["omarchy-font-list"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|f| (f.to_string(), f.to_string()))
        .collect();
    let current_font = cmd::output(&["omarchy-font-current"]).unwrap_or_default();
    g.add(&interface_font_row());
    let (r, _) = widgets::choice_row(
        "Monospace font",
        "Used by terminals, the bar and the lock screen.",
        fonts,
        &current_font,
        |font| {
            cmd::run_async(&["omarchy-font-set", &font], move |r| {
                if let Err(e) = r {
                    crate::window::toast(&format!("Couldn't set font: {e}"));
                }
            });
        },
    );
    g.add(&r);

    // ----- This window -----
    let g = page.group("This window");
    let p = prefs::get();
    let app_themes = theme::all();
    let options: Vec<(String, String)> = app_themes.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
    let (theme_row, theme_dd) = widgets::choice_row(
        "Settings theme",
        "Dracula, Catppuccin, Tokyo Night, One Dark Pro and more. Add your own in <tt>~/.config/settings/themes</tt>.",
        options,
        &p.theme,
        |id| {
            prefs::update(|p| {
                p.theme = id;
                p.mode = prefs::ThemeMode::Theme;
            });
            theme::apply();
        },
    );
    theme_dd.set_sensitive(p.mode == prefs::ThemeMode::Theme || !theme::omarchy_available());

    if theme::omarchy_available() {
        let dd = theme_dd.clone();
        let (r, _) = widgets::switch_row(
            "Follow Omarchy theme",
            "Match the desktop's colours and update live whenever the Omarchy theme changes.",
            p.mode == prefs::ThemeMode::Omarchy,
            move |on| {
                prefs::update(|p| p.mode = if on { prefs::ThemeMode::Omarchy } else { prefs::ThemeMode::Theme });
                dd.set_sensitive(!on);
                theme::apply();
            },
        );
        g.add(&r);
    }
    g.add(&theme_row);

    let swatches = widgets::hbox(4);
    let refresh_swatches = {
        let swatches = swatches.clone();
        move || {
            while let Some(c) = swatches.first_child() {
                swatches.remove(&c);
            }
            let pal = theme::current_palette();
            for c in [&pal.bg, &pal.surface, &pal.muted, &pal.text, &pal.accent, &pal.danger] {
                let s = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                s.add_css_class("swatch");
                let provider = gtk::CssProvider::new();
                provider.load_from_string(&format!("box {{ background: {c}; }}"));
                #[allow(deprecated)]
                s.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
                swatches.append(&s);
            }
        }
    };
    refresh_swatches();
    let last = std::cell::RefCell::new(theme::current_palette());
    glib::timeout_add_seconds_local(1, move || {
        let now = theme::current_palette();
        if *last.borrow() != now {
            *last.borrow_mut() = now;
            refresh_swatches();
        }
        glib::ControlFlow::Continue
    });
    g.add(&widgets::row("Current colours", "", Some(swatches.upcast_ref())));

    let (r, _) = widgets::switch_row("Glow", "Soft accent glow around focused and selected elements.", p.glow, |on| {
        prefs::update(|p| p.glow = on);
        theme::apply();
    });
    g.add(&r);
    let (r, _) =
        widgets::switch_row("Reduce motion", "Turn off transitions and animations in this window.", p.reduce_motion, |on| {
            prefs::update(|p| p.reduce_motion = on);
            theme::apply();
        });
    g.add(&r);
    let unit = if prefs::get().temp_unit == prefs::TempUnit::Fahrenheit { "f" } else { "c" };
    let (r, _) = widgets::choice_row(
        "Temperature unit",
        "How temperatures are shown, such as fan curves and GPU limits.",
        widgets::opts(&[("c", "Celsius (°C)"), ("f", "Fahrenheit (°F)")]),
        unit,
        |v| {
            prefs::update(|p| p.temp_unit = if v == "f" { prefs::TempUnit::Fahrenheit } else { prefs::TempUnit::Celsius });
            crate::window::rebuild_if_built("asus");
        },
    );
    widgets::keywords("fahrenheit celsius degrees units temperature");
    g.add(&r);
    let (r, _) = widgets::choice_row(
        "Open on",
        "The page Settings shows when it starts. A page asked for by name (<tt>--section</tt>) still opens.",
        widgets::opts(&[("home", "Home"), ("last", "The last page shown")]),
        &prefs::get().open_on,
        |v| prefs::update(|p| p.open_on = v),
    );
    widgets::keywords("start startup launch first page home remember last");
    g.add(&r);
}

// ----- Apps follow the theme (hyprchroma) -----

const HYPRCHROMA_URL: &str = "https://github.com/NobleDoodle/hyprchroma";

/// "Synced for <i>Catppuccin</i> · 2 min ago"
fn status_text(st: &chroma::State) -> String {
    if st.theme.is_empty() {
        return "Not synced yet.".into();
    }
    let ago = chroma::ago(&st.last_sync);
    let name = glib::markup_escape_text(&st.theme);
    if ago.is_empty() { format!("Synced for <i>{name}</i>") } else { format!("Synced for <i>{name}</i> · {ago}") }
}

/// The description label of a `widgets::row`.
fn row_desc(row: &gtk::Box) -> Option<gtk::Label> {
    row.first_child()?.last_child()?.downcast::<gtk::Label>().ok()
}

fn apps_group(page: &Page) {
    let g = page.group("Apps follow the theme");
    widgets::keywords("gtk qt kde libadwaita apps windows dialogs popups consistent dark reader hyprchroma omarchroma");
    if !chroma::installed() {
        g.add(&widgets::banner(
            "Install <b>hyprchroma</b> to give GTK, libadwaita, Qt and KDE apps the Omarchy theme's colours, \
             so every window and dialog matches.",
            false,
        ));
        let (r, _) = widgets::button_row("hyprchroma", "Not installed.", "Install instructions…", |_| {
            cmd::spawn(&["xdg-open", HYPRCHROMA_URL]);
        });
        g.add(&r);
        return;
    }
    g.note("Windows and dialogs in other apps use the Omarchy theme's colours, and change with it. Powered by hyprchroma.");

    let st = chroma::state();
    let status_row = widgets::row("Status", &status_text(&st), None);
    let refresh = {
        let status_row = status_row.clone();
        move || {
            if let Some(l) = row_desc(&status_row) {
                l.set_markup(&status_text(&chroma::state()));
            }
        }
    };
    let sync = gtk::Button::with_label("Sync now");
    sync.set_valign(gtk::Align::Center);
    {
        let refresh = refresh.clone();
        sync.connect_clicked(move |b| {
            b.set_sensitive(false);
            let (b, refresh) = (b.clone(), refresh.clone());
            cmd::background(chroma::sync_now, move |r| {
                b.set_sensitive(true);
                refresh();
                match r {
                    Ok(_) => crate::window::toast("Apps re-themed"),
                    Err(e) => crate::window::toast(&format!("Couldn't sync: {e}")),
                }
            });
        });
    }
    status_row.append(&sync);
    g.add(&status_row);

    for (key, on) in &st.frameworks {
        let Some(t) = chroma::target(key) else { continue };
        let refresh = refresh.clone();
        let (r, _) = widgets::switch_row(t.title, t.desc, *on, move |on| {
            let refresh = refresh.clone();
            cmd::background(move || chroma::set_enabled(t, on), move |r| {
                refresh();
                if let Err(e) = r {
                    crate::window::toast(&format!("Couldn't change {}: {e}", t.title));
                }
            });
        });
        if st.status.iter().any(|(k, v)| k == key && v == "not-installed") {
            widgets::tag_row(&r, "App not installed");
        }
        g.add(&r);
    }

    let removed: Vec<&'static chroma::Target> = st.removed.iter().filter_map(|k| chroma::target(k)).collect();
    if !removed.is_empty() {
        let names = removed.iter().map(|t| t.title).collect::<Vec<_>>().join(", ");
        let (r, _) = widgets::button_row("Hidden", &format!("{names}: removed from hyprchroma."), "Restore", move |b| {
            b.set_sensitive(false);
            let removed = removed.clone();
            cmd::background(
                move || removed.iter().try_for_each(|t| chroma::restore_framework(t).map(|_| ())),
                |r| match r {
                    Ok(()) => crate::window::rebuild("theme"),
                    Err(e) => crate::window::toast(&format!("Couldn't restore: {e}")),
                },
            );
        });
        g.add(&r);
    }

    let (r, _) = widgets::switch_row(
        "Automatic sync",
        "Re-theme apps in the background whenever the Omarchy theme or font changes.",
        chroma::service_active(),
        |on| {
            cmd::background(move || chroma::set_service(on), |r| {
                if let Err(e) = r {
                    crate::window::toast(&format!("Couldn't change automatic sync: {e}"));
                }
            });
        },
    );
    g.add(&r);

    // Apps that need a restart; filled in once hyprchroma answers.
    let stale = widgets::vbox(6);
    stale.set_visible(false);
    g.add(&stale);
    {
        let stale = stale.clone();
        cmd::background(chroma::stale_apps, move |apps| {
            for app in &apps {
                stale.append(&widgets::row(app, "Still showing the old theme. Restart it to update.", None));
            }
            stale.set_visible(!apps.is_empty());
        });
    }

    if chroma::plugin_installed() {
        let (r, b) = widgets::button_row(
            "Omarchroma bar widget",
            "Settings covers everything it does. Removing it keeps hyprchroma and automatic sync.",
            "Remove",
            |b| {
                if !b.has_css_class("armed") {
                    b.add_css_class("armed");
                    b.set_label("Click again to remove");
                    return;
                }
                b.set_sensitive(false);
                cmd::run_async(&["omarchy-plugin-remove", chroma::PLUGIN, "--yes"], |r| match r {
                    Ok(_) => {
                        crate::window::toast("Omarchroma removed");
                        crate::window::rebuild("theme");
                    }
                    Err(e) => crate::window::toast(&format!("Couldn't remove Omarchroma: {e}")),
                });
            },
        );
        b.add_css_class("destructive-action");
        g.add(&r);
    }

    let (adv, content) = widgets::disclosure("Advanced", "");
    let (r, b) = widgets::button_row(
        "Restore stock look",
        "Undo hyprchroma's changes to GTK and KDE files and put back the default styling. Sync again to re-theme.",
        "Restore",
        move |b| {
            if !b.has_css_class("armed") {
                b.add_css_class("armed");
                b.set_label("Click again to restore");
                return;
            }
            b.set_sensitive(false);
            let refresh = refresh.clone();
            cmd::background(chroma::restore_stock, move |r| {
                refresh();
                match r {
                    Ok(_) => crate::window::toast("Stock look restored"),
                    Err(e) => crate::window::toast(&format!("Couldn't restore: {e}")),
                }
            });
        },
    );
    b.add_css_class("destructive-action");
    content.append(&r);
    let source = widgets::row("Colours from", "", None);
    content.append(&source);
    cmd::background(chroma::palette_source, move |src| {
        if let Some(t) = source.first_child()
            && let Ok(t) = t.downcast::<gtk::Box>()
        {
            let d = widgets::label(if src.is_empty() { "Unknown" } else { &src }, "settings-option-description");
            t.append(&d);
        }
    });
    g.add(&adv);
}

// ----- Icons and interface font -----

const FOLLOW: &str = "";

fn icons_row(g: &widgets::Group) {
    let cfg = appearance::load();
    let follow_label = match appearance::omarchy_icon_theme() {
        Some(t) => format!("Follow the theme ({t})"),
        None => "Follow the theme".into(),
    };
    let mut options = vec![(FOLLOW.to_string(), follow_label)];
    options.extend(appearance::icon_themes().into_iter().map(|t| (t.clone(), t)));
    let current = cfg.icon_theme.clone().unwrap_or_default();
    let (r, _) = widgets::choice_row(
        "Icons",
        "Icon theme for apps and file choosers. Omarchy themes pick one; choose here to keep your own.",
        options,
        &current,
        |id| {
            let c = appearance::Config { icon_theme: (id != FOLLOW).then_some(id) };
            cmd::background(
                move || {
                    appearance::save(&c)?;
                    crate::backend::themehook::ensure();
                    appearance::apply_icons(&c)
                },
                |r: anyhow::Result<()>| {
                    if let Err(e) = r {
                        crate::window::toast(&format!("Couldn't set icons: {e}"));
                    }
                },
            );
        },
    );
    widgets::keywords("icon theme icons yaru papirus adwaita");
    g.add(&r);
}

fn interface_font_row() -> gtk::Box {
    let button = gtk::FontDialogButton::new(Some(gtk::FontDialog::new()));
    button.set_font_desc(&gtk::pango::FontDescription::from_string(&appearance::interface_font()));
    button.set_use_font(true);
    button.set_use_size(true);
    button.connect_font_desc_notify(|b| {
        let Some(desc) = b.font_desc() else { return };
        let desc = desc.to_string();
        if desc == appearance::interface_font() {
            return;
        }
        cmd::background(move || appearance::set_interface_font(&desc), |r| {
            if let Err(e) = r {
                crate::window::toast(&format!("Couldn't set font: {e}"));
            }
        });
    });
    widgets::keywords("interface font ui text gtk qt size");
    widgets::row("Interface font", "Text in app windows, menus and dialogs (GTK and Qt).", Some(button.upcast_ref()))
}
