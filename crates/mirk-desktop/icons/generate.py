#!/usr/bin/env python3
"""Generate the source icon the platform set is derived from.

`mirk` has no designer, so the identity is the one the scaffold chose and this script keeps it: the
same palette (dark slate ground, soft blue mark) and the same mark (a centred square inset a quarter
of the way in on every side). What changes is that the ground is now a rounded tile instead of a raw
rectangle of pixels, the edges are anti-aliased, and the source is 1024x1024 — Tauri's
`bun tauri icon` needs one square source of that order to derive `.icns`, `.ico`, 32/128/256 and the
store logos, and a 128x128 flat square upscales into a blurry, obviously placeholder icon.

Nothing here invents a second visual language: change `BACKGROUND`, `MARK` or `MARK_INSET` and the
whole generated set follows.

The pixels are written without an imaging dependency, as before, so regenerating the source needs
nothing but Python: each pixel's coverage comes from the signed distance to the shape's edge, which
is what an anti-aliasing rasteriser approximates anyway.

Usage:  crates/mirk-desktop/icons/generate.py
        bun tauri icon crates/mirk-desktop/icons/source.png -o crates/mirk-desktop/icons
"""

import math
import struct
import zlib
from pathlib import Path

SIZE = 1024

BACKGROUND = (26, 27, 38, 255)    # dark slate
MARK = (137, 180, 250, 255)       # soft blue

# The tile's corner radius, as a fraction of the side. A square tile at full bleed reads as a
# screenshot of a window; the rounded tile reads as an application icon at every size.
TILE_RADIUS = 0.225

# The mark, kept at the scaffold's proportion: a square inset a quarter of the side on every side,
# so it occupies the middle half of the icon.
MARK_INSET = 0.25
MARK_RADIUS = 0.08


def rounded_rect_coverage(x, y, centre, half_side, radius):
    """How much of the pixel at (x, y) is inside a rounded square, from 0 to 1.

    The signed distance to the shape's outline is negative inside and positive outside, so clamping
    the distance to a one-pixel ramp gives the coverage of a rasteriser that samples the edge: fully
    inside is 1, fully outside is 0, and the boundary pixel is the fraction the shape covers.
    """
    qx = abs(x - centre) - (half_side - radius)
    qy = abs(y - centre) - (half_side - radius)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    distance = outside + inside - radius
    return min(max(0.5 - distance, 0.0), 1.0)


def pixels() -> bytes:
    """The icon, as PNG rows: a rounded slate tile carrying the soft blue mark."""
    centre = SIZE / 2
    tile_half = centre
    tile_radius = TILE_RADIUS * SIZE

    mark_half = SIZE * (1 - 2 * MARK_INSET) / 2
    mark_radius = MARK_RADIUS * 2 * mark_half

    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)  # PNG filter type: none
        for x in range(SIZE):
            tile = rounded_rect_coverage(x + 0.5, y + 0.5, centre, tile_half, tile_radius)
            if tile == 0.0:
                rows.extend((0, 0, 0, 0))
                continue
            mark = rounded_rect_coverage(x + 0.5, y + 0.5, centre, mark_half, mark_radius)
            rows.extend(
                (
                    round(BACKGROUND[0] + (MARK[0] - BACKGROUND[0]) * mark),
                    round(BACKGROUND[1] + (MARK[1] - BACKGROUND[1]) * mark),
                    round(BACKGROUND[2] + (MARK[2] - BACKGROUND[2]) * mark),
                    round(255 * tile),
                )
            )
    return bytes(rows)


def chunk(kind: bytes, data: bytes) -> bytes:
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def main() -> None:
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)  # 8-bit RGBA
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(pixels(), 9))
        + chunk(b"IEND", b"")
    )

    target = Path(__file__).parent / "source.png"
    target.write_bytes(png)
    print(f"wrote {target} ({len(png)} bytes, {SIZE}x{SIZE} RGBA)")
    print("next: bun tauri icon crates/mirk-desktop/icons/source.png -o crates/mirk-desktop/icons")


if __name__ == "__main__":
    main()
