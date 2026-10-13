"""Tests for settings-streamdeck: python3 test_settings_streamdeck.py (needs tomlkit)."""

import importlib.machinery
import importlib.util
import os
import shutil
import sys
import tempfile
import time
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


class JobTests(unittest.TestCase):
    """Setup steps run as jobs, so they can't hit Settings' time limit for an answer."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.sd = load(os.path.join(self.tmp, "conf"), os.path.join(self.tmp, "state"))
        self.sd.JOBS = os.path.join(self.tmp, "state", "jobs")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def wait(self, name):
        for _ in range(100):
            st = self.sd.job_state(name)
            if st and st[0] == "done":
                return st
            time.sleep(0.05)
        self.fail("job didn't finish")

    def test_background_job(self):
        self.assertIsNone(self.sd.job_state("galleon"))
        self.sd.start_background("galleon", ["sh", "-c", "sleep 0.3; echo installed"])
        self.assertEqual(self.sd.job_state("galleon")[0], "running")
        self.assertEqual(self.wait("galleon"), ("done", 0, "installed"))

    def test_failed_job_keeps_its_last_lines(self):
        self.sd.start_background("galleon", ["sh", "-c", "echo one; echo two >&2; exit 3"])
        self.assertEqual(self.wait("galleon"), ("done", 3, "one\ntwo"))
        row = self.sd.step_row("install-galleon", "Set up", "desc", "Set up", "galleon", "Setting up")
        self.assertEqual(row["label"], "Try again")
        self.assertIn("exit 3", row["desc"])

    def test_running_step_shows_progress(self):
        self.sd.start_record("packages", terminal=True)
        row = self.sd.step_row("install-packages", "1. Install", "desc", "Install", "packages", "Installing")
        self.assertEqual(row["kind"], "info")
        self.assertIn("terminal window", row["value"])
        self.assertTrue(self.sd.jobs_running())

    def test_terminal_command_records_exit(self):
        import subprocess
        for cmd, code in (("true", 0), ("echo no >&2; exit 4", 4)):
            self.sd.start_record("access", terminal=True)
            r = subprocess.run(["bash", "-c", self.sd.wrap_for_terminal("access", cmd)], capture_output=True)
            self.assertEqual(r.returncode, code, "the terminal sees the real result")
            self.assertEqual(self.sd.job_state("access")[:2], ("done", code))


if __name__ == "__main__":
    unittest.main(verbosity=2)
