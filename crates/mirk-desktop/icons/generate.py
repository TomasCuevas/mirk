#!/usr/bin/env python3
"""Generate the placeholder application icon.

`mirk` has no visual identity yet, and Tauri's build step validates that the icon referenced by
`tauri.conf.json` exists. Rather than committing a binary nobody chose, the scaffold generates a
plain square that is obviously a placeholder. Replace it when the design lands.
"""

import struct
import zlib
from pathlib import Path

SIZE = 128
BACKGROUND = (26, 27, 38, 255)   # dark slate
MARK = (137, 180, 250, 255)      # soft blue


def pixels() -> bytes:
    """A dark square with a lighter inner square, drawn without any image dependency."""
    inset = SIZE // 4
    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)  # PNG filter type: none
        for x in range(SIZE):
            inside = inset <= x < SIZE - inset and inset <= y < SIZE - inset
            rows.extend(MARK if inside else BACKGROUND)
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

    target = Path(__file__).parent / "icon.png"
    target.write_bytes(png)
    print(f"wrote {target} ({len(png)} bytes, {SIZE}x{SIZE} RGBA)")


if __name__ == "__main__":
    main()
