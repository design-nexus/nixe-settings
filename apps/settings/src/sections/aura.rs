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
    keyboard_backlight(page);
    if asus::has_aura() {
        keyboard_effect(page);
        lighting_states(page);
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

// ----- Keyboard backlight -----

fn keyboard_backlight(page: &Page) {
    let g = page.group("Keyboard backlight");
    let levels: Vec<String> = asus::support().brightness.iter().map(|l| l.to_lowercase()).collect();
    if !levels.is_empty() {
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
        let (r, _) = widgets::choice_row("Brightness", "Also on the keyboard's brightness keys.", options, &current, |l| {
            asus::apply(vec!["leds".into(), "set".into(), l]);
        });
        g.add(&r);
    }
    if let Some(r) = timeout_row() {
        g.add(&r);
    }
}

// ----- Aura effect -----

#[derive(Clone)]
struct Effect {
    mode: String,
    c1: String,
    c2: String,
    speed: String,
    direction: String,
}

fn keyboard_effect(page: &Page) {
    let g = page.group("Keyboard effect");
    let modes: Vec<String> = asus::support().modes.iter().map(|m| asus::kebab(m)).collect();
    if modes.is_empty() {
        g.add(&widgets::row("No effects reported", "This keyboard doesn't list any lighting effects.", None));
        return;
    }
    let live = asus::aura_state();
    let start = Effect {
        mode: live.as_ref().map(|s| s.mode.clone()).filter(|m| modes.contains(m)).unwrap_or_else(|| modes[0].clone()),
        c1: live.as_ref().map(|s| s.colour1.clone()).unwrap_or_else(|| "#7aa2f7".into()),
        c2: live.as_ref().map(|s| s.colour2.clone()).unwrap_or_else(|| "#000000".into()),
        speed: live
            .as_ref()
            .map(|s| s.speed.clone())
            .filter(|s| ["low", "med", "high"].contains(&s.as_str()))
            .unwrap_or_else(|| "med".into()),
        direction: live
            .as_ref()
            .map(|s| s.direction.clone())
            .filter(|d| ["up", "down", "left", "right"].contains(&d.as_str()))
            .unwrap_or_else(|| "right".into()),
    };
    let state = Rc::new(RefCell::new(start.clone()));
    let debounce = Debounce::default();
    let apply: Rc<dyn Fn()> = {
        let state = state.clone();
        Rc::new(move || {
            let state = state.clone();
            debounce.call(450, move || {
                let s = state.borrow().clone();
                asus::apply(asus::effect_args(&s.mode, &asus::hex6(&s.c1), &asus::hex6(&s.c2), &s.speed, &s.direction));
            });
        })
    };

    // Rows whose visibility depends on the effect.
    let c1_row = {
        let (state, apply) = (state.clone(), apply.clone());
        let b = widgets::colour_button(&start.c1, move |c| {
            state.borrow_mut().c1 = c;
            apply();
        });
        widgets::row("Colour", "", Some(b.upcast_ref()))
    };
    let c2_row = {
        let (state, apply) = (state.clone(), apply.clone());
        let b = widgets::colour_button(&start.c2, move |c| {
            state.borrow_mut().c2 = c;
            apply();
        });
        widgets::row("Second colour", "", Some(b.upcast_ref()))
    };
    let speed_row = {
        let (state, apply) = (state.clone(), apply.clone());
        let (r, _) = widgets::choice_row(
            "Speed",
            "",
            opts(&[("low", "Slow"), ("med", "Medium"), ("high", "Fast")]),
            &start.speed,
            move |v| {
                state.borrow_mut().speed = v;
                apply();
            },
        );
        r
    };
    let dir_row = {
        let (state, apply) = (state.clone(), apply.clone());
        let (r, _) = widgets::choice_row(
            "Direction",
            "",
            opts(&[("right", "Right"), ("left", "Left"), ("up", "Up"), ("down", "Down")]),
            &start.direction,
            move |v| {
                state.borrow_mut().direction = v;
                apply();
            },
        );
        r
    };
    // Each row sits in a holder that logic shows or hides, so clearing a search
    // (which re-shows every row it hid) never brings back a row the effect doesn't use.
    let holder = |row: &gtk::Box| {
        let h = widgets::vbox(0);
        h.append(row);
        h
    };
    let (c1_h, c2_h, speed_h, dir_h) = (holder(&c1_row), holder(&c2_row), holder(&speed_row), holder(&dir_row));
    let show_for = {
        let (c1, c2, sp, di) = (c1_h.clone(), c2_h.clone(), speed_h.clone(), dir_h.clone());
        move |mode: &str| {
            let p = asus::effect_params(mode);
            c1.set_visible(p.colours >= 1);
            c2.set_visible(p.colours >= 2);
            sp.set_visible(p.speed);
            di.set_visible(p.direction);
        }
    };
    show_for(&start.mode);

    let options: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), asus::pretty(&heading_case(m)))).collect();
    let (mode_row, _) = {
        let (state, apply) = (state.clone(), apply.clone());
        widgets::choice_row("Effect", "", options, &start.mode, move |m| {
            show_for(&m);
            state.borrow_mut().mode = m;
            apply();
        })
    };
    widgets::keywords("aura rgb colour color lighting animation rainbow breathe static pulse");
    g.add(&mode_row);
    g.add(&c1_h);
    g.add(&c2_h);
    g.add(&speed_h);
    g.add(&dir_h);
    if cmd::present("omarchy-theme-set-keyboard-asus-rog") {
        let (r, _) =
            widgets::button_row("Match the Omarchy theme", "Set the keyboard to the current theme's colour.", "Match", |_| {
                cmd::spawn(&["omarchy-theme-set-keyboard-asus-rog"])
            });
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
    g.note("Choose which power states light each zone. For example, turn the keyboard off while the laptop sleeps.");
    for (i, (zone_id, flags)) in entries.iter().enumerate() {
        let name = if names.len() == entries.len() {
            names[i].clone()
        } else if entries.len() == 1 {
            "Keyboard".to_string()
        } else {
            continue;
        };
        let cli = asus::kebab(&name);
        let title = format!("{} lighting", asus::pretty(&name));
        let states = widgets::chip_toggles(&["Boot", "Awake", "Sleep", "Shutdown"], flags, move |v| {
            let s: asus::PowerStates = [v[0], v[1], v[2], v[3]];
            asus::apply(asus::power_args(&cli, s));
        });
        let _ = zone_id;
        g.add(&widgets::row(&title, "", Some(states.upcast_ref())));
    }
    widgets::keywords("boot awake sleep shutdown power state startup");
}

