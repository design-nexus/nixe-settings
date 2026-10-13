"""Elgato Stream Decks for the Stream Deck page: setup (packages, device
access, the nixe-deck service), brightness, profiles and pages, and what each
key does. nixe-deck (beside this file) runs the deck itself."""

import glob
import os
import shlex
import shutil
import subprocess
import tomllib

HERE = os.path.dirname(os.path.realpath(__file__))
HOME = os.path.expanduser("~")
CONF = os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.join(HOME, ".config"), "nixe-deck")
CONFIG = os.path.join(CONF, "config.toml")
PROFILES = os.path.join(CONF, "profiles")
UNIT = "nixe-deck.service"
UNIT_FILE = os.path.join(HOME, ".config/systemd/user", UNIT)
UDEV_RULE = "/etc/udev/rules.d/71-nixe-elgato.rules"
RULE_TEXT = (
    "# Written by Settings: lets the signed-in user run Elgato Stream Decks.\n"
    'SUBSYSTEMS=="usb", ATTRS{idVendor}=="0fd9", TAG+="uaccess"\n'
    'KERNEL=="hidraw*", ATTRS{idVendor}=="0fd9", TAG+="uaccess"\n'
)
PACKAGES = ["python-pillow", "python-tomlkit", "hidapi", "wtype", "playerctl"]
AUR = ["python-elgato-streamdeck"]

# Product id -> (name, keys, columns).
MODELS = {
    "0060": ("Stream Deck", 15, 5), "006d": ("Stream Deck", 15, 5), "0080": ("Stream Deck MK.2", 15, 5),
    "00a5": ("Stream Deck MK.2", 15, 5), "0063": ("Stream Deck Mini", 6, 3), "0090": ("Stream Deck Mini", 6, 3),
    "006c": ("Stream Deck XL", 32, 8), "008f": ("Stream Deck XL", 32, 8), "0084": ("Stream Deck +", 8, 4),
    "009a": ("Stream Deck Neo", 8, 4), "0086": ("Stream Deck Pedal", 3, 3),
}

ACTIONS = [["none", "Nothing"], ["keys", "A shortcut"], ["exec", "Run a command"], ["media", "Media"],
           ["page", "Go to a page"], ["profile", "Switch profile"]]
FIELDS = ("key", "keys", "exec", "media", "page", "profile")
MEDIA = [["play-pause", "Play / pause"], ["next", "Next"], ["previous", "Previous"], ["stop", "Stop"]]

STARTER = """start_page = "main"

[[page]]
name = "main"
keys = [
  { label = "Terminal", exec = "xdg-terminal-exec" },
  { label = "Files", exec = "nautilus --new-window" },
  { label = "Browser", exec = "omarchy-launch-browser" },
  { label = "Play", media = "play-pause" },
  { label = "Next", media = "next" },
  { label = "Screenshot", exec = "omarchy-cmd-screenshot" },
]
"""


def present():
    """The Elgato decks plugged in: [(product id, serial)]."""
    found = []
    for dev in glob.glob("/sys/bus/usb/devices/*"):
        try:
            with open(os.path.join(dev, "idVendor")) as f:
                if f.read().strip() != "0fd9":
                    continue
            with open(os.path.join(dev, "idProduct")) as f:
                pid = f.read().strip()
        except OSError:
            continue
        if pid in MODELS:
            serial = ""
            try:
                with open(os.path.join(dev, "serial")) as f:
                    serial = f.read().strip()
            except OSError:
                pass
            found.append((pid, serial))
    return found


def read_toml(path):
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return {}


def library_ready():
    return subprocess.run(["python3", "-c", "import StreamDeck, PIL"], capture_output=True).returncode == 0


def service_active():
    return subprocess.run(["systemctl", "--user", "is-active", "--quiet", UNIT]).returncode == 0


def profiles():
    return sorted(os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(PROFILES, "*.toml")))


def ensure_starter():
    if not profiles():
        os.makedirs(PROFILES, exist_ok=True)
        with open(os.path.join(PROFILES, "default.toml"), "w") as f:
            f.write(STARTER)


def action_of(spec):
    return next((f for f in FIELDS if f in spec), "none")


def page_names(profile):
    return [p.get("name", f"page {i + 1}") for i, p in enumerate(read_toml(os.path.join(PROFILES, profile + ".toml")).get("page", []))]


def pages():
    return [{"id": "elgato", "title": MODELS.get(present()[0][0], ("Stream Deck",))[0] if present() else "Stream Deck",
             "icon": "view-app-grid-symbolic",
             "description": "Your Elgato Stream Deck: what each key does, profiles, pages and brightness.",
             "keywords": "stream deck streamdeck elgato keys profiles pages macro shortcut dials pedal"}] if present() or os.path.isdir(PROFILES) else []


