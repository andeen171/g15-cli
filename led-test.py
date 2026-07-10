#!/usr/bin/env python3
"""Set G15 5520 keyboard backlight (187c:0550) — exact AWCC-captured sequence.

Transport must be the USB control pipe (SET_REPORT/GET_REPORT ioctls);
the firmware ignores the interrupt endpoints, so plain hidraw write()/read()
gets no response. This is why dell-g-series-controller failed on this machine.

Usage:
  led-test.py                         # white, full brightness
  led-test.py RR GG BB [brightness]   # hex color, brightness 0-100 (default 100)

NEVER write /sys/class/leds/dell::kbd_backlight — SMBIOS writes wedge this
controller until full power removal (see protocol.md).
"""
import fcntl, os, sys

DEV = "/dev/hidraw0"
ZONES = [0x10, 0x11, 0x12, 0x13]
DIM_ZONES = list(range(0x14))  # dimming addresses all 20 zones


def _ioc(nr):  # _IOC(READ|WRITE, 'H', nr, 34): report-number byte + 33 data
    return (3 << 30) | (34 << 16) | (ord("H") << 8) | nr


HIDIOCGINPUT, HIDIOCSOUTPUT = _ioc(0x0A), _ioc(0x0B)


def cmd(fd, *payload):
    buf = bytearray(34)  # buf[0] = report number 0 (device has no report IDs)
    buf[1:1 + len(payload) + 1] = bytes([0x03, *payload])
    fcntl.ioctl(fd, HIDIOCSOUTPUT, buf)
    ack = bytearray(34)
    fcntl.ioctl(fd, HIDIOCGINPUT, ack)
    return bytes(ack[1:])


def set_color(r, g, b, brightness=100):
    # 0x26 is DIMMING: 0 = full brightness, 100 = off (inverted!)
    fd = os.open(DEV, os.O_RDWR)
    try:
        cmd(fd, 0x26, 100 - brightness, 0x00, len(DIM_ZONES), *DIM_ZONES)
        cmd(fd, 0x21, 0x00, 0x01, 0xFF, 0xFF)                  # start new RUNNING animation
        cmd(fd, 0x23, 0x01, 0x00, len(ZONES), *ZONES)          # series: loop, 4 zones
        cmd(fd, 0x24, 0x00, 0x07, 0xD0, 0x00, 0xFA, r, g, b)   # action: static color
        cmd(fd, 0x21, 0x00, 0x03, 0x00, 0xFF)                  # finish-play RUNNING
    finally:
        os.close(fd)


if __name__ == "__main__":
    a = sys.argv[1:]
    r, g, b = (int(x, 16) for x in (a[:3] or ["ff", "ff", "ff"]))
    set_color(r, g, b, int(a[3]) if len(a) > 3 else 100)
    print(f"applied #{r:02x}{g:02x}{b:02x}")
