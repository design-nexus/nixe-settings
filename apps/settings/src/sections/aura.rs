use crate::backend::asus;
use crate::backend::kbdidle;
use crate::widgets::{self, Debounce, Page, opts};
use crate::{cmd, window};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// ----- Keyboard backlight timeout (also used by the Power page) -----

pub fn timeout_row() -> Option<gtk::Box> {
    if !kbdidle::available() {
        return None;
    }
    let cur = kbdidle::load().timeout_secs;
    let mut options: Vec<(String, String)> = kbdidle::CHOICES.iter().map(|(s, l)| (s.to_string(), l.to_string())).collect();
    if !kbdidle::CHOICES.iter().any(|(s, _)| *s == cur) {
        options.push((cur.to_string(), format!("{cur} seconds")));
    }
    let (r, _) = widgets::choice_row(
        "Turn off after",
        "Switch the backlight off after this long without typing or touching the trackpad. It comes back on the next key \
         press or touch.",
        options,
        &cur.to_string(),
        |v| {
            let secs = v.parse::<u32>().unwrap_or(0);
            cmd::background(
                move || kbdidle::apply(&kbdidle::Config { timeout_secs: secs }).map_err(|e| format!("{e:#}")),
                move |r| match r {
                    Ok(()) if secs == 0 => window::toast("Keyboard backlight stays on"),
                    Ok(()) => window::toast("Keyboard backlight timeout set"),
                    Err(e) => window::toast(&format!("Couldn't set the timeout: {e}")),
                },
            );
        },
    );
    widgets::keywords("keyboard backlight timeout idle off sleep dim inactivity");
    Some(r)
}

fn on_off_row(title: &str, desc: &str, on_pick: impl Fn(bool) + 'static) -> gtk::Box {
    let bx = widgets::hbox(6);
    let on = gtk::Button::with_label("On");
    let off = gtk::Button::with_label("Off");
    let cb = Rc::new(on_pick);
    let c1 = cb.clone();
    on.connect_clicked(move |_| c1(true));
    off.connect_clicked(move |_| cb(false));
    bx.append(&on);
    bx.append(&off);
    widgets::row(title, desc, Some(bx.upcast_ref()))
}

pub fn build(page: &Page) {
    if asus::has_aura() {
        keyboard(page);
        lighting_states(page);
    } else {
        keyboard_backlight_only(page);
    }
    if asus::has_slash() {
        slash(page);
    }
    if asus::has_anime() {
        anime(page);
    }
    if asus::has_xgm()
        && let Some(on) = asus::xgm_state()
    {
        let g = page.group("XG Mobile");
        let (r, _) = widgets::switch_row("Light", "The light on a connected XG Mobile dock.", on, |on| {
            asus::apply(vec!["xgmled".into(), "set".into(), if on { "1" } else { "0" }.into()]);
        });
        g.add(&r);
    }
    if asus::has_scsi() {
        drive_leds(page);
    }
}

// ----- Keyboard -----

fn brightness_control() -> Option<gtk::Box> {
    let levels: Vec<String> = asus::support().brightness.iter().map(|l| l.to_lowercase()).collect();
    if levels.is_empty() {
        return None;
    }
    let current = cmd::output(&["asusctl", "leds", "get"])
        .map(|t| t.rsplit(':').next().unwrap_or("").trim().to_lowercase())
        .unwrap_or_default();
    let options: Vec<(String, String)> = levels
        .iter()
        .map(|l| {
            let label = match l.as_str() {
                "off" => "Off",
                "low" => "Low",
                "med" => "Medium",
                "high" => "High",
                other => other,
            };
            (l.clone(), label.to_string())
        })
        .collect();
    Some(widgets::segmented(&options, &current, |l| asus::apply(vec!["leds".into(), "set".into(), l])))
}

fn keyboard_backlight_only(page: &Page) {
    let g = page.group("Keyboard backlight");
    if let Some(b) = brightness_control() {
        g.add(&widgets::row("Brightness", "Also on the keyboard's brightness keys.", Some(b.upcast_ref())));
    }
    if let Some(r) = timeout_row() {
        g.add(&r);
    }
}

fn hsv(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let c = gtk::gdk::RGBA::parse(hex).unwrap_or(gtk::gdk::RGBA::WHITE);
    (c.red() as f64, c.green() as f64, c.blue() as f64)
}

