use crate::widgets::{Page, hypr_choice, hypr_entry, hypr_slider, hypr_switch, opts};

/// Map a few xkb layout codes to names for the description.
fn layouts_hint() -> String {
    "Comma-separated xkb codes, e.g. <tt>us</tt>, <tt>us,de</tt> or <tt>gb,fr</tt>. \
     Switch between several with the option below."
        .to_string()
}

pub fn build(page: &Page) {
    let g = page.group("Layout");
    g.add(&hypr_entry("input.kb_layout", "Keyboard layouts", &layouts_hint(), "us"));
    g.add(&hypr_entry(
        "input.kb_variant",
        "Variants",
        "One per layout, e.g. <tt>intl</tt> or <tt>,dvorak</tt>. Leave empty for the default.",
        "",
    ));

    let g = page.group("Special keys");
    let mut options = opts(&[
        ("compose:caps,shift:both_capslock_cancel", "Compose — Caps Lock is both Shifts (Omarchy)"),
        ("", "Caps Lock"),
        ("caps:escape", "Escape"),
        ("caps:escape_shifted_capslock", "Escape — Shift+Caps is Caps Lock"),
        ("ctrl:nocaps", "Control"),
        ("caps:backspace", "Backspace"),
        ("caps:super", "Super"),
        ("caps:none", "Nothing"),
        ("compose:caps,shift:both_capslock_cancel,grp:alts_toggle", "Compose, and Left+Right Alt switch layouts"),
        ("compose:ralt", "Caps Lock, and Right Alt is Compose"),
    ]);
    if let Some(current) = crate::backend::hypr::get_str("input.kb_options")
        && !options.iter().any(|(id, _)| *id == current)
    {
        options.push((current.clone(), format!("Custom: {current}")));
    }
    g.add(&hypr_choice(
        "input.kb_options",
        "Caps Lock key",
        "What Caps Lock does. Compose types characters like é with <tt>Caps ' e</tt>.",
        options,
    ));

    let g = page.group("Typing");
    g.add(&hypr_slider(
        "input.repeat_rate",
        "Repeat rate",
        "Characters per second while a key is held.",
        (10.0, 80.0, 1.0),
        0,
        "/s",
        true,
    ));
    g.add(&hypr_slider(
        "input.repeat_delay",
        "Repeat delay",
        "How long to hold a key before it repeats.",
        (150.0, 800.0, 10.0),
        0,
        " ms",
        true,
    ));
    g.add(&hypr_switch("input.numlock_by_default", "Num Lock on at login", ""));
}
