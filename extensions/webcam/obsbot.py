"""OBSBOT Tiny 3 series vendor controls: status, auto-sleep timer, sleep and wake.

A port of the parts of obsbot-tiny3-linux (https://github.com/joshualambert/obsbot-tiny3-linux,
MIT) that the Webcams extension needs. Everything goes through the camera's UVC
extension unit (unit 2) with the UVCIOC_CTRL_QUERY ioctl on its /dev/video node,
which works while another app is streaming and doesn't need root.

- Selector 0x06, read: a 60-byte status block. Byte 0x02 is 1 while asleep; bytes
  0x0a-0x0b are the firmware auto-sleep timer in seconds (0 = never). Reading it
  doesn't wake the camera.
- Selector 0x06, write `0b 02 lo hi`: set that timer.
- Selector 0x02, write a 60-byte "V3" frame: command 0xA0C2 to the camera with
  payload `01 00 00 00` (sleep) or `00 00 00 00` (wake).
"""

import ctypes
import fcntl
import os
import random
import time

VENDOR = "3564"
XU_UNIT = 2
SEL_CMD = 0x02
SEL_STATUS = 0x06
UVC_SET_CUR = 0x01
UVC_GET_CUR = 0x81
UVCIOC_CTRL_QUERY = 0xC0107521
FRAME_LEN = 60

ST_SLEEP = 0x02
ST_AUTO_SLEEP = 0x0A
TLV_AUTO_SLEEP = 0x0B

MAGIC = 0xAA
FLAG_SET = 0x25
SENDER_HOST = 0x0A
RCV_CAMERA = 0x02
CMD_POWER = 0xA0C2


class _Query(ctypes.Structure):
    # struct uvc_xu_control_query; ctypes pads it like C (16 bytes on 64-bit).
    _fields_ = [("unit", ctypes.c_uint8), ("selector", ctypes.c_uint8), ("query", ctypes.c_uint8),
                ("size", ctypes.c_uint16), ("data", ctypes.c_void_p)]


def crc16_usb(data):
    """CRC-16/USB: poly 0xA001 (reflected), init 0xFFFF, xorout 0xFFFF."""
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc ^ 0xFFFF


def frame(flags, seq, receiver, cmd, payload=b""):
    """A 60-byte V3 frame for selector 0x02."""
    if len(payload) > FRAME_LEN - 16:
        raise ValueError("payload too long")
    f = bytearray(FRAME_LEN)
    f[0], f[1] = MAGIC, flags
    f[2:4] = seq.to_bytes(2, "little")
    f[4:6] = (0x000C).to_bytes(2, "little")  # the header token covers bytes 0..12
    f[8], f[9] = SENDER_HOST, receiver
    f[10:12] = cmd.to_bytes(2, "little")
    f[6:8] = crc16_usb(bytes(f[0:6]) + b"\0\0" + bytes(f[8:12])).to_bytes(2, "little")
    if payload:
        n = len(payload).to_bytes(2, "little")
        f[12:14] = n
        f[14:16] = crc16_usb(n + b"\0\0" + bytes(payload)).to_bytes(2, "little")
        f[16:16 + len(payload)] = payload
    return bytes(f)


def parse_status(block):
    """(asleep, auto_sleep_seconds) from the 60-byte status block."""
    return block[ST_SLEEP] != 0, int.from_bytes(block[ST_AUTO_SLEEP:ST_AUTO_SLEEP + 2], "little")


def is_tiny3(video_node):
    """Whether /dev/videoN belongs to an OBSBOT Tiny 3 series camera."""
    usb = usb_ids(video_node)
    if not usb or usb[0] != VENDOR:
        return False
    name = os.path.basename(video_node)
    try:
        for link in os.listdir("/dev/v4l/by-id"):
            if os.path.basename(os.path.realpath(os.path.join("/dev/v4l/by-id", link))) == name:
                return "OBSBOT_Tiny_3" in link
    except OSError:
        pass
    return usb[1] == "ff04"  # Tiny 3 Lite


def usb_ids(video_node):
    """(vendor, product) of the USB device behind /dev/videoN, as lowercase hex."""
    d = os.path.realpath(f"/sys/class/video4linux/{os.path.basename(video_node)}/device")
    for _ in range(4):  # interface → device
        try:
            with open(os.path.join(d, "idVendor")) as v, open(os.path.join(d, "idProduct")) as p:
                return v.read().strip().lower(), p.read().strip().lower()
        except OSError:
            d = os.path.dirname(d)
    return None


class Camera:
    def __init__(self, video_node):
        self.fd = os.open(video_node, os.O_RDWR | os.O_CLOEXEC)
        self.seq = random.randint(1, 0xFFFF)

    def close(self):
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def _xu(self, selector, query, buf):
        data = (ctypes.c_uint8 * FRAME_LEN).from_buffer(buf)
        q = _Query(XU_UNIT, selector, query, FRAME_LEN, ctypes.addressof(data))
        fcntl.ioctl(self.fd, UVCIOC_CTRL_QUERY, q)

    def get(self, selector):
        buf = bytearray(FRAME_LEN)
        self._xu(selector, UVC_GET_CUR, buf)
        return bytes(buf)

    def set(self, selector, data):
        buf = bytearray(FRAME_LEN)
        buf[:len(data)] = data
        self._xu(selector, UVC_SET_CUR, buf)

    def status(self):
        return parse_status(self.get(SEL_STATUS))

    def set_auto_sleep(self, seconds):
        self.set(SEL_STATUS, bytes([TLV_AUTO_SLEEP, 2]) + int(seconds).to_bytes(2, "little"))
        for _ in range(10):
            time.sleep(0.05)
            if self.status()[1] == seconds:
                return True
        return False

    def power(self, asleep):
        self.seq = (self.seq + 1) & 0xFFFF or 1
        self.set(SEL_CMD, frame(FLAG_SET, self.seq, RCV_CAMERA, CMD_POWER, bytes([1 if asleep else 0, 0, 0, 0])))
        # The gimbal takes a moment; confirm from the status block.
        for _ in range(30):
            time.sleep(0.1)
            if self.status()[0] == asleep:
                return True
        return False