/// A small keyboard drawn in the effect's colours, animated like the real one.
fn preview(choice: Rc<RefCell<asus::AuraChoice>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("kbd-preview");
    area.set_content_height(64);
    area.set_hexpand(true);
    area.set_draw_func(move |_, cr, w, h| {
        let c = asus::resolved(&choice.borrow());
        let period = match c.speed.as_str() {
            "low" => 6.0,
            "high" => 1.8,
            _ => 3.5,
        };
        let t = gtk::glib::monotonic_time() as f64 / 1_000_000.0;
        let phase = (t / period).fract();
        let wave = 0.5 - 0.5 * (phase * std::f64::consts::TAU).cos();
        let (a, b) = (rgb(&c.colour1), rgb(&c.colour2));
        let (cols, rows) = (18.0, 3.0);
        let (pad, gap) = (10.0, 4.0);
        let kw = (w as f64 - 2.0 * pad - (cols - 1.0) * gap) / cols;
        let kh = (h as f64 - 2.0 * pad - (rows - 1.0) * gap) / rows;
        for r in 0..rows as usize {
            for k in 0..cols as usize {
                let x = pad + k as f64 * (kw + gap);
                let y = pad + r as f64 * (kh + gap);
                let pos = k as f64 / cols;
                let (cr_, cg, cb) = match c.mode.as_str() {
                    // Fades between the two colours.
                    "breathe" | "stars" => (a.0 + (b.0 - a.0) * wave, a.1 + (b.1 - a.1) * wave, a.2 + (b.2 - a.2) * wave),
                    "pulse" => {
                        let v = 0.2 + 0.8 * wave;
                        (a.0 * v, a.1 * v, a.2 * v)
                    }
                    "rainbow-cycle" => hsv(phase, 0.85, 1.0),
                    "rainbow-wave" => {
                        let dir = if c.direction == "left" || c.direction == "up" { 1.0 } else { -1.0 };
                        hsv(phase + dir * pos, 0.85, 1.0)
                    }
                    _ => a,
                };
                let radius = 3.0;
                cr.new_sub_path();
                cr.arc(x + kw - radius, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
                cr.arc(x + kw - radius, y + kh - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
                cr.arc(x + radius, y + kh - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
                cr.arc(x + radius, y + radius, radius, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
                cr.close_path();
                cr.set_source_rgba(cr_, cg, cb, 0.92);
                let _ = cr.fill();
            }
        }
    });
    area.add_tick_callback(|a, _| {
        a.queue_draw();
        gtk::glib::ControlFlow::Continue
    });
    area
}

fn keyboard(page: &Page) {
    let modes = asus::usable_modes(asus::support());
    let g = page.group("Keyboard");
    if modes.is_empty() {
        keyboard_backlight_only(page);
        return;
    }

    // What to show: the saved choice, else what the keyboard is doing now.
    let start = asus::load_aura().unwrap_or_else(|| {
        let live = asus::aura_state();
        let mut c = asus::AuraChoice::default();
        if let Some(l) = live {
            c.mode = l.mode;
            c.colour1 = l.colour1;
            c.colour2 = l.colour2;
            if ["low", "med", "high"].contains(&l.speed.as_str()) {
                c.speed = l.speed;
            }
            if ["up", "down", "left", "right"].contains(&l.direction.as_str()) {
                c.direction = l.direction;
            }
        }
        c
    });
    let mut start = start;
    if !modes.contains(&start.mode) {
        start.mode = modes[0].clone();
    }
    let choice = Rc::new(RefCell::new(start.clone()));
    let debounce = Debounce::default();
    let commit: Rc<dyn Fn()> = {
        let choice = choice.clone();
        Rc::new(move || {
            let c = choice.borrow().clone();
            let _ = asus::save_aura(&c);
            debounce.call(350, move || {
                cmd::background(
                    move || asus::apply_aura(&c).map_err(|e| format!("{e:#}")),
                    |r| {
                        if let Err(e) = r {
                            window::toast(&format!("Couldn't set the keyboard lighting: {e}"));
                        }
                    },
                );
            });
        })
    };

    g.add(&preview(choice.clone()));

    if let Some(b) = brightness_control() {
        g.add(&widgets::row("Brightness", "Also on the keyboard's brightness keys.", Some(b.upcast_ref())));
    }

    // Effect.
    let effect_opts: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), asus::pretty(&heading_case(m)))).collect();
    let effect_row = widgets::vbox(0);
    g.add(&effect_row);

    // Colours.
    let theme_colour = asus::theme_keyboard_colour();
    let follow_switch = gtk::Switch::new();
    follow_switch.set_active(start.follow_theme);
    let follow_guard = Rc::new(std::cell::Cell::new(false));
    let picker1 = {
        let theme_swatch = theme_colour.as_deref().map(|c| ("Omarchy theme", c));
        let (choice, commit, sw, guard, theme_colour) =
            (choice.clone(), commit.clone(), follow_switch.clone(), follow_guard.clone(), theme_colour.clone());
        widgets::colour_picker(&asus::resolved(&start).colour1, theme_swatch, move |hex| {
            // Picking the theme swatch follows the theme; any other colour stops following.
            let follow = theme_colour.as_deref() == Some(hex.as_str());
            {
                let mut c = choice.borrow_mut();
                c.colour1 = hex;
                c.follow_theme = follow;
            }
            guard.set(true);
            sw.set_active(follow);
            guard.set(false);
            commit();
        })
    };
    let picker1 = Rc::new(picker1);
    let c1_row = widgets::stacked_row("Colour", "", picker1.widget.upcast_ref());
    widgets::keywords("colour color rgb hex");
    let picker2 = {
        let (choice, commit) = (choice.clone(), commit.clone());
        widgets::colour_picker(&start.colour2, None, move |hex| {
            choice.borrow_mut().colour2 = hex;
            commit();
        })
    };
    let c2_row = widgets::stacked_row("Second colour", "Breathing fades between the two colours.", picker2.widget.upcast_ref());
    {
        let (choice, commit, picker1, guard) = (choice.clone(), commit.clone(), picker1.clone(), follow_guard.clone());
        let theme_colour = theme_colour.clone();
        follow_switch.connect_active_notify(move |sw| {
            if guard.get() {
                return;
            }
            let on = sw.is_active();
            choice.borrow_mut().follow_theme = on;
            if on && let Some(t) = &theme_colour {
                choice.borrow_mut().colour1 = t.clone();
                picker1.show(t);
            }
            commit();
        });
    }
    let follow_row = widgets::row(
        "Follow Omarchy theme",
        "Use the theme's keyboard colour, and update it whenever you change themes. Your effect is kept.",
        Some(follow_switch.upcast_ref()),
    );

    let speed_row = {
        let (choice, commit) = (choice.clone(), commit.clone());
        let s = widgets::segmented(&opts(&[("low", "Slow"), ("med", "Medium"), ("high", "Fast")]), &start.speed, move |v| {
            choice.borrow_mut().speed = v;
            commit();
        });
        widgets::row("Speed", "", Some(s.upcast_ref()))
    };
    let dir_row = {
        let (choice, commit) = (choice.clone(), commit.clone());
        let s = widgets::segmented(
            &opts(&[("left", "←"), ("right", "→"), ("up", "↑"), ("down", "↓")]),
            &start.direction,
            move |v| {
                choice.borrow_mut().direction = v;
                commit();
            },
        );
        widgets::row("Direction", "", Some(s.upcast_ref()))
    };

    // Holders keep rows hidden by the effect hidden even after a search is cleared.
    let holder = |row: &gtk::Box| {
        let h = widgets::vbox(0);
        h.append(row);
        h
    };
    let (follow_h, c1_h, c2_h, speed_h, dir_h) =
        (holder(&follow_row), holder(&c1_row), holder(&c2_row), holder(&speed_row), holder(&dir_row));
    let show_for = {
        let (f, c1, c2, sp, di) = (follow_h.clone(), c1_h.clone(), c2_h.clone(), speed_h.clone(), dir_h.clone());
        move |mode: &str| {
            let p = asus::effect_params(mode);
            f.set_visible(p.colours >= 1);
            c1.set_visible(p.colours >= 1);
            c2.set_visible(p.colours >= 2);
            // Breathe needs a speed on the command line, but the keyboard ignores it.
            sp.set_visible(p.speed && mode != "breathe");
            di.set_visible(p.direction);
        }
    };
    show_for(&start.mode);
    let effects = {
        let (choice, commit) = (choice.clone(), commit.clone());
        widgets::segmented(&effect_opts, &start.mode, move |m| {
            show_for(&m);
            choice.borrow_mut().mode = m;
            commit();
        })
    };
    let effect_card = widgets::stacked_row("Effect", "", effects.upcast_ref());
    widgets::keywords("aura rgb lighting animation rainbow breathe static pulse effect");
    effect_row.append(&effect_card);

    g.add(&follow_h);
    g.add(&c1_h);
    g.add(&c2_h);
    g.add(&speed_h);
    g.add(&dir_h);
    if let Some(r) = timeout_row() {
        g.add(&r);
    }
}

/// `rainbow-wave` -> `RainbowWave`, so [`asus::pretty`] can split it into words.
fn heading_case(kebab: &str) -> String {
    kebab.split('-').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

// ----- Lighting states -----

fn lighting_states(page: &Page) {
    let entries = asus::aura_power();
    let names = &asus::support().power_zones;
    if entries.is_empty() {
        return;
    }
    let g = page.group("When the lights are on");
    g.note("Pick the moments each zone lights up. For example, leave <b>Sleep</b> off so the keyboard is dark while the laptop sleeps.");
    for (i, (_zone, flags)) in entries.iter().enumerate() {
        let name = if names.len() == entries.len() {
            names[i].clone()
        } else if entries.len() == 1 {
            "Keyboard".to_string()
        } else {
            continue;
        };
        let cli = asus::kebab(&name);
        let states = widgets::chip_toggles(&["Boot", "Awake", "Sleep", "Shutdown"], flags, move |v| {
            let s: asus::PowerStates = [v[0], v[1], v[2], v[3]];
            asus::apply(asus::power_args(&cli, s));
        });
        g.add(&widgets::row(&asus::pretty(&name), "", Some(states.upcast_ref())));
    }
    widgets::keywords("boot awake sleep shutdown power state startup");
}

// ----- Slash -----

fn slash(page: &Page) {
    let s = asus::slash();
    let g = page.group("Slash lightbar");
    let settings = widgets::vbox(6);
    settings.set_visible(s.enabled);
    let shown = settings.clone();
    let (r, _) = widgets::switch_row(
        "Slash lightbar",
        "The light strip on the back of the lid. Its LEDs are white only, so there's no colour to choose.",
        s.enabled,
        move |on| {
            shown.set_visible(on);
            asus::apply(vec!["slash".into(), "set".into(), if on { "--enable" } else { "--disable" }.into()]);
        },
    );
    widgets::keywords("slash led lid strip lightbar");
    g.add(&r);
    g.add(&settings);

    let modes = asus::slash_modes();
    if !modes.is_empty() {
        let options: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), asus::pretty(m))).collect();
        let (r, _) = widgets::choice_row("Animation", "", options, &s.mode, |m| {
            asus::apply(vec!["slash".into(), "set".into(), "--mode".into(), m]);
        });
        settings.append(&r);
    }
    let debounce = Debounce::default();
    let (r, _) =
        widgets::slider_row("Brightness", "", (0.0, 100.0, 1.0), (s.brightness as f64 / 2.55).round(), 0, "%", move |v| {
            debounce.call(400, move || {
                asus::apply(vec!["slash".into(), "set".into(), "-l".into(), ((v * 2.55).round() as i64).to_string()]);
            });
        });
    settings.append(&r);
    let debounce = Debounce::default();
    let (r, _) = widgets::slider_row(
        "Pause between animations",
        "How long the lightbar waits before playing the animation again. 0 repeats straight away.",
        (0.0, 5.0, 1.0),
        s.interval as f64,
        0,
        "",
        move |v| {
            debounce.call(400, move || {
                asus::apply(vec!["slash".into(), "set".into(), "--interval".into(), (v.round() as i64).to_string()]);
            });
        },
    );
    widgets::keywords("interval delay repeat pause gap");
    settings.append(&r);

    let flags = ["-B", "-S", "-s", "-b"];
    let when = widgets::chip_toggles(
        &["Boot", "Shutdown", "Sleep", "On battery"],
        &[s.on_boot, s.on_shutdown, s.on_sleep, s.on_battery],
        {
            let last = Rc::new(RefCell::new(vec![s.on_boot, s.on_shutdown, s.on_sleep, s.on_battery]));
            move |v: Vec<bool>| {
                // Send only the one that changed.
                let mut prev = last.borrow_mut();
                for (i, (now, was)) in v.iter().zip(prev.iter()).enumerate() {
                    if now != was {
                        asus::apply(vec!["slash".into(), "set".into(), flags[i].into(), now.to_string()]);
                    }
                }
                *prev = v;
            }
        },
    );
    settings.append(&widgets::row("Show the animation", "Besides while you're using the laptop.", Some(when.upcast_ref())));
    widgets::keywords("boot shutdown sleep battery show animation");
    let (r, _) = widgets::switch_row(
        "Low battery warning",
        "Flash a warning on the lightbar when the battery is low.",
        s.battery_warning,
        |on| {
            asus::apply(vec!["slash".into(), "set".into(), "-w".into(), on.to_string()]);
        },
    );
    settings.append(&r);
}

