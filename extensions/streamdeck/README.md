# Stream Deck

The Corsair Galleon 100 SD's built-in Stream Deck (USB `1b1c:2b18`) in Settings: its
12 screen keys, top screen and two dials.

The deck is driven by [galleon-deck](https://github.com/NLMP-DDHS/galleon-deck)
(MIT), which runs as a user service (`galleon-deck.service`) and reads TOML files in
`~/.config/galleon-deck`. This extension fetches galleon-deck at a fixed commit and,
from the Stream Deck page:

- installs its packages and adds its device rule and the `uinput` module in a terminal
  (`sudo`, so you type your password there and can answer pacman), and runs its
  installer in the background; the page shows progress, and the last lines of output
  if a step fails;
- turns the service on or off, restarts it, or opens galleon-deck's own app (icons,
  images, themes, window rules);
- edits brightness, the start profile, clock format, animations and the volume
  step, and each profile's theme, start page and keys (label and action: a key, a
  shortcut, a command, media, a page or a profile).

Config is written with tomlkit, so comments in the files are kept; galleon-deck
picks up changes within a second. Profiles using the **Wallpaper** theme are
regenerated from the Omarchy wallpaper whenever the theme changes.
