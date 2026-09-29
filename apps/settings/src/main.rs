//! Settings — a control panel for Omarchy, styled after the Strata file manager.

mod backend;
mod cmd;
mod paths;
mod prefs;
mod sections;
mod theme;
mod widgets;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.design_nexus.Settings";

const USAGE: &str = "Usage: settings [--section ID] [--volume raise|lower|+N|-N] [--eq on|off|toggle|status] [--apply]\n\
\n\
  --section ID   open (or switch the open window) to a page, e.g. audio, trackpad\n\
  --volume STEP  change the output volume, allowing past 100% up to the maximum set in Sound\n\
  --eq ACTION    turn the preamp/equalizer on or off, or print whether it's running\n\
  --apply        rewrite ~/.config/hypr/settings.lua from saved state and exit\n";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Headless commands never touch GTK.
    if let Some(i) = args.iter().position(|a| a == "--volume") {
        let step = args.get(i + 1).map(String::as_str).unwrap_or("raise");
        let max = backend::state::State::load(&paths::state_file()).max_volume.unwrap_or(100);
        return match backend::audio::step_volume(step, max) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("settings: {e:#}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if let Some(i) = args.iter().position(|a| a == "--eq") {
        use backend::audio;
        let action = args.get(i + 1).map(String::as_str).unwrap_or("status");
        let mut eq = audio::load();
        let on = match action {
            "on" => true,
            "off" => false,
            "toggle" => !audio::running(),
            _ => {
                println!("{}", if audio::running() { "on" } else { "off" });
                return glib::ExitCode::SUCCESS;
            }
        };
        eq.enabled = on;
        let result = if on { audio::install_and_start(&eq) } else { audio::save(&eq).and_then(|_| audio::stop(&eq)) };
        return match result {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("settings: {e:#}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|a| a == "--apply") {
        let state = backend::state::State::load(&paths::state_file());
        return match backend::hypr::write(&state) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("settings: {e:#}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }

    let app = gtk::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    app.connect_command_line(|app, cl| {
        let argv: Vec<String> = cl.arguments().iter().map(|a| a.to_string_lossy().to_string()).collect();
        let section = argv.iter().position(|a| a == "--section").and_then(|i| argv.get(i + 1)).cloned();
        window::present(app, section.as_deref());
        glib::ExitCode::SUCCESS
    });
    app.run()
}
