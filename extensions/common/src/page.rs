//! Pages and rows, as the JSON Settings draws (see the README's row table).

use serde_json::{Value, json};

/// A page entry for `pages`.
pub fn entry(id: &str, title: &str, icon: &str, description: &str, keywords: &str) -> Value {
    json!({ "id": id, "title": title, "icon": icon, "description": description, "keywords": keywords })
}

/// A whole page: a subtitle (markup), banners, and groups.
#[derive(Default)]
pub struct Page {
    subtitle: String,
    banners: Vec<Value>,
    groups: Vec<Value>,
    poll: Option<u32>,
}

impl Page {
    pub fn new(subtitle: &str) -> Page {
        Page { subtitle: subtitle.to_string(), ..Default::default() }
    }

    pub fn banner(mut self, text: &str, warning: bool) -> Page {
        self.banners.push(json!({ "text": text, "warning": warning }));
        self
    }

    /// Draw the page again every `secs` while it's showing.
    pub fn poll(mut self, secs: u32) -> Page {
        self.poll = Some(secs);
        self
    }

    pub fn group(mut self, title: &str, note: &str, rows: Vec<Value>) -> Page {
        if !rows.is_empty() {
            self.groups.push(json!({ "title": title, "note": note, "rows": rows }));
        }
        self
    }

    pub fn build(self) -> Value {
        let mut v = json!({ "subtitle": self.subtitle, "banners": self.banners, "groups": self.groups });
        if let Some(p) = self.poll {
            v["poll"] = json!(p);
        }
        v
    }
}

/// Add a description to a row.
pub fn desc(mut row: Value, text: &str) -> Value {
    if !text.is_empty() {
        row["desc"] = json!(text);
    }
    row
}

/// Ask for the page again after this row changes.
pub fn refreshes(mut row: Value) -> Value {
    row["refresh"] = json!(true);
    row
}

pub fn info(key: &str, title: &str, value: &str) -> Value {
    json!({ "kind": "info", "key": key, "title": title, "value": value })
}

pub fn switch(key: &str, title: &str, value: bool) -> Value {
    json!({ "kind": "switch", "key": key, "title": title, "value": value })
}

/// A slider; `on_release` sends the value only once it's let go (for slow hardware).
pub fn slider(key: &str, title: &str, value: f64, (min, max, step): (f64, f64, f64), unit: &str) -> Value {
    json!({ "kind": "slider", "key": key, "title": title, "value": value, "min": min, "max": max, "step": step,
            "digits": if step.fract() == 0.0 { 0 } else { 2 }, "unit": unit, "on_release": true })
}

fn options(opts: &[(&str, &str)]) -> Value {
    Value::Array(opts.iter().map(|(id, label)| json!([id, label])).collect())
}

/// Options built at run time.
pub fn owned_options(opts: &[(String, String)]) -> Value {
    Value::Array(opts.iter().map(|(id, label)| json!([id, label])).collect())
}

pub fn choice(key: &str, title: &str, value: &str, opts: &[(&str, &str)]) -> Value {
    json!({ "kind": "choice", "key": key, "title": title, "value": value, "options": options(opts) })
}

pub fn choice_owned(key: &str, title: &str, value: &str, opts: &[(String, String)]) -> Value {
    json!({ "kind": "choice", "key": key, "title": title, "value": value, "options": owned_options(opts) })
}

pub fn segmented(key: &str, title: &str, value: &str, opts: &[(&str, &str)]) -> Value {
    json!({ "kind": "segmented", "key": key, "title": title, "value": value, "options": options(opts) })
}

pub fn buttons(key: &str, title: &str, opts: &[(&str, &str)]) -> Value {
    json!({ "kind": "buttons", "key": key, "title": title, "options": options(opts) })
}

pub fn button(key: &str, title: &str, label: &str) -> Value {
    json!({ "kind": "button", "key": key, "title": title, "label": label })
}

pub fn entry_row(key: &str, title: &str, value: &str, placeholder: &str) -> Value {
    json!({ "kind": "entry", "key": key, "title": title, "value": value, "placeholder": placeholder })
}

pub fn colour(key: &str, title: &str, value: &str) -> Value {
    json!({ "kind": "colour", "key": key, "title": title, "value": value })
}

/// A colour row offering the Omarchy theme's accent as a swatch.
pub fn colour_with_theme(key: &str, title: &str, value: &str, theme: Option<&str>) -> Value {
    let mut v = colour(key, title, value);
    if let Some(c) = theme {
        v["theme"] = json!({ "label": "Theme colour", "colour": c });
    }
    v
}

pub fn disclosure(title: &str, rows: Vec<Value>) -> Value {
    json!({ "kind": "disclosure", "title": title, "rows": rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_pages() {
        let p = Page::new("<b>X</b>")
            .banner("hi", true)
            .group("Empty", "", vec![])
            .group("G", "n", vec![switch("a", "A", true), refreshes(slider("b", "B", 5.0, (0.0, 10.0, 1.0), " %"))])
            .poll(5)
            .build();
        assert_eq!(p["groups"].as_array().unwrap().len(), 1);
        assert_eq!(p["groups"][0]["rows"][1]["refresh"], true);
        assert_eq!(p["groups"][0]["rows"][1]["digits"], 0);
        assert_eq!(p["poll"], 5);
        assert_eq!(choice("c", "C", "x", &[("x", "X")])["options"][0][1], "X");
    }
}