def setup_rows():
    rows = []
    if not library_ready():
        rows.append({"kind": "button", "key": "elgato-install", "title": "Install what it needs",
                     "desc": "The Stream Deck library (from the AUR) and helpers for shortcuts and media keys. Opens a terminal.",
                     "label": "Install…"})
    if not os.path.exists(UDEV_RULE):
        rows.append({"kind": "button", "key": "elgato-access", "title": "Allow your account to use it",
                     "desc": f"Installs <tt>{UDEV_RULE}</tt>. Asks for your password.", "label": "Allow…"})
    return rows


def key_rows(profile, page, count):
    doc = read_toml(os.path.join(PROFILES, profile + ".toml"))
    keys = next((list(p.get("keys", [])) for p in doc.get("page", []) if p.get("name") == page), [])
    keys += [{}] * (count - len(keys))
    names = page_names(profile)
    rows = []
    for i, spec in enumerate(keys[:count]):
        action = action_of(spec)
        value = spec.get(action, "") if action != "none" else ""
        inner = [
            {"kind": "entry", "key": f"ekey:{i}:label", "title": "Label", "value": spec.get("label", ""), "placeholder": "Shown on the key"},
            {"kind": "choice", "key": f"ekey:{i}:action", "title": "Does", "options": ACTIONS, "value": action, "refresh": True},
        ]
        if action in ("keys", "key"):
            inner.append({"kind": "entry", "key": f"ekey:{i}:value", "title": "Shortcut", "value": value, "placeholder": "CTRL+SHIFT+T"})
        elif action == "exec":
            inner.append({"kind": "entry", "key": f"ekey:{i}:value", "title": "Command", "value": value, "placeholder": "firefox"})
        elif action == "media":
            inner.append({"kind": "choice", "key": f"ekey:{i}:value", "title": "Media", "options": MEDIA, "value": value})
        elif action == "page":
            inner.append({"kind": "choice", "key": f"ekey:{i}:value", "title": "Page", "options": [[n, n] for n in names], "value": value})
        elif action == "profile":
            inner.append({"kind": "choice", "key": f"ekey:{i}:value", "title": "Profile", "options": [[p, p] for p in profiles()], "value": value})
        summary = spec.get("label") or (dict(ACTIONS).get(action, "") if action != "none" else "Nothing")
        rows.append({"kind": "disclosure", "title": f"Key {i + 1} · {summary}", "rows": inner})
    return rows


def recall(state, name):
    try:
        with open(os.path.join(state, "elgato-" + name)) as f:
            return f.read().strip()
    except OSError:
        return ""


def remember(state, name, value):
    os.makedirs(state, exist_ok=True)
    with open(os.path.join(state, "elgato-" + name), "w") as f:
        f.write(value)


def describe(state):
    decks = present()
    groups = []
    banners = []
    if not decks:
        banners.append({"text": "No Elgato Stream Deck is connected right now. Changes are saved for when it is.", "warning": False})
    pid = decks[0][0] if decks else "0080"
    name, count, cols = MODELS.get(pid, ("Stream Deck", 15, 5))
    setup = setup_rows()
    if setup:
        groups.append({"title": "Set up", "rows": setup})
    ensure_starter()
    cfg = read_toml(CONFIG)
    groups.append({"title": "Stream Deck", "rows": [
        {"kind": "switch", "key": "elgato-run", "title": "Running", "value": service_active(),
         "desc": "nixe-deck runs the deck in the background, and starts at every sign-in."},
        {"kind": "slider", "key": "elgato-brightness", "title": "Brightness", "value": int(cfg.get("brightness", 70)),
         "min": 0, "max": 100, "step": 5, "unit": " %", "on_release": True},
    ]})
    profile = recall(state, "profile")
    if profile not in profiles():
        profile = cfg.get("start_profile") or (profiles() or [""])[0]
    names = page_names(profile)
    page = recall(state, "page")
    if page not in names:
        page = names[0] if names else ""
    groups.append({"title": "Profile", "rows": [
        {"kind": "choice", "key": "elgato-edit-profile", "title": "Profile to edit", "options": [[p, p] for p in profiles()], "value": profile, "refresh": True},
        {"kind": "choice", "key": "elgato-edit-page", "title": "Page to edit", "options": [[p, p] for p in names], "value": page, "refresh": True},
        {"kind": "entry", "key": "elgato-new-page", "title": "Add a page", "value": "", "placeholder": "Page name"},
    ]})
    if pid == "0084":
        groups.append({"title": "Dials", "note": "Turning a dial changes the volume; pressing it mutes.", "rows": []})
    groups.append({"title": f"Keys on “{page}”", "note": f"Left to right, top to bottom: {cols} across.", "rows": key_rows(profile, page, count)})
    return {"subtitle": f"<b>{name}</b>", "banners": banners, "groups": [g for g in groups if g["rows"] or g.get("note")]}


