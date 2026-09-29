use crate::widgets::{self, Page};
use crate::{cmd, window};

fn installed(command: &str) -> bool {
    cmd::present(command)
}

fn default_row(
    title: &str,
    desc: &str,
    setter: &'static str,
    choices: &[(&str, &str, &str)], // (id, label, command that must exist)
) -> gtk::Box {
    let current = cmd::output(&[setter]).unwrap_or_default().trim().to_string();
    let options: Vec<(String, String)> = choices
        .iter()
        .filter(|(id, _, bin)| installed(bin) || *id == current)
        .map(|(id, label, _)| (id.to_string(), label.to_string()))
        .collect();
    let (r, _) = widgets::choice_row(title, desc, options, &current, move |id| {
        let label = id.clone();
        cmd::run_async(&[setter, &id], move |r| match r {
            Ok(_) => window::toast(&format!("Default set to {label}")),
            Err(e) => window::toast(&format!("{e}")),
        });
    });
    r
}

const FILE_MANAGERS: &[(&str, &str, &str)] = &[
    ("io.github.lgse.Strata.desktop", "Strata", "strata"),
    ("org.gnome.Nautilus.desktop", "Files (Nautilus)", "nautilus"),
    ("thunar.desktop", "Thunar", "thunar"),
    ("org.kde.dolphin.desktop", "Dolphin", "dolphin"),
    ("nemo.desktop", "Nemo", "nemo"),
    ("pcmanfm.desktop", "PCManFM", "pcmanfm"),
];

pub fn build(page: &Page) {
    let g = page.group("Defaults");
    g.add(&default_row(
        "Web browser",
        "Opens links from every app.",
        "omarchy-default-browser",
        &[
            ("chromium", "Chromium", "chromium"),
            ("chrome", "Google Chrome", "google-chrome-stable"),
            ("brave", "Brave", "brave"),
            ("brave-origin", "Brave Origin", "brave-origin"),
            ("edge", "Microsoft Edge", "microsoft-edge-stable"),
            ("firefox", "Firefox", "firefox"),
            ("zen", "Zen", "zen-browser"),
        ],
    ));
    g.add(&default_row(
        "Terminal",
        "Used by <tt>Super Enter</tt> and anything that opens a terminal.",
        "omarchy-default-terminal",
        &[
            ("alacritty", "Alacritty", "alacritty"),
            ("foot", "foot", "foot"),
            ("ghostty", "Ghostty", "ghostty"),
            ("kitty", "kitty", "kitty"),
        ],
    ));
    g.add(&default_row(
        "Text editor",
        "Opens config files, including this app's Open config buttons.",
        "omarchy-default-editor",
        &[
            ("nvim", "Neovim", "nvim"),
            ("code", "VS Code", "code"),
            ("cursor", "Cursor", "cursor"),
            ("zed", "Zed", "zeditor"),
            ("sublime_text", "Sublime Text", "subl"),
            ("helix", "Helix", "helix"),
            ("vim", "Vim", "vim"),
            ("emacs", "Emacs", "emacs"),
        ],
    ));

    let current = cmd::output(&["xdg-mime", "query", "default", "inode/directory"]).unwrap_or_default();
    let options: Vec<(String, String)> = FILE_MANAGERS
        .iter()
        .filter(|(id, _, bin)| cmd::present(bin) || *id == current)
        .map(|(id, l, _)| (id.to_string(), l.to_string()))
        .collect();
    let (r, _) = widgets::choice_row("File manager", "Opens folders from every app.", options, &current, |id| {
        cmd::run_async(&["xdg-mime", "default", &id, "inode/directory"], |r| {
            if let Err(e) = r {
                window::toast(&format!("{e}"));
            }
        });
    });
    widgets::keywords("folders files strata nautilus");
    g.add(&r);
}