// ----- Anime matrix -----

fn anime(page: &Page) {
    let g = page.group("AniMe Matrix");
    g.note("These can't be read back from the laptop, so they show buttons instead of switches.");
    g.add(&on_off_row("Display", "", |on| asus::apply(vec!["anime".into(), "--enable-display".into(), on.to_string()])));
    let (r, _) = widgets::choice_row(
        "Brightness",
        "",
        opts(&[("off", "Off"), ("low", "Low"), ("med", "Medium"), ("high", "High")]),
        "",
        |b| asus::apply(vec!["anime".into(), "--brightness".into(), b]),
    );
    g.add(&r);
    g.add(&on_off_row("Built-in animations", "", |on| {
        asus::apply(vec!["anime".into(), "--enable-powersave-anim".into(), on.to_string()])
    }));
    g.add(&on_off_row("Off when unplugged", "", |on| {
        asus::apply(vec!["anime".into(), "--off-when-unplugged".into(), on.to_string()])
    }));
    g.add(&on_off_row("Off when asleep", "", |on| {
        asus::apply(vec!["anime".into(), "--off-when-suspended".into(), on.to_string()])
    }));
    g.add(&on_off_row("Off when the lid is closed", "", |on| {
        asus::apply(vec!["anime".into(), "--off-when-lid-closed".into(), on.to_string()])
    }));
    let (r, _) = widgets::button_row("Clear the display", "", "Clear", |_| asus::apply(vec!["anime".into(), "--clear".into()]));
    g.add(&r);
}

