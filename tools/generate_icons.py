#!/usr/bin/env python3
"""Write every Den application icon from the shared brand sheet."""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import brand  # noqa: E402

MARK = "den"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "apps", "desktop", "src-tauri", "icons")
PUBLIC = os.path.join(ROOT, "apps", "desktop", "public")

SIZES = [32, 64, 128, 256, 512]


def main():
    tiles = brand.render(MARK, SIZES)
    encoded = {size: brand.png(rows, size) for size, rows in tiles.items()}

    files = {
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
    }

    for path, data in files.items():
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as handle:
            handle.write(data)
        print(f"wrote {os.path.relpath(path, ROOT)} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
