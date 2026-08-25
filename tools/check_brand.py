#!/usr/bin/env python3
"""Assert that the brand mark in the interface is the one on the icon."""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import brand  # noqa: E402
import generate_icons  # noqa: E402

MARK = "den"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS_TS = os.path.join(ROOT, "apps", "desktop", "src", "ui", "icons.ts")
ICONS_DIR = os.path.join(ROOT, "apps", "desktop", "src-tauri", "icons")
PUBLIC_DIR = os.path.join(ROOT, "apps", "desktop", "public")


def normalize(d):
    return re.sub(r"\s+", " ", d).strip()


def bracketed(source, key):
    start = source.index(key)
    i = source.index("[", start)
    depth = 0
    for j in range(i, len(source)):
        if source[j] == "[":
            depth += 1
        elif source[j] == "]":
            depth -= 1
            if depth == 0:
                return source[start:j]
    raise ValueError(f"unterminated block after {key}")


def interface_paths():
    source = open(ICONS_TS, encoding="utf-8").read()
    body = bracketed(source, "brandDen: [")
    body = re.sub(r'"\s*\+\s*"', "", body)
    return [normalize(m) for m in re.findall(r'"((?:M|m)[^"]*)"', body)]


def sheet_paths():
    return [normalize(shape["d"]) for shape in brand.MARKS[MARK]]


def _check_generated():
    problems = []
    tiles = brand.render(MARK, generate_icons.SIZES)
    encoded = {size: brand.png(rows, size) for size, rows in tiles.items()}
    expected = {
        os.path.join(ICONS_DIR, "32x32.png"): encoded[32],
        os.path.join(ICONS_DIR, "64x64.png"): encoded[64],
        os.path.join(ICONS_DIR, "128x128.png"): encoded[128],
        os.path.join(ICONS_DIR, "128x128@2x.png"): encoded[256],
        os.path.join(ICONS_DIR, "icon.png"): encoded[512],
        os.path.join(ICONS_DIR, "icon.ico"): brand.ico(
            [(s, encoded[s]) for s in (32, 64, 128, 256)]
        ),
        os.path.join(ICONS_DIR, "icon.icns"): brand.icns(
            [(s, encoded[s]) for s in (32, 128, 256, 512)]
        ),
        os.path.join(PUBLIC_DIR, "brand-mark.svg"): brand.svg(MARK).encode("utf-8"),
    }
    for path, data in expected.items():
        if not os.path.isfile(path):
            problems.append(f"missing {os.path.relpath(path, ROOT)}")
            continue
        with open(path, "rb") as handle:
            if handle.read() != data:
                problems.append(
                    f"{os.path.relpath(path, ROOT)} is stale; re-run generate_icons.py"
                )
    return problems


def main():
    problems = []

    drawn = interface_paths()
    sheet = sheet_paths()
    if not drawn:
        problems.append(f"could not find brandDen path data in {ICONS_TS}")
    elif drawn != sheet:
        problems.append(
            "brandDen in icons.ts differs from brand.py; re-run generate_icons.py"
        )

    problems.extend(_check_generated())

    if problems:
        for p in problems:
            print(f"brand check failed: {p}")
        sys.exit(1)
    print("brand check passed")


if __name__ == "__main__":
    main()