// ----- Drive LEDs -----

fn drive_leds(page: &Page) {
    let g = page.group("Drive lights");
    g.note("Lights on external ASUS drives. These can't be read back from the laptop.");
    g.add(&on_off_row("Lights", "", |on| asus::apply(vec!["scsi".into(), "--enable".into(), on.to_string()])));
    let modes: Vec<(String, String)> = cmd::output(&["asusctl", "scsi", "--list"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(|l| (l.to_string(), l.to_string()))
        .collect();
    if !modes.is_empty() {
        let (r, _) = widgets::choice_row("Effect", "", modes, "", |m| asus::apply(vec!["scsi".into(), "--mode".into(), m]));
        g.add(&r);
    }
    let (r, _) = widgets::choice_row(
        "Speed",
        "",
        opts(&[("slowest", "Slowest"), ("slow", "Slow"), ("med", "Medium"), ("fast", "Fast"), ("fastest", "Fastest")]),
        "",
        |s| asus::apply(vec!["scsi".into(), "--speed".into(), s]),
    );
    g.add(&r);
    let (r, _) = widgets::choice_row("Direction", "", opts(&[("forward", "Forward"), ("reverse", "Reverse")]), "", |d| {
        asus::apply(vec!["scsi".into(), "--direction".into(), d]);
    });
    g.add(&r);
    let b = widgets::colour_button("#7aa2f7", |c| asus::apply(vec!["scsi".into(), "--colours".into(), asus::hex6(&c)]));
    g.add(&widgets::row("Colour", "", Some(b.upcast_ref())));
}
