"""Generates assets/eposctl.ico and assets/logo.png.

Same drawing as `icons::bitmap` in the GUI (full teal ring with a headset in
the middle), so the exe icon matches the tray icon. Standard library only:

    python assets/make_icon.py
"""

import math
import os
import struct
import zlib

ACCENT = (0x44, 0xEA, 0xD6)


def glyph(x, y):
    r = math.hypot(x, y - 0.05)
    band = 0.50 <= r <= 0.66 and y < 0.05
    cup = 0.38 <= abs(x) <= 0.66 and 0.0 <= y <= 0.48
    return band or cup


def rgba(size, ss=4):
    n = float(size)
    c, outer, inner = n / 2, n * 0.48, n * 0.36
    out = bytearray()
    for y in range(size):
        out.append(0)  # PNG filter byte
        for x in range(size):
            hits = 0
            for sy in range(ss):
                for sx in range(ss):
                    fx = x + (sx + 0.5) / ss - c
                    fy = y + (sy + 0.5) / ss - c
                    d = math.hypot(fx, fy)
                    if inner <= d <= outer or glyph(fx / inner, fy / inner):
                        hits += 1
            a = round(hits / (ss * ss) * 255)
            out += bytes(ACCENT + (a,)) if a else b"\0\0\0\0"
    return bytes(out)


def png(size):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rgba(size), 9)) + chunk(b"IEND", b"")


def ico(sizes):
    images = [png(s) for s in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for s, data in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return head + entries + b"".join(images)


here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "eposctl.ico"), "wb") as f:
    f.write(ico([16, 20, 24, 32, 40, 48, 64, 128, 256]))
with open(os.path.join(here, "logo.png"), "wb") as f:
    f.write(png(160))
print("ok")
