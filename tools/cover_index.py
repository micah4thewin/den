#!/usr/bin/env python3
"""Rebuild the list of box art names Play matches games against.

libretro publishes box art for every system it knows in one GitHub repository
per system (github.com/libretro-thumbnails). Play ships the *names* of those
images -- not the images -- so it can pick the right cover without asking a
listing service first. The image itself is fetched once, when a game that
needs it is shelved.

    python3 tools/cover_index.py            # clone the listings and rewrite the index
    python3 tools/cover_index.py --check    # say how many names each system has

Only git trees are fetched (`--filter=blob:none`), so the whole run moves a
few megabytes, not the images.
"""

import argparse
import gzip
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "crates", "play-art", "data", "boxarts.txt.gz")

# Must match `repo_for` in crates/play-art/src/lib.rs.
REPOS = [
    "Nintendo_-_Nintendo_Entertainment_System",
    "Nintendo_-_Super_Nintendo_Entertainment_System",
    "Sega_-_Mega_Drive_-_Genesis",
    "Sega_-_Mega-CD_-_Sega_CD",
    "Sega_-_32X",
    "Nintendo_-_Nintendo_64",
    "Sony_-_PlayStation",
    "Nintendo_-_Game_Boy",
    "Nintendo_-_Game_Boy_Color",
    "Nintendo_-_Game_Boy_Advance",
    "FBNeo_-_Arcade_Games",
    "DOS",
    "Sony_-_PlayStation_2",
    "Nintendo_-_GameCube",
    "Nintendo_-_Wii",
]


def names_in(repo_dir):
    listing = subprocess.run(
        ["git", "-C", repo_dir, "ls-tree", "-r", "--name-only", "HEAD", "--", "Named_Boxarts"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    out = []
    for line in listing.splitlines():
        name = line.removeprefix("Named_Boxarts/")
        if "/" in name or not name.lower().endswith(".png"):
            continue
        out.append(name[: -len(".png")])
    return sorted(set(out))


def clone(repo, into):
    url = f"https://github.com/libretro-thumbnails/{repo}"
    subprocess.run(
        ["git", "clone", "-q", "--filter=blob:none", "--no-checkout", "--depth", "1", url, into],
        check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--from", dest="source", help="a folder already holding the clones")
    parser.add_argument("--check", action="store_true", help="count what the shipped index holds")
    args = parser.parse_args()

    if args.check:
        with gzip.open(OUT, "rt", encoding="utf-8") as handle:
            counts, current = {}, None
            for line in handle:
                line = line.rstrip("\n")
                if line.startswith("#"):
                    current = line[1:]
                    counts[current] = 0
                elif current:
                    counts[current] += 1
        for repo in REPOS:
            print(f"{counts.get(repo, 0):>7}  {repo}")
        missing = [r for r in REPOS if r not in counts]
        return 1 if missing else 0

    work = args.source or tempfile.mkdtemp(prefix="play-covers-")
    lines = []
    for repo in REPOS:
        repo_dir = os.path.join(work, repo)
        if not os.path.isdir(repo_dir):
            print(f"cloning  {repo}", flush=True)
            clone(repo, repo_dir)
        names = names_in(repo_dir)
        print(f"{len(names):>7}  {repo}", flush=True)
        lines.append(f"#{repo}")
        lines.extend(names)

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    data = ("\n".join(lines) + "\n").encode("utf-8")
    # mtime=0 keeps the file byte-for-byte the same when nothing changed.
    with open(OUT, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=9, mtime=0) as gz:
        gz.write(data)
    print(f"wrote {os.path.relpath(OUT, ROOT)} ({os.path.getsize(OUT)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
