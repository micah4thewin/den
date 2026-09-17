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
REMOTE_HTML = os.path.join(ROOT, "crates", "den-web", "web", "index.html")


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


def remote_paths():
    """The mark inlined in the page Play serves to the network.

    It cannot import anything — a standalone page has no bundler — so the
    drawing is written out there, and this is what keeps it the same drawing.
    """
    source = open(REMOTE_HTML, encoding="utf-8").read()
    start = source.find('class="mark"')
    if start < 0:
        return []
    end = source.find("</svg>", start)
    if end < 0:
        return []
    return [normalize(d) for d in re.findall(r'\sd="([^"]*)"', source[start:end])]


def sheet_paths():
    return [normalize(shape["d"]) for shape in brand.MARKS[MARK]]


def _check_generated():
    """Every generated file, against what the brand sheet says it should be."""
    problems = []
    for path, data in generate_icons.generated().items():
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

    remote = remote_paths()
    if not remote:
        problems.append(f"could not find the mark in {REMOTE_HTML}")
    elif remote != sheet:
        problems.append(
            "the mark in den-web/web/index.html differs from brand.py; "
            "copy the paths across"
        )

    problems.extend(_check_generated())

    if problems:
        for p in problems:
            print(f"brand check failed: {p}")
        sys.exit(1)
    print("brand check passed")


if __name__ == "__main__":
    main()
