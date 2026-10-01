"""Tests for the Webcams extension: python3 test_webcam.py"""

import importlib.machinery
import importlib.util
import os
import shutil
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp()
os.environ["XDG_CONFIG_HOME"] = TMP
os.environ["SETTINGS_KINDS"] = "info switch camera meter"

import obsbot  # noqa: E402

loader = importlib.machinery.SourceFileLoader("webcam", os.path.join(HERE, "settings-webcam"))
spec = importlib.util.spec_from_loader("webcam", loader)
webcam = importlib.util.module_from_spec(spec)
loader.exec_module(webcam)


class Obsbot(unittest.TestCase):
    def test_crc(self):
        self.assertEqual(obsbot.crc16_usb(b"123456789"), 0xB4C8)  # CRC-16/USB check value
        self.assertEqual(obsbot.crc16_usb(bytes([0xAA, 0x25, 0xA5, 0, 0x0C, 0, 0, 0, 0x0A, 0x02, 0xC2, 0xA0])), 0xEF5F)

    def test_frames_match_captures(self):
        # Frames OBSBOT's own software sends, as captured by Tiny4Linux / obsbot-tiny3-linux.
        wake = obsbot.frame(obsbot.FLAG_SET, 0x00A5, obsbot.RCV_CAMERA, obsbot.CMD_POWER, bytes(4))
        self.assertEqual(wake[:20].hex(), "aa25a5000c005fef0a02c2a00400be0700000000")
        sleep = obsbot.frame(obsbot.FLAG_SET, 0x0042, obsbot.RCV_CAMERA, obsbot.CMD_POWER, bytes([1, 0, 0, 0]))
        self.assertEqual(sleep[:20].hex(), "aa2542000c00ea630a02c2a00400bffb01000000")
        self.assertEqual(len(wake), 60)
        self.assertFalse(any(wake[20:]))

    def test_status(self):
        block = bytearray(60)
        block[:14] = bytes([0x2E, 1, 0, 2, 0, 0, 0, 1, 0, 1, 120, 0, 0, 1])
        self.assertEqual(obsbot.parse_status(bytes(block)), (False, 120))
        block[2] = 1
        block[10:12] = (600).to_bytes(2, "little")
        self.assertEqual(obsbot.parse_status(bytes(block)), (True, 600))


class Options(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(os.path.join(TMP, "settings"), ignore_errors=True)

    def test_round_trip(self):
        webcam.save_options({"mic:alsa_input.usb-Remo_OBSBOT": {"default": True, "no_suspend": False},
                             "camera:usb-obsbot-video-index0": {"keep_awake": True}})
        opts = webcam.load_options()
        self.assertTrue(webcam.option(opts, "mic:alsa_input.usb-Remo_OBSBOT", "default"))
        self.assertFalse(webcam.option(opts, "mic:alsa_input.usb-Remo_OBSBOT", "no_suspend"))
        self.assertTrue(webcam.guard_wanted(opts))
        self.assertFalse(webcam.guard_wanted({"mic:x": {"no_suspend": True}}), "a rule file needs no service")

    def test_wireplumber_rule(self):
        r = webcam.wireplumber_rule(["alsa_input.usb-A", "alsa_input.usb-B"])
        self.assertIn('node.name = "alsa_input.usb-A"', r)
        self.assertIn('node.name = "alsa_input.usb-B"', r)
        self.assertIn("session.suspend-timeout-seconds = 0", r)
        self.assertTrue(r.startswith("## Written by the Settings Webcams extension"))


class Mics(unittest.TestCase):
    OBJECTS = [
        {"id": 50, "type": "PipeWire:Interface:Device",
         "info": {"props": {"device.vendor.id": "0x3564", "device.product.id": "0xff04"}}},
        {"id": 51, "type": "PipeWire:Interface:Device",
         "info": {"props": {"device.vendor.id": "0x8086", "device.product.id": "0x7728"}}},
        {"id": 90, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Audio/Source", "device.id": 50,
                            "node.name": "alsa_input.usb-Remo_Tech_OBSBOT_Tiny_3_Lite-02.analog-stereo",
                            "node.description": "OBSBOT Tiny 3 Lite Analog Stereo"}}},
        {"id": 91, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Audio/Source", "device.id": 51, "node.name": "alsa_input.pci"}}},
        {"id": 92, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Audio/Sink", "device.id": 50, "node.name": "alsa_output.usb-Remo"}}},
    ]

    def test_finds_the_cameras_mic(self):
        m = webcam.mics_for(("3564", "ff04"), self.OBJECTS)
        self.assertEqual([x[0] for x in m], [90])
        self.assertEqual(webcam.mics_for(("1234", "5678"), self.OBJECTS), [])
        self.assertEqual(webcam.mics_for(None, self.OBJECTS), [])


class Preview(unittest.TestCase):
    def test_in_page_or_window(self):
        self.assertEqual(webcam.preview_group("/dev/video0")["rows"][0]["kind"], "camera")
        old = webcam.KINDS
        webcam.KINDS = set()
        try:
            self.assertEqual(webcam.preview_group("/dev/video0")["rows"][0]["key"], "@preview-window")
        finally:
            webcam.KINDS = old


if __name__ == "__main__":
    try:
        unittest.main(verbosity=2)
    finally:
        shutil.rmtree(TMP, ignore_errors=True)
