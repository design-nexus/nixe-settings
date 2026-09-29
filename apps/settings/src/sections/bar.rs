use crate::backend::shell;
use crate::widgets::{self, Page, opts};
use crate::{cmd, paths, window};
use gtk::prelude::*;

fn run(args: &[&str]) {
    let label = args.join(" ");
    cmd::run_async(args, move |r| {
        if let Err(e) = r {
            window::toast(&format!("{label}: {e}"));
        }
    });
}

fn shell_text_size() -> Option<f64> {
    let text = std::fs::read_to_string(paths::omarchy_config().join("shell.toml")).ok()?;
    let t: toml::Table = toml::from_str(&text).ok()?;
    t.get("font")?.get("base-size")?.as_integer().map(|n| n as f64)
}

fn set_shell_text_size(size: i64) -> anyhow::Result<()> {
    let path = paths::omarchy_config().join("shell.toml");
    let mut t: toml::Table = std::fs::read_to_string(&path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
    let font = t.entry("font").or_insert_with(|| toml::Value::Table(Default::default()));
    if let Some(f) = font.as_table_mut() {
        f.insert("base-size".into(), toml::Value::Integer(size));
    }
    cmd::atomic_write(&path, &toml::to_string(&t)?)
}

pub fn build(page: &Page) {
    let g = page.group("Bar");
    let position = shell::get(&["bar", "position"]).and_then(|v| v.as_str().map(String::from)).unwrap_or("top".into());
    let (r, _) = widgets::choice_row(
        "Position",
        "",
        opts(&[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right")]),
        &position,
        |p| run(&["omarchy-bar", "position", &p]),
    );
    g.add(&r);
    let transparent = shell::get(&["bar", "transparent"]).and_then(|v| v.as_bool()).unwrap_or(false);
    let (r, _) = widgets::switch_row("Transparent background", "Let the wallpaper show through the bar.", transparent, |on| {
        run(&["omarchy-bar", "transparent", if on { "true" } else { "false" }]);
    });
    g.add(&r);
    let (r, _) = widgets::button_row("Show or hide the bar", "Also on <tt>Super Shift Space</tt>.", "Toggle", |_| {
        run(&["omarchy-toggle-bar"])
    });
    g.add(&r);
    let (r, _) = widgets::button_row(
        "Widgets",
        "Rearrange what the bar shows from the Plugins page, or reset to Omarchy's layout.",
        "Reset layout",
        |_| run(&["omarchy-bar", "reset"]),
    );
    g.add(&r);

    let g = page.group("Shell text");
    let size = shell_text_size().unwrap_or(12.0);
    let (r, _) =
        widgets::slider_row("Text size", "Font size of the bar, menus and panels.", (9.0, 18.0, 1.0), size, 0, " pt", |v| {
            match set_shell_text_size(v.round() as i64) {
                Ok(()) => {}
                Err(e) => window::toast(&format!("Couldn't save text size: {e}")),
            }
        });
    widgets::keywords("font size bigger smaller scale");
    g.add(&r);

    let g = page.group("Notifications");
    let (r, _) = widgets::button_row(
        "Do not disturb",
        "Silence notifications until you turn it back on. Also <tt>Super Ctrl ,</tt>.",
        "Toggle",
        |_| run(&["omarchy-toggle-notification-silencing"]),
    );
    widgets::keywords("dnd silence mute notifications quiet");
    g.add(&r);
    let buttons = widgets::hbox(8);
    buttons.append(&widgets::command_button("History", &["omarchy-shell", "notifications", "showHistory"]));
    buttons.append(&widgets::command_button("Dismiss all", &["omarchy-shell", "notifications", "dismissAll"]));
    g.add(&widgets::row("Notification centre", "", Some(buttons.upcast_ref())));
}
