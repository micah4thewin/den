#!/usr/bin/env python3
"""Write every Play application icon from the shared brand sheet."""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import brand  # noqa: E402

MARK = "den"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "apps", "desktop", "src-tauri", "icons")
PUBLIC = os.path.join(ROOT, "apps", "desktop", "public")
WEB = os.path.join(ROOT, "crates", "den-web", "web")

SIZES = [32, 64, 128, 192, 256, 512]

# Android masks a home-screen icon to whatever shape the launcher uses, so a
# maskable icon fills its square and keeps the mark inside the circle that
# survives every mask. 0.42 of the canvas clears the 0.8 safe zone with room.
MASKABLE_SIZES = [192, 512]
MASKABLE_SPAN = 0.42


def maskable(size):
    """The mark alone on a full-bleed ink square, inside the safe zone."""
    glyph = brand.render_glyph(MARK, [size], colour=brand.PAPER, span=MASKABLE_SPAN)[size]
    rows = []
    for row in glyph:
        out = bytearray()
        for i in range(0, len(row), 4):
            over = row[i + 3] / 255.0
            for channel in range(3):
                out.append(round(row[i + channel] * over + brand.INK[channel] * (1.0 - over)))
            out.append(255)
        rows.append(bytes(out))
    return rows


def generated():
    """Every generated brand file, as path -> bytes."""
    tiles = brand.render(MARK, SIZES)
    encoded = {size: brand.png(rows, size) for size, rows in tiles.items()}
    masked = {size: brand.png(maskable(size), size) for size in MASKABLE_SIZES}

    return {
        os.path.join(ICONS, "32x32.png"): encoded[32],
        os.path.join(ICONS, "64x64.png"): encoded[64],
        os.path.join(ICONS, "128x128.png"): encoded[128],
        os.path.join(ICONS, "128x128@2x.png"): encoded[256],
        os.path.join(ICONS, "icon.png"): encoded[512],
        os.path.join(ICONS, "icon.ico"): brand.ico([(s, encoded[s]) for s in (32, 64, 128, 256)]),
        os.path.join(ICONS, "icon.icns"): brand.icns(
            [(s, encoded[s]) for s in (32, 128, 256, 512)]
        ),
        os.path.join(PUBLIC, "brand-mark.svg"): brand.svg(MARK).encode("utf-8"),
        # The remote's home-screen icons, for a phone that installs the shelf.
        os.path.join(WEB, "icon-192.png"): encoded[192],
        os.path.join(WEB, "icon-512.png"): encoded[512],
        os.path.join(WEB, "icon-192-maskable.png"): masked[192],
        os.path.join(WEB, "icon-512-maskable.png"): masked[512],
    }


def main():
    for path, data in generated().items():
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as handle:
            handle.write(data)
        print(f"wrote {os.path.relpath(path, ROOT)} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
