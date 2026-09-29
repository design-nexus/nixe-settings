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
    let (r, _) = widgets::choice_row(
        "System font",
        "The monospace font used by terminals, the bar and the lock screen.",
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
}
