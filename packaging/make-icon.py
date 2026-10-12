#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Generate the wideviner application icon using only the standard library.

Draws a rounded blue tile with a white "W" and writes it as a PNG. Run it from
the repository root:

    python3 packaging/make-icon.py packaging/wideviner.png
"""

import math
import struct
import sys
import zlib
from pathlib import Path

SIZE = 256
MARGIN = 22.0
RADIUS = 52.0

BG_TOP = (0x3B, 0x82, 0xFF)
BG_BOTTOM = (0x1A, 0x4F, 0xC8)
FG = (0xFF, 0xFF, 0xFF)

# The strokes of the "W", as a list of line segments.
STROKES = [
    ((60.0, 88.0), (86.0, 176.0)),
    ((86.0, 176.0), (128.0, 110.0)),
    ((128.0, 110.0), (170.0, 176.0)),
    ((170.0, 176.0), (196.0, 88.0)),
]
STROKE_HALF_WIDTH = 11.5


def rounded_rect_sdf(x: float, y: float) -> float:
    """Signed distance from (x, y) to the rounded rectangle."""
    left, top = MARGIN, MARGIN
    right, bottom = SIZE - MARGIN, SIZE - MARGIN
    cx = min(max(x, left + RADIUS), right - RADIUS)
    cy = min(max(y, top + RADIUS), bottom - RADIUS)
    return math.hypot(x - cx, y - cy) - RADIUS


def segment_distance(px: float, py: float, a, b) -> float:
    ax, ay = a
    bx, by = b
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    length2 = vx * vx + vy * vy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / length2))
    cx, cy = ax + t * vx, ay + t * vy
    return math.hypot(px - cx, py - cy)


def clamp01(value: float) -> float:
    return max(0.0, min(1.0, value))


def coverage(distance: float, half: float = 0.5) -> float:
    """Anti-aliased coverage from a signed distance."""
    return clamp01(half - distance)


def build_pixels() -> bytes:
    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)  # PNG filter type 0
        # vertical gradient for the tile background
        t = y / (SIZE - 1)
        bg = tuple(
            round(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3)
        )

        for x in range(SIZE):
            fx, fy = x + 0.5, y + 0.5

            tile = coverage(rounded_rect_sdf(fx, fy))
            if tile <= 0.0:
                rows.extend((0, 0, 0, 0))
                continue

            # the letter on top of the tile
            letter = 0.0
            for a, b in STROKES:
                letter = max(
                    letter,
                    coverage(segment_distance(fx, fy, a, b) - STROKE_HALF_WIDTH),
                )
            letter *= tile

            r = round(bg[0] + (FG[0] - bg[0]) * letter)
            g = round(bg[1] + (FG[1] - bg[1]) * letter)
            b_ = round(bg[2] + (FG[2] - bg[2]) * letter)
            rows.extend((r, g, b_, round(tile * 255)))
    return bytes(rows)


def chunk(kind: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def write_png(path: Path, pixels: bytes) -> None:
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", header)
    png += chunk(b"IDAT", zlib.compress(pixels, 9))
    png += chunk(b"IEND", b"")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)


def main() -> int:
    target = Path(sys.argv[1] if len(sys.argv) > 1 else "packaging/wideviner.png")
    write_png(target, build_pixels())
    print(f"wrote {target} ({target.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
