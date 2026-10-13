"""Tests for the low-battery warning. Run: python3 -m unittest test_headset.py"""

import importlib.machinery
import importlib.util
import os
import tempfile
import unittest

os.environ["SETTINGS_EXTENSION_STATE"] = tempfile.mkdtemp()
HERE = os.path.dirname(os.path.realpath(__file__))
loader = importlib.machinery.SourceFileLoader("headset", os.path.join(HERE, "settings-headset"))
spec = importlib.util.spec_from_loader("headset", loader)
h = importlib.util.module_from_spec(spec)
loader.exec_module(h)


class LowBattery(unittest.TestCase):
    def test_warns_once_then_again_after_charging(self):
        sent = []
        h.subprocess.run = lambda argv, **kw: sent.append(argv)
        level = {"v": 10}
        h.devices = lambda: [{"device": "Arctis", "product": "Arctis 7", "id_vendor": "0x1038", "id_product": "0x12ad",
                              "battery": {"status": "BATTERY_AVAILABLE", "level": level["v"]}}]
        h.battery_check()
        h.battery_check()
        self.assertEqual(len([a for a in sent if a[0] == "notify-send"]), 1)
        level["v"] = 90
        h.battery_check()
        level["v"] = 12
        h.battery_check()
        self.assertEqual(len([a for a in sent if a[0] == "notify-send"]), 2)


if __name__ == "__main__":
    unittest.main()
