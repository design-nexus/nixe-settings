use crate::widgets::{Page, hypr_choice, hypr_choice_typed, hypr_slider, hypr_switch, opts};

pub fn build(page: &Page) {
    let g = page.group("Tiling");
    g.add(&hypr_choice(
        "general.layout",
        "Layout",
        "How new windows share the screen. <b>Scrolling</b> lays windows out in columns you scroll through.",
        opts(&[("dwindle", "Dwindle (split in halves)"), ("master", "Master & stack"), ("scrolling", "Scrolling columns")]),
    ));
    g.add(&hypr_switch("dwindle.preserve_split", "Keep split direction", "Don't re-orient splits when windows move."));
    g.add(&hypr_switch("dwindle.smart_split", "Split toward the cursor", "Place the new window on the side the mouse is on."));
    g.add(&hypr_choice_typed(
        "master.new_status",
        "New windows in Master layout",
        "",
        opts(&[("slave", "Join the stack"), ("master", "Become the master"), ("inherit", "Same as focused")]),
        false,
    ));
    g.add(&hypr_slider(
        "master.mfact",
        "Master width",
        "Share of the screen the master window gets.",
        (0.2, 0.8, 0.01),
        2,
        "",
        false,
    ));
    g.add(&hypr_slider(
        "scrolling.column_width",
        "Scrolling column width",
        "Default column width in Scrolling layout.",
        (0.2, 1.0, 0.05),
        2,
        "",
        false,
    ));

    let g = page.group("Focus");
    g.add(&hypr_choice_typed(
        "input.follow_mouse",
        "Focus follows mouse",
        "",
        opts(&[
            ("1", "Always — focus the window under the pointer"),
            ("2", "Only when clicking, keyboard follows"),
            ("0", "Never — click to focus"),
            ("3", "Fully separate"),
        ]),
        true,
    ));
    g.add(&hypr_switch(
        "misc.focus_on_activate",
        "Focus apps that ask for it",
        "Let an app jump to the front when it requests attention.",
    ));
    g.add(&hypr_switch(
        "cursor.no_warps",
        "Don't move the cursor on focus",
        "Keep the pointer still when focus changes by keyboard.",
    ));

    let g = page.group("Workspaces");
    g.add(&hypr_switch(
        "binds.workspace_back_and_forth",
        "Press again to go back",
        "Choosing the workspace you're on returns to the previous one.",
    ));
    g.add(&hypr_switch(
        "misc.close_special_on_empty",
        "Close empty scratchpad",
        "Hide the scratchpad when its last window closes.",
    ));
    g.add(&hypr_switch(
        "misc.enable_swallow",
        "Terminal swallowing",
        "A GUI app started from a terminal takes the terminal's place.",
    ));
    g.add(&hypr_switch("misc.middle_click_paste", "Middle-click paste", "Paste the primary selection with the middle button."));
}