def edit_profile(profile, change):
    import tomlkit
    path = os.path.join(PROFILES, profile + ".toml")
    with open(path) as f:
        doc = tomlkit.parse(f.read())
    change(doc)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        f.write(tomlkit.dumps(doc))
    os.replace(tmp, path)


def write_config(key, value):
    import tomlkit
    os.makedirs(CONF, exist_ok=True)
    doc = tomlkit.parse(open(CONFIG).read()) if os.path.exists(CONFIG) else tomlkit.document()
    doc[key] = value
    with open(CONFIG, "w") as f:
        f.write(tomlkit.dumps(doc))


def unit_text():
    return ("[Unit]\nDescription=Elgato Stream Deck (Settings)\nAfter=graphical-session.target\n\n"
            f"[Service]\nExecStart=/usr/bin/python3 {os.path.join(HERE, 'nixe-deck')}\nRestart=on-failure\nRestartSec=5\n\n"
            "[Install]\nWantedBy=default.target\n")


def set_value(state, key, value, start_terminal):
    if key == "elgato-install":
        helper = "yay" if shutil.which("yay") else "paru" if shutil.which("paru") else None
        if not helper:
            raise SystemExit("Install python-elgato-streamdeck from the AUR, and " + " ".join(PACKAGES))
        start_terminal("elgato", f"sudo pacman -S --needed {' '.join(PACKAGES)} && {helper} -S --needed {' '.join(AUR)}")
        return {"toast": "Installing in a terminal", "refresh": True}
    if key == "elgato-access":
        script = f"printf %s {shlex.quote(RULE_TEXT)} > {UDEV_RULE} && udevadm control --reload-rules && udevadm trigger --subsystem-match=usb --subsystem-match=hidraw"
        r = subprocess.run(["pkexec", "sh", "-c", script], capture_output=True, text=True)
        if r.returncode != 0:
            raise SystemExit(r.stderr.strip() or "Not allowed")
        return {"toast": "Allowed: unplug and replug the deck if it doesn't light up", "refresh": True}
    if key == "elgato-run":
        if value == "true":
            os.makedirs(os.path.dirname(UNIT_FILE), exist_ok=True)
            with open(UNIT_FILE, "w") as f:
                f.write(unit_text())
            subprocess.run(["systemctl", "--user", "daemon-reload"])
            r = subprocess.run(["systemctl", "--user", "enable", "--now", UNIT], capture_output=True, text=True)
            if r.returncode != 0:
                raise SystemExit(r.stderr.strip())
        else:
            subprocess.run(["systemctl", "--user", "disable", "--now", UNIT])
        return {"refresh": True}
    if key == "elgato-brightness":
        write_config("brightness", int(float(value)))
        return None
    if key == "elgato-edit-profile":
        remember(state, "profile", value)
        remember(state, "page", "")
        return {"refresh": True}
    if key == "elgato-edit-page":
        remember(state, "page", value)
        return {"refresh": True}
    profile = recall(state, "profile") or (profiles() or [""])[0]
    if key == "elgato-new-page":
        name = value.strip()
        if not name:
            return None
        import tomlkit

        def add(doc):
            pages = doc.setdefault("page", tomlkit.aot())
            pages.append({"name": name, "keys": []})
        edit_profile(profile, add)
        remember(state, "page", name)
        return {"refresh": True}
    if key.startswith("ekey:"):
        _, index, field = key.split(":")
        index = int(index)
        page = recall(state, "page") or (page_names(profile) or [""])[0]

        def change(doc):
            for p in doc.get("page", []):
                if p.get("name") != page:
                    continue
                keys = p.setdefault("keys", [])
                while len(keys) <= index:
                    keys.append({})
                spec = dict(keys[index])
                if field == "label":
                    spec["label"] = value
                elif field == "action":
                    label = spec.get("label")
                    spec = {"label": label} if label else {}
                    if value != "none":
                        spec[value] = ""
                elif field == "value":
                    act = action_of(spec)
                    if act != "none":
                        spec[act] = value
                keys[index] = {k: v for k, v in spec.items() if v != "" or k in FIELDS}
        edit_profile(profile, change)
        return {"refresh": True} if field == "action" else None
    raise SystemExit(f"unknown setting {key}")
