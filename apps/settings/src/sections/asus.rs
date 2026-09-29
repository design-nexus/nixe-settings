use crate::widgets::{self, Page, opts};
use crate::{cmd, window};
use gtk::prelude::*;

pub fn available() -> bool {
    cmd::present("asusctl")
}

fn run(args: Vec<String>) {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    cmd::run_async(&refs, |r| {
        if let Err(e) = r {
            window::toast(&format!("{e}"));
        }
    });
}

fn after_colon(text: &str) -> String {
    text.rsplit(':').next().unwrap_or("").trim().to_string()
}

pub fn build(page: &Page) {
    let profiles: Vec<(String, String)> = cmd::output(&["asusctl", "profile", "list"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|l| (l.clone(), l))
        .collect();
    let info = cmd::output(&["asusctl", "profile", "get"]).unwrap_or_default();
    let find = |prefix: &str| {
        info.lines()
            .find(|l| l.trim_start().starts_with(prefix))
            .map(|l| l.trim_start()[prefix.len()..].trim().trim_start_matches(':').trim().to_string())
            .unwrap_or_default()
    };

    let g = page.group("Fan & performance profile");
    let (r, _) = widgets::choice_row("Right now", "", profiles.clone(), &find("Active profile"), |p| {
        run(vec!["asusctl".into(), "profile".into(), "set".into(), p]);
    });
    g.add(&r);
    let (r, _) = widgets::choice_row("When plugged in", "", profiles.clone(), &find("AC profile"), |p| {
        run(vec!["asusctl".into(), "profile".into(), "set".into(), "-a".into(), p]);
    });
    g.add(&r);
    let (r, _) = widgets::choice_row("On battery", "", profiles, &find("Battery profile"), |p| {
        run(vec!["asusctl".into(), "profile".into(), "set".into(), "-b".into(), p]);
    });
    g.add(&r);

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
        run(vec!["asusctl".into(), "battery".into(), "limit".into(), (scale.value().round() as i64).to_string()]);
    });
    s.scale.add_controller(click);
    g.add(&r);
    let (r, _) =
        widgets::button_row("Charge to full once", "Ignore the limit until the next full charge.", "Charge to 100%", |_| {
            run(vec!["asusctl".into(), "battery".into(), "oneshot".into()]);
        });
    g.add(&r);

    let g = page.group("Keyboard lighting");
    let current = cmd::output(&["asusctl", "leds", "get"]).map(|s| after_colon(&s).to_lowercase()).unwrap_or_default();
    let (r, _) = widgets::choice_row(
        "Brightness",
        "",
        opts(&[("off", "Off"), ("low", "Low"), ("med", "Medium"), ("high", "High")]),
        &current,
        |b| run(vec!["asusctl".into(), "leds".into(), "set".into(), b]),
    );
    g.add(&r);

    let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    color.set_rgba(&gtk::gdk::RGBA::parse("#7aa2f7").unwrap_or(gtk::gdk::RGBA::WHITE));
    let effect_opts = opts(&[
        ("static", "Solid colour"),
        ("breathe", "Breathing"),
        ("pulse", "Pulse"),
        ("rainbow-cycle", "Rainbow cycle"),
        ("rainbow-wave", "Rainbow wave"),
    ]);
    let effect = widgets::dropdown(&effect_opts, "static");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    {
        let color = color.clone();
        let effect = effect.clone();
        apply.connect_clicked(move |_| {
            let c = color.rgba();
            let hex = format!("{:02x}{:02x}{:02x}", (c.red() * 255.0) as u8, (c.green() * 255.0) as u8, (c.blue() * 255.0) as u8);
            let id = effect_opts.get(effect.selected() as usize).map(|e| e.0.clone()).unwrap_or("static".into());
            let mut args: Vec<String> = vec!["asusctl".into(), "aura".into(), "effect".into(), id.clone()];
            if !id.starts_with("rainbow") {
                args.push("-c".into());
                args.push(hex);
            }
            run(args);
        });
    }
    let bx = widgets::hbox(8);
    bx.append(&effect);
    bx.append(&color);
    bx.append(&apply);
    g.add(&widgets::row("Effect", "Some effects ignore the colour.", Some(bx.upcast_ref())));
    let (r, _) = widgets::button_row("Match the Omarchy theme", "Use the current theme's accent colour.", "Match", |_| {
        cmd::spawn(&["omarchy-theme-set-keyboard-asus-rog"]);
    });
    widgets::keywords("aura rgb colour color");
    g.add(&r);
}
