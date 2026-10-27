"""Tests for the Elgato page (elgato.py) and nyx-deck's shortcut typing.
Run: python3 -m unittest test_elgato.py (uses a throwaway config folder)."""

import importlib.machinery
import importlib.util
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
TMP = tempfile.mkdtemp()
os.environ["XDG_CONFIG_HOME"] = TMP
sys.path.insert(0, HERE)
import elgato  # noqa: E402

loader = importlib.machinery.SourceFileLoader("nyx_deck", os.path.join(HERE, "nyx-deck"))
spec = importlib.util.spec_from_loader("nyx_deck", loader)
deck = importlib.util.module_from_spec(spec)
loader.exec_module(deck)


class ShortcutTyping(unittest.TestCase):
    def test_modifiers_wrap_the_key(self):
        self.assertEqual(deck.wtype_args("CTRL+SHIFT+T"), ["-M", "ctrl", "-M", "shift", "-k", "t", "-m", "shift", "-m", "ctrl"])

    def test_named_keys(self):
        self.assertEqual(deck.wtype_args("SUPER+ENTER"), ["-M", "logo", "-k", "Return", "-m", "logo"])
        self.assertEqual(deck.wtype_args("F5"), ["-k", "F5"])
        self.assertEqual(deck.wtype_args(""), [])


try:
    import tomlkit  # noqa: F401
    HAVE_TOMLKIT = True
except ImportError:
    HAVE_TOMLKIT = False


class Page(unittest.TestCase):
    def setUp(self):
        self.state = tempfile.mkdtemp()
        elgato.ensure_starter()

    def test_starter_profile_and_page(self):
        self.assertEqual(elgato.profiles(), ["default"])
        page = elgato.describe(self.state)
        titles = [g["title"] for g in page["groups"]]
        self.assertIn("Profile", titles)
        keys = page["groups"][-1]
        self.assertEqual(len(keys["rows"]), 15)
        self.assertIn("Terminal", keys["rows"][0]["title"])

    @unittest.skipUnless(HAVE_TOMLKIT, "editing needs python-tomlkit")
    def test_editing_a_key(self):
        elgato.set_value(self.state, "ekey:7:action", "exec", None)
        elgato.set_value(self.state, "ekey:7:value", "firefox", None)
        elgato.set_value(self.state, "ekey:7:label", "Web", None)
        keys = elgato.read_toml(os.path.join(elgato.PROFILES, "default.toml"))["page"][0]["keys"]
        self.assertEqual(keys[7], {"exec": "firefox", "label": "Web"})
        elgato.set_value(self.state, "ekey:7:action", "none", None)
        keys = elgato.read_toml(os.path.join(elgato.PROFILES, "default.toml"))["page"][0]["keys"]
        self.assertEqual(keys[7], {"label": "Web"})

    @unittest.skipUnless(HAVE_TOMLKIT, "editing needs python-tomlkit")
    def test_adding_a_page(self):
        elgato.set_value(self.state, "elgato-new-page", "Streaming", None)
        self.assertIn("Streaming", elgato.page_names("default"))


if __name__ == "__main__":
    unittest.main()
