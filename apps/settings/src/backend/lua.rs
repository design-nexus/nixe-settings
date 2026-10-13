//! Turning JSON-ish values into Lua literals.

use serde_json::Value;
use std::collections::BTreeMap;

pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn ident_or_key(key: &str) -> String {
    let ok = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok { key.to_string() } else { format!("[{}]", quote(key)) }
}

pub fn value(v: &Value) -> String {
    match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|_| n.is_f64()) {
                // Keep floats readable and never in exponent form.
                let s = format!("{f:.4}");
                let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
                if s.is_empty() || s == "-" { "0".into() } else { s }
            } else {
                n.to_string()
            }
        }
        Value::String(s) => quote(s),
        Value::Array(items) => format!("{{ {} }}", items.iter().map(value).collect::<Vec<_>>().join(", ")),
        Value::Object(map) => {
            let parts: Vec<String> = map.iter().map(|(k, v)| format!("{} = {}", ident_or_key(k), value(v))).collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

enum Node {
    Leaf(Value),
    Branch(BTreeMap<String, Node>),
}

/// `{"input.touchpad.natural_scroll": true, "input.sensitivity": 0.2}` becomes
/// `{ input = { sensitivity = 0.2, touchpad = { natural_scroll = true } } }`.
pub fn nested_table(options: &BTreeMap<String, Value>) -> String {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    for (key, v) in options {
        let parts: Vec<&str> = key.split('.').collect();
        let mut level = &mut root;
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                level.insert(part.to_string(), Node::Leaf(v.clone()));
            } else {
                let entry = level.entry(part.to_string()).or_insert_with(|| Node::Branch(BTreeMap::new()));
                if let Node::Leaf(_) = entry {
                    *entry = Node::Branch(BTreeMap::new());
                }
                let Node::Branch(next) = entry else { unreachable!() };
                level = next;
            }
        }
    }
    fn render(map: &BTreeMap<String, Node>, depth: usize) -> String {
        let pad = "  ".repeat(depth + 1);
        let close = "  ".repeat(depth);
        let mut s = String::from("{\n");
        for (k, node) in map {
            let body = match node {
                Node::Leaf(v) => value(v),
                Node::Branch(b) => render(b, depth + 1),
            };
            s.push_str(&format!("{pad}{} = {body},\n", ident_or_key(k)));
        }
        s.push_str(&close);
        s.push('}');
        s
    }
    render(&root, 0)
}

/// A one-line version for `hyprctl eval`.
pub fn nested_table_inline(options: &BTreeMap<String, Value>) -> String {
    nested_table(options).split('\n').map(str::trim).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nests_dotted_keys() {
        let mut m = BTreeMap::new();
        m.insert("input.touchpad.natural_scroll".to_string(), json!(true));
        m.insert("input.sensitivity".to_string(), json!(0.25));
        m.insert("general.layout".to_string(), json!("dwindle"));
        let out = nested_table(&m);
        assert!(out.contains("touchpad = {"));
        assert!(out.contains("natural_scroll = true"));
        assert!(out.contains("sensitivity = 0.25"));
        assert!(out.contains("layout = \"dwindle\""));
    }

    #[test]
    fn quotes_safely() {
        assert_eq!(quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn floats_are_plain() {
        assert_eq!(value(&json!(1.0)), "1");
        assert_eq!(value(&json!(0.4)), "0.4");
        assert_eq!(value(&json!(-0.5)), "-0.5");
        assert_eq!(value(&json!(3)), "3");
    }

    #[test]
    fn odd_keys_are_bracketed() {
        assert_eq!(ident_or_key("col.active"), "[\"col.active\"]");
        assert_eq!(ident_or_key("gaps_in"), "gaps_in");
    }
}
