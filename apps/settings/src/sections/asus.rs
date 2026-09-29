use crate::backend::asus::{self, Attribute, Kind};
use crate::units;
use crate::widgets::{self, Debounce, Page};
use crate::{cmd, window};
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn available() -> bool {
    asus::available()
}

thread_local! {
    /// Which profile the fan curve editor is showing (empty: the active one).
    static FAN_PROFILE: RefCell<String> = const { RefCell::new(String::new()) };
}

fn after_colon(text: &str) -> String {
    text.rsplit(':').next().unwrap_or("").trim().to_string()
}

pub fn build(page: &Page) {
    let sup = asus::support();
    if !sup.product.is_empty() {
        page.banner(
            &format!("<b>{}</b> · {}", glib::markup_escape_text(&sup.product), glib::markup_escape_text(&sup.board)),
            false,
        );
    }

    let profiles: Vec<(String, String)> = cmd::output(&["asusctl", "profile", "list"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|l| (l.clone(), l))
        .collect();

    if asus::has_platform_profile() {
        performance(page, &profiles);
    }
    if asus::has_charge_limit() {
        battery(page);
    }
    if asus::has_fan_curves() && !profiles.is_empty() {
        fan_curves(page, &profiles);
    }
    if asus::has_armoury() {
        firmware(page);
    }
    if asus::has_screenpad() {
        screenpad(page);
    }
}

// ----- Performance -----

fn performance(page: &Page, profiles: &[(String, String)]) {
    let info = cmd::output(&["asusctl", "profile", "get"]).unwrap_or_default();
    let find = |prefix: &str| {
        info.lines()
            .find(|l| l.trim_start().starts_with(prefix))
            .map(|l| l.trim_start()[prefix.len()..].trim().trim_start_matches(':').trim().to_string())
            .unwrap_or_default()
    };
    let g = page.group("Performance profile");
    g.note("Sets fan behaviour and power limits. Changing it also changes the fan curve shown below.");
    let (r, _) = widgets::choice_row("Right now", "", profiles.to_vec(), &find("Active profile"), |p| {
        asus::apply(vec!["profile".into(), "set".into(), p]);
    });
    g.add(&r);
    let (r, _) = widgets::choice_row("When plugged in", "", profiles.to_vec(), &find("AC profile"), |p| {
        asus::apply(vec!["profile".into(), "set".into(), "-a".into(), p]);
    });
    g.add(&r);
    let (r, _) = widgets::choice_row("On battery", "", profiles.to_vec(), &find("Battery profile"), |p| {
        asus::apply(vec!["profile".into(), "set".into(), "-b".into(), p]);
    });
    g.add(&r);
    if let Some(t) = cmd::output(&["asusctl", "profile", "tuning"]).and_then(|t| t.contains("Profile tuning:").then_some(t)) {
        let on = after_colon(&t) == "true";
        let (r, _) =
            widgets::switch_row("Profile tuning", "Apply per-profile power tuning (advanced; off by default).", on, |on| {
                asus::apply(vec!["profile".into(), "tuning".into(), on.to_string()]);
            });
        g.add(&r);
    }
}

// ----- Battery -----

fn battery(page: &Page) {
    let g = page.group("Battery care");
    let limit = cmd::output(&["asusctl", "battery", "info"])
        .map(|s| after_colon(&s).trim_end_matches('%').parse::<f64>().unwrap_or(100.0))
        .unwrap_or(100.0);
    let (r, s) = widgets::slider_row(
        "Charge limit",
        "Stop charging at this level. 80% keeps the battery healthy if you're usually plugged in.",
        (20.0, 100.0, 5.0),
        limit,
        0,
        "%",
        |_| {},
    );
    // Apply on release, not on every step.
    let click = gtk::GestureClick::new();
    let scale = s.scale.clone();
    click.connect_released(move |_, _, _, _| {
        asus::apply(vec!["battery".into(), "limit".into(), (scale.value().round() as i64).to_string()]);
    });
    s.scale.add_controller(click);
    let key = gtk::EventControllerKey::new();
    let scale = s.scale.clone();
    key.connect_key_released(move |_, _, _, _| {
        asus::apply(vec!["battery".into(), "limit".into(), (scale.value().round() as i64).to_string()]);
    });
    s.scale.add_controller(key);
    g.add(&r);
    let (r, _) =
        widgets::button_row("Charge to full once", "Ignore the limit until the next full charge.", "Charge to 100%", |_| {
            asus::apply(vec!["battery".into(), "oneshot".into()]);
        });
    g.add(&r);
}

// ----- Fan curves -----

fn fan_curves(page: &Page, profiles: &[(String, String)]) {
    let active = cmd::output(&["asusctl", "profile", "get"])
        .and_then(|t| t.lines().find(|l| l.contains("Active profile")).map(after_colon))
        .unwrap_or_else(|| profiles[0].0.clone());
    let profile = FAN_PROFILE.with(|p| {
        let cur = p.borrow().clone();
        if profiles.iter().any(|(id, _)| *id == cur) { cur } else { active.clone() }
    });

    let g = page.group("Fan curves");
    g.note(
        "Set how fast each fan runs at each temperature. <b>Fans running too slowly can overheat the laptop</b> — the \
         firmware still protects it, but keep speeds rising with temperature.",
    );
    let (r, _) = widgets::choice_row(
        "Profile to edit",
        "Each performance profile has its own curves.",
        profiles.to_vec(),
        &profile,
        |p| {
            FAN_PROFILE.with(|f| *f.borrow_mut() = p);
            window::rebuild("asus");
        },
    );
    g.add(&r);

    let curves = asus::fan_curves(&profile);
    if curves.is_empty() {
        g.add(&widgets::row("No fan curves for this profile", "", None));
        return;
    }
    let enabled = curves.iter().any(|c| c.enabled);
    // Everything below the switch only matters while custom curves are on.
    let curves_box = widgets::vbox(6);
    curves_box.set_visible(enabled);
    let p2 = profile.clone();
    let shown = curves_box.clone();
    let (r, _) = widgets::switch_row(
        "Use custom fan curves",
        "Off uses the firmware's own curves for this profile.",
        enabled,
        move |on| {
            shown.set_visible(on);
            asus::apply(vec![
                "fan-curve".into(),
                "--mod-profile".into(),
                p2.clone(),
                "--enable-fan-curves".into(),
                on.to_string(),
            ]);
        },
    );
    g.add(&r);
    g.add(&curves_box);
    if profile == active {
        let (r, _) = widgets::button_row("Reset to the firmware's curves", "For the active profile.", "Reset", |_| {
            asus::apply(vec!["fan-curve".into(), "--default".into()]);
            glib::timeout_add_local_once(std::time::Duration::from_millis(900), || window::rebuild("asus"));
        });
        curves_box.append(&r);
    }

    for curve in curves {
        let fan = curve.fan.clone();
        let pwm = Rc::new(RefCell::new(curve.pwm.clone()));
        let temp = Rc::new(RefCell::new(curve.temp.clone()));
        let debounce = Debounce::default();
        let send = {
            let (pwm, temp, debounce, profile, fan) = (pwm.clone(), temp.clone(), debounce.clone(), profile.clone(), fan.clone());
            Rc::new(move || {
                let (pwm, temp, profile, fan) = (pwm.clone(), temp.clone(), profile.clone(), fan.clone());
                debounce.call(700, move || match asus::fan_data(&temp.borrow(), &pwm.borrow()) {
                    Ok(data) => asus::apply(vec![
                        "fan-curve".into(),
                        "--mod-profile".into(),
                        profile,
                        "--fan".into(),
                        fan.to_lowercase(),
                        "--data".into(),
                        data,
                    ]),
                    Err(e) => window::toast(&format!("{fan} curve not applied: {e}")),
                });
            })
        };

        let row = widgets::hbox(4);
        for i in 0..curve.pwm.len() {
            let col = widgets::vbox(6);
            col.set_hexpand(true);
            let (f, scale) = widgets::vfader("", (0.0, 100.0), asus::pwm_to_percent(curve.pwm[i]) as f64, |v| {
                format!("{}%", v.round() as i64)
            });
            scale.set_size_request(-1, 130);
            {
                let (pwm, send) = (pwm.clone(), send.clone());
                scale.connect_value_changed(move |s| {
                    pwm.borrow_mut()[i] = asus::percent_to_pwm(s.value().round() as u32);
                    send();
                });
            }
            col.append(&f);
            // Shown in the chosen unit; the laptop always gets whole °C.
            let spin = gtk::SpinButton::with_range(units::from_celsius(0.0), units::from_celsius(110.0), units::step());
            spin.set_value(units::from_celsius(curve.temp[i] as f64));
            spin.set_width_chars(3);
            spin.set_tooltip_text(Some(&format!("Temperature ({})", units::symbol())));
            {
                let (temp, send) = (temp.clone(), send.clone());
                spin.connect_value_changed(move |s| {
                    temp.borrow_mut()[i] = units::to_celsius(s.value()).max(0) as u32;
                    send();
                });
            }
            col.append(&spin);
            row.append(&col);
        }
        let head = widgets::hbox(8);
        head.append(&widgets::label(&format!("Speed above, temperature ({}) below", units::symbol()), "dim"));
        let body = widgets::vbox(8);
        body.append(&head);
        body.append(&row);
        curves_box.append(&widgets::stacked_row(&format!("{} fan", curve.fan), "", body.upcast_ref()));
        widgets::keywords("fan curve speed temperature cooling");
    }
}

// ----- Firmware & power limits -----

struct Meta {
    title: &'static str,
    desc: &'static str,
    unit: &'static str,
    restart: bool,
    advanced: bool,
}

fn meta(name: &str) -> Meta {
    let m = |title, desc, unit, restart, advanced| Meta { title, desc, unit, restart, advanced };
    match name {
        "boot_sound" => m("Boot sound", "Play the ASUS startup sound when the laptop powers on.", "", false, false),
        "dgpu_disable" => m(
            "Turn off the dedicated GPU",
            "Use only the integrated GPU: cooler and longer battery life, but no NVIDIA graphics. Applies after a restart.",
            "",
            true,
            false,
        ),
        "gpu_mux_mode" => m(
            "GPU mode",
            "Hybrid lets the integrated GPU drive the screen and saves battery. Dedicated sends everything through the NVIDIA \
             GPU for lower latency. Applies after a restart.",
            "",
            true,
            false,
        ),
        "panel_overdrive" => m("Panel overdrive", "Faster pixel response on the built-in display.", "", false, false),
        "charge_mode" => m(
            "Charge mode",
            "Charging behaviour chosen by the firmware. Leave it unless you know what a value does.",
            "",
            false,
            true,
        ),
        "nv_base_tgp" => m("GPU base power", "The NVIDIA GPU's guaranteed power.", " W", false, true),
        "nv_dynamic_boost" => {
            m("GPU dynamic boost", "Extra power the GPU may borrow from the CPU when it needs it.", " W", false, true)
        }
        "nv_temp_target" => m("GPU temperature target", "The GPU slows down to stay under this temperature.", " °C", false, true),
        "nv_tgp" => m("GPU power limit", "Total power the NVIDIA GPU may use.", " W", false, true),
        "ppt_pl1_spl" => m("CPU sustained power limit", "Power the CPU may use for long workloads (PL1).", " W", false, true),
        "ppt_pl2_sppt" => m("CPU boost power limit", "Power the CPU may use in short bursts (PL2).", " W", false, true),
        "ppt_pl3_fppt" => {
            m("CPU fast boost power limit", "Power the CPU may use for the shortest bursts (PL3).", " W", false, true)
        }
        "ppt_apu_sppt" => {
            m("APU sustained power limit", "Power the whole processor may use for long workloads.", " W", false, true)
        }
        "ppt_platform_sppt" => {
            m("Platform sustained power limit", "Power the whole system may use for long workloads.", " W", false, true)
        }
        _ => m("", "", "", false, true),
    }
}

fn title_for(name: &str, m: &Meta) -> String {
    if !m.title.is_empty() {
        return m.title.to_string();
    }
    let mut s = name.replace('_', " ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    s
}

fn firmware(page: &Page) {
    let attrs = asus::armoury();
    if attrs.is_empty() {
        return;
    }
    let (basic, advanced): (Vec<&Attribute>, Vec<&Attribute>) = attrs.iter().partition(|a| !meta(&a.name).advanced);

    if !basic.is_empty() {
        let g = page.group("Firmware");
        for a in basic {
            g.add(&attribute_row(a));
        }
    }
    if !advanced.is_empty() {
        let g = page.group("Power limits (advanced)");
        g.note(
            "Higher limits make the laptop hotter and louder and drain the battery faster. Ranges come from the firmware, \
             and each value has a reset to its default.",
        );
        for a in advanced {
            g.add(&attribute_row(a));
        }
    }
}

fn attribute_row(a: &Attribute) -> gtk::Box {
    let m = meta(&a.name);
    let title = title_for(&a.name, &m);
    let desc = m.desc.to_string();
    let name = a.name.clone();
    let restart = m.restart;
    let set = move |value: i64| {
        asus::armoury_set(&name, value);
        if restart {
            window::toast("Restart to apply this change");
        }
    };
    let is_temp = m.unit == " °C";
    match &a.kind {
        Kind::Fixed(v) => {
            let text = if is_temp {
                format!("{}\u{a0}{}", units::from_celsius(*v as f64).round() as i64, units::symbol())
            } else {
                format!("{v}{}", m.unit.replace(' ', "\u{a0}"))
            };
            let (r, _) = widgets::info_row(&title, &text);
            if !desc.is_empty() {
                // Info rows have no description slot; keep it as a tooltip.
                r.set_tooltip_text(Some(&desc));
            }
            r
        }
        Kind::Choice { choices, current }
            if choices.len() == 2 && choices.contains(&0) && choices.contains(&1) && a.name != "gpu_mux_mode" =>
        {
            let (r, _) = widgets::switch_row(&title, &desc, *current == 1, move |on| set(if on { 1 } else { 0 }));
            r
        }
        Kind::Choice { choices, current } => {
            let options: Vec<(String, String)> = choices
                .iter()
                .map(|c| {
                    let label = match (a.name.as_str(), c) {
                        ("gpu_mux_mode", 0) => "Dedicated GPU (MUX)".to_string(),
                        ("gpu_mux_mode", 1) => "Hybrid".to_string(),
                        (_, n) => format!("Mode {n}"),
                    };
                    (c.to_string(), label)
                })
                .collect();
            let (r, _) = widgets::choice_row(&title, &desc, options, &current.to_string(), move |v| {
                if let Ok(n) = v.parse::<i64>() {
                    set(n);
                }
            });
            r
        }
        Kind::Range { min, max, current, default } => {
            // Temperatures are shown in the chosen unit; the firmware always gets °C.
            let unit_text = if is_temp { format!("\u{a0}{}", units::symbol()) } else { m.unit.to_string() };
            let show = move |c: i64| if is_temp { units::from_celsius(c as f64) } else { c as f64 };
            let step = if is_temp {
                units::step()
            } else if *max - *min > 60 {
                5.0
            } else {
                1.0
            };
            let debounce = Debounce::default();
            let set = Rc::new(set);
            let (r, s) = {
                let set = set.clone();
                widgets::slider_row(&title, &desc, (show(*min), show(*max), step), show(*current), 0, &unit_text, move |v| {
                    let set = set.clone();
                    let value = if is_temp { units::to_celsius(v) } else { v.round() as i64 };
                    debounce.call(800, move || set(value));
                })
            };
            if let Some(d) = *default {
                let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
                reset.add_css_class("reset-button");
                reset.add_css_class("flat");
                reset.set_valign(gtk::Align::Center);
                reset.set_tooltip_text(Some(&format!("Reset to the firmware default ({}{})", show(d).round() as i64, unit_text)));
                let scale = s.scale.clone();
                reset.connect_clicked(move |_| scale.set_value(show(d)));
                r.insert_child_after(&reset, r.first_child().as_ref());
            }
            r
        }
    }
}

// ----- Screenpad -----

fn screenpad(page: &Page) {
    let g = page.group("Screenpad");
    let get = |prop: &str| asus::busctl_get("/xyz/ljones", "xyz.ljones.Backlight", prop);
    let brightness = get("ScreenpadBrightness").and_then(|v| v.parse::<f64>().ok()).unwrap_or(50.0);
    let debounce = Debounce::default();
    let (r, _) = widgets::slider_row("Brightness", "", (0.0, 100.0, 1.0), brightness.clamp(0.0, 100.0), 0, "%", move |v| {
        debounce.call(400, move || {
            asus::apply(vec!["backlight".into(), "--screenpad-brightness".into(), (v.round() as i64).to_string()])
        });
    });
    g.add(&r);
    let sync = get("ScreenpadSyncWithPrimary").is_some_and(|v| v == "true");
    let (r, _) = widgets::switch_row("Match the main display's brightness", "", sync, |on| {
        asus::apply(vec!["backlight".into(), "--sync-screenpad-brightness".into(), on.to_string()]);
    });
    g.add(&r);
    let gamma = get("ScreenpadGamma").and_then(|v| v.trim_matches('"').parse::<f64>().ok()).unwrap_or(1.0);
    let debounce = Debounce::default();
    let (r, _) = widgets::slider_row("Gamma", "1.0 is linear.", (0.5, 2.2, 0.1), gamma.clamp(0.5, 2.2), 1, "", move |v| {
        debounce.call(400, move || asus::apply(vec!["backlight".into(), "--screenpad-gamma".into(), format!("{v:.1}")]));
    });
    g.add(&r);
}
