"""Tests for settings-streamdeck: python3 test_settings_streamdeck.py (needs tomlkit)."""

import importlib.machinery
import importlib.util
import os
import shutil
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE = os.environ.get("GALLEON_EXAMPLES")  # galleon-deck's examples/ folder


def load(config_home, state):
    os.environ["XDG_CONFIG_HOME"] = config_home
    os.environ["SETTINGS_EXTENSION_STATE"] = state
    os.environ["CORSAIR_MOCK"] = "galleon-100-sd"
    loader = importlib.machinery.SourceFileLoader("sd", os.path.join(HERE, "settings-streamdeck"))
    spec = importlib.util.spec_from_loader("sd", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


@unittest.skipUnless(SAMPLE, "set GALLEON_EXAMPLES to galleon-deck's examples/ folder")
class StreamDeckTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        shutil.copytree(SAMPLE, os.path.join(self.tmp, "galleon-deck"))
        self.sd = load(self.tmp, os.path.join(self.tmp, "state"))
        self.sd.BIN = os.path.join(self.tmp, "bin")
        os.makedirs(self.sd.BIN)
        open(os.path.join(self.sd.BIN, "galleon-deck"), "w").close()

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def keys(self):
        return self.sd.page_keys(self.sd.current_profile(), self.sd.current_page(self.sd.current_profile()))

    def test_page_and_groups(self):
        self.assertEqual(self.sd.pages()[0]["id"], "deck")
        titles = [g["title"] for g in self.sd.describe("deck")["groups"]]
        self.assertIn("Profile", titles)
        self.assertEqual(titles[-1], "Keys on “numpad”")
        rows = self.sd.describe("deck")["groups"][-1]["rows"]
        self.assertEqual(len(rows), 12)
        self.assertEqual(rows[0]["title"], "Key 1 · 7")

    def test_edits_keep_comments(self):
        path = self.sd.profile_path("desktop")
        self.sd.set_value("deck", "key:0:label", "Seven")
        self.sd.set_value("deck", "key:0:action", "exec")
        self.sd.set_value("deck", "key:0:value", "firefox")
        self.sd.set_value("deck", "key:0:confirm", "true")
        k = self.keys()[0]
        self.assertEqual(k, {"label": "Seven", "exec": "firefox", "confirm": True})
        with open(path) as f:
            self.assertIn("# Desktop profile", f.read())
        self.sd.set_value("deck", "key:0:action", "none")
        self.assertEqual(self.keys()[0], {"label": "Seven"})

    def test_short_pages_are_padded(self):
        self.sd.set_value("deck", "edit-page", "numpad ops")
        self.sd.set_value("deck", "key:11:label", "end")
        self.assertEqual(self.keys()[11]["label"], "end")
        self.assertEqual(len(self.keys()), 12)

    def test_global_settings(self):
        self.sd.set_value("deck", "brightness", "45.0")
        self.sd.set_value("deck", "boot", "false")
        cfg = self.sd.read_toml(self.sd.CONFIG)
        self.assertEqual((cfg["brightness"], cfg["boot"]["enabled"]), (45, False))
        with open(self.sd.CONFIG) as f:
            self.assertIn("# galleon-deck:", f.read())

    def test_rejects_unknown(self):
        with self.assertRaises(SystemExit):
            self.sd.set_value("deck", "key:12:label", "x")
        with self.assertRaises(SystemExit):
            self.sd.set_value("deck", "warp-drive", "1")


if __name__ == "__main__":
    unittest.main(verbosity=2)
