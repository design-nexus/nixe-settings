# Settings Extensions

Device support for [Omarchy Settings](https://github.com/design-nexus/omarchy-settings),
installed from **Settings → Extensions** or the command line:

```sh
settings --ext install asus
```

Each extension adds its own pages under **Devices**, drawn with the same controls as
the rest of Settings. Pages only appear when matching hardware is found.

| Extension | Works with | Needs |
| --- | --- | --- |
| `asus` | ASUS ROG, TUF and Zephyrus laptops: performance profiles, fan curves, charge limit, firmware settings, Aura lighting, Slash, AniMe Matrix | `asusctl` |
| `logitech` | Logitech mice and keyboards: battery, pointer speed, scrolling, buttons, backlight, Easy-Switch, lighting | — |
| `headset` | SteelSeries Arctis, HyperX Cloud, Corsair Void, Logitech G and Roccat headsets: battery, sidetone, lights, EQ presets, auto power-off | `headsetcontrol` |
| `webcam` | USB webcams (OBSBOT, Logitech, Elgato…): live preview, microphone level, mic options (never idle, default, unmuted), zoom, pan/tilt, focus, exposure, white balance; keep OBSBOT Tiny 3 cameras awake (vendor commands from [obsbot-tiny3-linux](https://github.com/joshualambert/obsbot-tiny3-linux)) | `v4l2-ctl`, `ffmpeg` |
| `corsair` | Corsair Galleon 100 SD: device details, access for Corsair Web Hub, a read-only probe report (native lighting next) | — |
| `streamdeck` | The Galleon 100 SD's built-in Stream Deck: keys, profiles, pages, brightness, dials, via [galleon-deck](https://github.com/NLMP-DDHS/galleon-deck) | `python3`, `git` |

`index.json` is the catalog Settings reads.

## Command line

```
settings --ext list                  catalog, and what's installed
settings --ext install ID            from the catalog
settings --ext install URL [FOLDER]  any git repository (FOLDER: the extension's folder in it)
settings --ext update [ID]
settings --ext disable ID            turn off without removing (pages hidden)
settings --ext enable ID
settings --ext remove ID
```

Extensions live in `~/.local/share/settings/extensions/<id>`. Each one can be
turned off on the Extensions page, which hides its pages without removing it.

## Writing an extension

An extension is a folder with an `extension.toml` and a helper program, in any
language. Settings runs the helper, reads JSON from its stdout, and draws the
page itself.

```toml
id = "headset"                 # letters, digits, - and _
name = "Headsets"
version = "1.0.0"
description = "…"
exec = "./settings-headset"    # relative to this folder, or a program on PATH
args = []                      # optional, put before every command (e.g. ["--extension"])
needs = ["headsetcontrol"]     # pages stay hidden until these are on PATH
needs_hint = "Needs <tt>headsetcontrol</tt>: <tt>yay -S headsetcontrol</tt>."
install = "install.sh"         # optional, run after install and update (build or fetch the helper)
hooks = ["theme-changed"]      # optional
files = ["~/.config/thing.toml"]  # optional, offered by the page's "Open config" button
```

### Commands

| Command | Prints |
| --- | --- |
| `pages` | `[{id, title, icon, description, keywords}]` — one entry per page; `[]` when no hardware is found |
| `describe PAGE` | the page (below) |
| `set PAGE KEY VALUE` | nothing, or `{"toast": "…", "refresh": true, "reload": true}` (`reload`: ask for the pages again, e.g. after devices appear) |
| `theme-changed` | nothing; run after the Omarchy theme changes (with `hooks = ["theme-changed"]`) |

A non-zero exit shows stderr as a message. Settings waits 15 s for `describe` and 30 s for `set`; start anything longer (package installs, builds) in the background or a terminal, and report progress from `describe` with `poll`. Settings sets `SETTINGS_KINDS` (the row kinds it can draw; send only those), `SETTINGS_EXTENSION_DIR`
(the extension's folder), `SETTINGS_EXTENSION_STATE` (a folder for small things
to remember) and `SETTINGS_TEMP_UNIT` (`C` or `F`).

### Pages

```json
{
  "subtitle": "<b>Model</b> · details",
  "banners": [{ "text": "…", "warning": false }],
  "poll": 30,
  "groups": [
    { "title": "Battery", "note": "…", "rows": [ { "kind": "slider", "key": "limit", "title": "Charge limit", "value": 80, "min": 20, "max": 100, "unit": "%" } ] }
  ]
}
```

`poll` asks for the page again every so many seconds while it's showing.
Every row takes `key`, `title`, `desc`, `keywords`, `tag`, `tooltip`, and
`refresh` (ask for the page again after it changes).

| `kind` | Fields | `set` value |
| --- | --- | --- |
| `info` | `value` | — |
| `switch` | `value` (bool), `rows` (shown while on) | `true` / `false` |
| `slider` | `value`, `min`, `max`, `step`, `digits`, `unit`, `marks`, `reset`, `on_release`, `temperature` (values in °C, shown in the user's unit) | the number |
| `choice`, `segmented` | `value`, `options` (`[[id, label], …]`) | the id |
| `buttons` | `options` | the id of the button pressed |
| `entry` | `value`, `placeholder` | the text |
| `button` | `label`, `confirm` (needs a second click), `destructive` | empty |
| `colour` | `value` (`#rrggbb`), `theme` (`{label, colour}` swatch), `compact` | `#rrggbb` |
| `chips` | `labels`, `value` (bools) | `true,false,…` |
| `camera` | `device` (`/dev/videoN`): a live preview, started with a button | — |
| `meter` | `source` (a PipeWire node name; empty for the default mic): a live level graph | — |
| `curve` | `series` (`[{id, label, points: [[°C, %], …]}]`), `presets`, `hint` | key `KEY/SERIES`, value `°C:%,…` |
| `disclosure` | `rows` (shown when opened) | — |

## License

MIT
