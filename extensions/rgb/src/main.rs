//! RGB lighting for every device OpenRGB sees: motherboards, memory, graphics
//! cards, coolers, LED strips and peripherals, from any brand.
//!
//! Pages: `all` (one colour everywhere, following the theme, profiles), and
//! one per device.

use anyhow::{Context, Result, bail};
use nyx_ext::openrgb::{self, Client, Rgb};
use nyx_ext::page::{self, Page, desc};
use nyx_ext::{Extension, lighting, sys};
use serde_json::{Value, json};

const ALL: &str = "all";
const DEVICE: &str = "dev-";

struct Rgbx;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

impl Extension for Rgbx {
    fn pages(&self) -> Value {
        let (installed, _) = lighting::status();
        if !installed && !sys::present("openrgb") {
            // Nothing to show until OpenRGB is installed: one page to set it up.
            return json!([page::entry(ALL, "RGB lighting", "weather-clear-symbolic", "Lighting for RGB devices of every brand, through OpenRGB.", "rgb lighting led openrgb colour")]);
        }
        let mut out = vec![page::entry(ALL, "RGB lighting", "weather-clear-symbolic", "Every RGB device at once: one colour, the theme colour, saved profiles.", "rgb lighting led openrgb colour profile all")];
        for d in lighting::devices() {
            out.push(page::entry(
                &format!("{DEVICE}{}", d.id()),
                &d.name,
                d.icon(),
                &format!("{} lighting.", d.kind_name()),
                &format!("rgb lighting led {} {} {}", d.kind_name().to_lowercase(), d.vendor.to_lowercase(), d.name.to_lowercase()),
            ));
        }
        Value::Array(out)
    }

    fn describe(&self, id: &str) -> Result<Value> {
        if id == ALL {
            let setup = lighting::setup_rows();
            if !setup.is_empty() {
                return Ok(Page::new("RGB lighting for motherboards, memory, graphics cards, fans and peripherals of every brand, through OpenRGB.")
                    .group("OpenRGB", "", setup)
                    .build());
            }
            let devices = lighting::devices();
            let list: Vec<Value> = devices.iter().map(|d| page::info(&d.id(), &d.name, d.kind_name())).collect();
            let theme = sys::theme_accent();
            let mut rows = vec![page::colour_with_theme("all/colour", "One colour everywhere", "#ffffff", theme.as_deref())];
            if theme.is_some() {
                rows.push(desc(page::button("all/theme", "Use the theme colour everywhere", "Apply"), "Each device's own page can follow the theme as it changes."));
            }
            rows.push(desc(page::button("all/off", "Turn every light off", "Off"), ""));
            let mut c = Client::connect()?;
            let profiles = c.profiles().unwrap_or_default();
            let mut prof = vec![page::entry_row("profile/save", "Save the lighting as a profile", "", "Profile name")];
            if !profiles.is_empty() {
                let opts: Vec<(String, String)> = profiles.iter().map(|p| (p.clone(), p.clone())).collect();
                prof.push(page::choice_owned("profile/load", "Load a profile", "", &opts));
            }
            return Ok(Page::new(&format!("<b>{}</b> devices", devices.len()))
                .group("Everything", "", rows)
                .group("Profiles", "Profiles are OpenRGB's: the effect and colours of every device.", prof)
                .group("Devices", "Each one has its own page for effects, brightness and speed.", list)
                .build());
        }
        let Some(dev) = id.strip_prefix(DEVICE) else { bail!("no such page") };
        let Some(d) = lighting::devices().into_iter().find(|d| d.id() == dev) else { bail!("OpenRGB doesn't see that device any more") };
        let mut info = vec![page::info("kind", "Kind", d.kind_name())];
        for (k, t, v) in [("vendor", "Maker", &d.vendor), ("version", "Firmware", &d.version), ("serial", "Serial", &d.serial), ("location", "Connected at", &d.location)] {
            if !v.is_empty() {
                info.push(page::info(k, t, v));
            }
        }
        if !d.zones.is_empty() {
            let zones: Vec<String> = d.zones.iter().map(|z| format!("{} ({} LEDs)", z.name, z.leds_count)).collect();
            info.push(page::info("zones", "Zones", &zones.join("\n")));
        }
        Ok(Page::new(&format!("<b>{}</b> · {}", esc(&d.name), d.kind_name()))
            .group("Lighting", "", lighting::rows(&d))
            .group("Device", "", info)
            .build())
    }

    fn set(&self, page: &str, key: &str, value: &str) -> Result<Option<Value>> {
        let device = page.strip_prefix(DEVICE).unwrap_or("");
        if let Some(reply) = lighting::set(device, key, value)? {
            return Ok(reply);
        }
        let mut c = Client::connect()?;
        match key {
            "all/colour" | "all/theme" | "all/off" => {
                let colour = match key {
                    "all/colour" => Rgb::parse(value).context("not a colour")?,
                    "all/theme" => sys::theme_accent().and_then(|t| Rgb::parse(&t)).context("the theme has no accent colour")?,
                    _ => Rgb(0, 0, 0),
                };
                let devices = c.devices()?;
                for d in &devices {
                    openrgb::set_colour(&mut c, d, colour)?;
                }
                Ok(nyx_ext::toast(&format!("{} devices set", devices.len())))
            }
            "profile/save" => {
                let name = value.trim();
                if name.is_empty() || name.contains('/') {
                    bail!("Give the profile a name");
                }
                c.save_profile(name)?;
                Ok(nyx_ext::refresh())
            }
            "profile/load" => {
                c.load_profile(value)?;
                Ok(nyx_ext::toast(&format!("Loaded {value}")))
            }
            _ => bail!("unknown setting {key}"),
        }
    }

    fn theme_changed(&self) -> Result<()> {
        lighting::theme_changed()
    }
}

fn main() {
    nyx_ext::run(&Rgbx);
}