// ----- Slash -----

fn slash(page: &Page) {
    let s = asus::slash();
    let g = page.group("Slash lightbar");
    let (r, _) = widgets::switch_row("Slash lightbar", "The light strip on the back of the lid.", s.enabled, |on| {
        asus::apply(vec!["slash".into(), "set".into(), if on { "--enable" } else { "--disable" }.into()]);
    });
    widgets::keywords("slash led lid strip lightbar");
    g.add(&r);

    let modes = asus::slash_modes();
    if !modes.is_empty() {
        let options: Vec<(String, String)> = modes.iter().map(|m| (m.clone(), asus::pretty(m))).collect();
        let (r, _) = widgets::choice_row("Animation", "", options, &s.mode, |m| {
            asus::apply(vec!["slash".into(), "set".into(), "--mode".into(), m]);
        });
        g.add(&r);
    }
    let debounce = Debounce::default();
    let (r, _) =
        widgets::slider_row("Brightness", "", (0.0, 100.0, 1.0), (s.brightness as f64 / 2.55).round(), 0, "%", move |v| {
            debounce.call(400, move || {
                asus::apply(vec!["slash".into(), "set".into(), "-l".into(), ((v * 2.55).round() as i64).to_string()]);
            });
        });
    g.add(&r);
    let debounce = Debounce::default();
    let (r, _) = widgets::slider_row(
        "Interval",
        "How the animation is paced, 0 to 5. Try a few values to see what suits the animation.",
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
    g.add(&r);

    let g = page.group("Slash: show the animation");
    for (title, desc, flag, on) in [
        ("On boot", "", "-B", s.on_boot),
        ("On shutdown", "", "-S", s.on_shutdown),
        ("While asleep", "", "-s", s.on_sleep),
        ("On battery", "Keep it lit when unplugged.", "-b", s.on_battery),
        ("Low battery warning", "Flash a warning when the battery is low.", "-w", s.battery_warning),
    ] {
        let (r, _) = widgets::switch_row(title, desc, on, move |on| {
            asus::apply(vec!["slash".into(), "set".into(), flag.into(), on.to_string()]);
        });
        g.add(&r);
    }
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
