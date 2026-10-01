//! ASUS laptop controls for Settings: performance, battery, fan curves, firmware
//! settings and Aura lighting, all through `asusctl` / asusd.
//!
//! settings-asus pages | describe PAGE | set PAGE KEY VALUE | theme-changed

mod asus;
mod cmd;
mod lighting;
mod machine;
mod paths;

use anyhow::{Result, bail};
use serde_json::{Value, json};

fn pages() -> Value {
    if !asus::available() {
        return json!([]);
    }
    let mut out = vec![json!({
        "id": "asus",
        "title": "ASUS",
        "icon": "input-gaming-symbolic",
        "description": "Performance profile, fan curves, battery limit and firmware settings via asusctl.",
        "keywords": "asus rog zephyrus fan curve profile performance charge limit battery gpu mux dgpu overdrive power limit tgp asusctl screenpad",
    })];
    if asus::lighting_available() {
        out.push(json!({
            "id": "aura",
            "title": "Aura Lighting",
            "icon": "keyboard-brightness-symbolic",
            "description": "Keyboard backlight and effects, the Slash lightbar and other ASUS lights.",
            "keywords": "aura rgb keyboard backlight lighting effects slash lightbar led anime matrix rainbow colour",
        }));
    }
    Value::Array(out)
}

fn run(args: &[String]) -> Result<Option<Value>> {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match arg(0) {
        "pages" => Ok(Some(pages())),
        "describe" => match arg(1) {
            "asus" => Ok(Some(machine::describe())),
            "aura" => Ok(Some(lighting::describe())),
            p => bail!("no page called {p}"),
        },
        "set" => match arg(1) {
            "asus" => machine::set(arg(2), arg(3)),
            "aura" => lighting::set(arg(2), arg(3)),
            p => bail!("no page called {p}"),
        },
        "theme-changed" => asus::sync_after_theme().map(|_| None),
        _ => bail!("usage: settings-asus pages | describe PAGE | set PAGE KEY VALUE | theme-changed"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(Some(v)) => println!("{v}"),
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
}

/// A reply asking Settings to show a message.
pub fn toast(text: &str) -> Option<Value> {
    Some(json!({ "toast": text }))
}

/// A reply asking Settings to ask for the page again.
pub fn refresh() -> Option<Value> {
    Some(json!({ "refresh": true }))
}

pub fn bool_arg(v: &str) -> Result<bool> {
    match v {
        "true" | "1" | "on" => Ok(true),
        "false" | "0" | "off" => Ok(false),
        _ => bail!("expected true or false, got \"{v}\""),
    }
}

pub fn num_arg(v: &str) -> Result<i64> {
    match v.parse::<f64>() {
        Ok(n) => Ok(n.round() as i64),
        Err(_) => bail!("expected a number, got \"{v}\""),
    }
}

/// `(id, label)` pairs as JSON options.
pub fn options<A: AsRef<str>, B: AsRef<str>>(pairs: &[(A, B)]) -> Value {
    Value::Array(pairs.iter().map(|(a, b)| json!([a.as_ref(), b.as_ref()])).collect())
}
