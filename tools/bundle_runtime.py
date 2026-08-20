#!/usr/bin/env python3
"""Put a RetroArch inside Den, so a built Den needs nothing installed.

    python3 tools/bundle_runtime.py                     # from this machine
    python3 tools/bundle_runtime.py --from-archive X    # from a download
    python3 tools/bundle_runtime.py --check             # say what it would do

What it writes is `apps/desktop/src-tauri/resources/runtime/`, which
`tauri.conf.json` ships as a bundle resource; the shell hands that directory
to `den-runner` as `DEN_RUNTIME_DIR`, and `Runner::locate` prefers it over
anything installed on the machine. A build with a runtime staged is
self-contained; a build without one falls back to the system, so this script
is never required to get a working Den.

Three sources, in the order you should prefer them:

  --from-archive  A RetroArch you downloaded: an AppImage, a .zip, a .tar.*,
                  a .7z, or an unpacked directory. This is the one to use for
                  a release, because those builds carry their own libraries
                  and so run on a machine that is not yours.

  --from-system   The RetroArch already installed here, found the same way
                  Den finds it. Immediate and needs no network, but a native
                  package is linked against this machine's libraries: good
                  for a build you are going to run yourself, not for one you
                  are going to hand to somebody else.

  --from-manifest The URLs in tools/runtime-manifest.json. Needs network.

A note on licences, because bundling changes them from somebody else's
problem into yours. RetroArch is GPLv3: shipping it inside Den is fine --
Den runs it as a separate process, which is aggregation rather than linking,
so Den's own MIT terms are unaffected -- but *distributing* that bundle
carries GPLv3's obligations, including offering the corresponding source.
The cores are not uniform: mesen, mupen64plus_next and swanstation are GPL,
mgba is MPL-2.0, and snes9x, genesis_plus_gx and fbneo carry non-commercial
terms that restrict redistribution. Bundling for yourself is unproblematic.
Publishing a bundle means reading those terms. This script prints what it
staged and under what licence so the question is at least visible.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RESOURCES = os.path.join(ROOT, "apps", "desktop", "src-tauri", "resources")
RUNTIME = os.path.join(RESOURCES, "runtime")
MANIFEST = os.path.join(ROOT, "tools", "runtime-manifest.json")

# The cores Den asks for by default, and what each one is under.
CORE_LICENCES = {
    "mesen": "GPLv3",
    "snes9x": "Snes9x (non-commercial)",
    "genesis_plus_gx": "Genesis Plus GX (non-commercial)",
    "picodrive": "MAME-like (non-commercial)",
    "mupen64plus_next": "GPLv3",
    "swanstation": "GPLv3",
    "gambatte": "GPLv2",
    "mgba": "MPL-2.0",
    "fbneo": "FBNeo (non-commercial)",
    "dosbox_pure": "GPLv2",
}

BINARY_NAMES = (
    ["retroarch.exe", "retroarch"]
    if sys.platform == "win32"
    else ["retroarch", "org.libretro.RetroArch", "RetroArch"]
)

CORE_EXT = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")


def log(message):
    print(message, flush=True)


# ---- finding a RetroArch on this machine --------------------------------


def system_candidates():
    """The same list den-runner walks, kept deliberately in step with it."""
    out = []
    for directory in os.environ.get("PATH", "").split(os.pathsep):
        if not directory:
            continue
        for name in BINARY_NAMES:
            out.append(os.path.join(directory, name))

    home = os.path.expanduser("~")
    if sys.platform == "win32":
        for var in ("ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"):
            base = os.environ.get(var)
            if base:
                out += [
                    os.path.join(base, "RetroArch", "retroarch.exe"),
                    os.path.join(base, "RetroArch-Win64", "retroarch.exe"),
                    os.path.join(base, "Programs", "RetroArch", "retroarch.exe"),
                ]
    elif sys.platform == "darwin":
        out += [
            "/Applications/RetroArch.app/Contents/MacOS/RetroArch",
            os.path.join(home, "Applications/RetroArch.app/Contents/MacOS/RetroArch"),
            "/opt/homebrew/bin/retroarch",
            "/usr/local/bin/retroarch",
        ]
    else:
        out += [
            "/usr/bin/retroarch",
            "/usr/local/bin/retroarch",
            "/usr/games/retroarch",
            "/snap/bin/retroarch",
            "/var/lib/flatpak/exports/bin/org.libretro.RetroArch",
            os.path.join(home, ".local/share/flatpak/exports/bin/org.libretro.RetroArch"),
            os.path.join(home, ".local/bin/retroarch"),
        ]
    return out


def find_system_retroarch():
    """The RetroArch on this machine, as found -- symlinks left alone.

    `realpath` here would turn the Flatpak and Snap wrappers into
    `/usr/bin/flatpak` and `/usr/bin/snap`, which behave like RetroArch only
    because they look at the name they were invoked under.
    """
    override = os.environ.get("RETROARCH")
    if override and os.path.isfile(override) and os.access(override, os.X_OK):
        return override
    for candidate in system_candidates():
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


def is_a_wrapper(path):
    """Whether this is a launcher that starts RetroArch rather than RetroArch.

    Copying one into a bundle gets you a file that needs the very thing the
    bundle exists to avoid needing.
    """
    target = os.path.basename(os.path.realpath(path)).lower()
    return target in ("flatpak", "snap")


def find_core_dir(retroarch):
    """The cores directory that goes with a RetroArch, if there is one."""
    home = os.path.expanduser("~")
    parent = os.path.dirname(retroarch)
    for candidate in [
        os.path.join(parent, "cores"),
        os.path.join(os.path.dirname(parent), "Resources", "cores"),
        os.path.join(home, ".config/retroarch/cores"),
        os.path.join(home, ".var/app/org.libretro.RetroArch/config/retroarch/cores"),
        os.path.join(home, "snap/retroarch/current/.config/retroarch/cores"),
        os.path.join(home, "Library/Application Support/RetroArch/cores"),
        "/usr/lib/libretro",
        "/usr/local/lib/libretro",
        "/usr/lib/x86_64-linux-gnu/libretro",
    ]:
        if os.path.isdir(candidate):
            return candidate
    return None


# ---- unpacking a download ------------------------------------------------


def extract(archive, into):
    """Unpack `archive` into `into`, whatever shape it arrived in."""
    lower = archive.lower()
    if os.path.isdir(archive):
        shutil.copytree(archive, into, dirs_exist_ok=True)
        return
    if lower.endswith(".appimage"):
        # An AppImage is already one self-contained executable; that is the
        # whole point of it, so it becomes the binary rather than being
        # unpacked into pieces.
        os.makedirs(into, exist_ok=True)
        target = os.path.join(into, "retroarch")
        shutil.copy2(archive, target)
        os.chmod(target, 0o755)
        return
    if lower.endswith(".zip"):
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(into)
        return
    if any(lower.endswith(s) for s in (".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2")):
        with tarfile.open(archive) as tf:
            tf.extractall(into)
        return
    if lower.endswith(".7z"):
        for tool in ("7zz", "7z", "7za"):
            if shutil.which(tool):
                subprocess.run([tool, "x", "-y", f"-o{into}", archive], check=True)
                return
        raise SystemExit(
            f"{archive} is a .7z and no 7-Zip tool is installed.\n"
            "Install p7zip (`apt install p7zip-full`, `brew install sevenzip`),\n"
            "or unpack it yourself and pass the directory."
        )
    raise SystemExit(f"don't know how to unpack {archive}")


def flatten(into):
    """Lift a single wrapper directory, so the binary is where we expect."""
    entries = [e for e in os.listdir(into) if not e.startswith(".")]
    if len(entries) != 1:
        return
    only = os.path.join(into, entries[0])
    if not os.path.isdir(only):
        return
    for name in os.listdir(only):
        shutil.move(os.path.join(only, name), os.path.join(into, name))
    os.rmdir(only)


def find_binary(root):
    """The RetroArch inside a staged directory."""
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in BINARY_NAMES:
            if name in filenames:
                return os.path.join(dirpath, name)
    return None


# ---- staging -------------------------------------------------------------


def stage_from_system(dest, want_cores):
    retroarch = find_system_retroarch()
    if not retroarch:
        raise SystemExit(
            "No RetroArch found on this machine.\n"
            "Install one, or use --from-archive with a download."
        )
    if is_a_wrapper(retroarch):
        raise SystemExit(
            f"{retroarch} is a launcher, not RetroArch itself\n"
            f"  (it runs {os.path.realpath(retroarch)}).\n"
            "Copying it into a bundle would ship a file that needs the very\n"
            "thing the bundle exists to avoid needing. Download a portable\n"
            "build and pass it with --from-archive instead."
        )
    log(f"  binary   {retroarch}")
    log("  note     a system RetroArch is linked against this machine's")
    log("           libraries; use --from-archive for a bundle to hand on")
    os.makedirs(dest, exist_ok=True)
    target = os.path.join(dest, "retroarch.exe" if sys.platform == "win32" else "retroarch")
    shutil.copy2(retroarch, target)
    os.chmod(target, 0o755)

    if want_cores:
        source = find_core_dir(retroarch)
        if not source:
            log("  cores    none found beside it; RetroArch will use its own")
        else:
            log(f"  cores    {source}")
            copy_cores(source, os.path.join(dest, "cores"))
    return target


def copy_cores(source, dest):
    os.makedirs(dest, exist_ok=True)
    copied = []
    for core, licence in sorted(CORE_LICENCES.items()):
        name = f"{core}_libretro{CORE_EXT}"
        path = os.path.join(source, name)
        if os.path.isfile(path):
            shutil.copy2(path, os.path.join(dest, name))
            copied.append((name, licence))
    for name, licence in copied:
        log(f"           {name}  [{licence}]")
    missing = len(CORE_LICENCES) - len(copied)
    if missing:
        log(f"           {missing} of Den's default cores were not installed here")
    return copied


def load_manifest():
    if not os.path.isfile(MANIFEST):
        raise SystemExit(f"no manifest at {MANIFEST}")
    with open(MANIFEST, encoding="utf-8") as handle:
        return json.load(handle)


def platform_key():
    machine = (os.uname().machine if hasattr(os, "uname") else "x86_64").lower()
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}
    return f"{sys.platform}-{arch.get(machine, machine)}"


def stage_from_manifest(dest, want_cores, record):
    import urllib.request

    manifest = load_manifest()
    key = platform_key()
    entry = manifest.get("platforms", {}).get(key)
    if not entry or not entry.get("url"):
        raise SystemExit(
            f"The manifest has no download for {key}.\n"
            f"Add one to {os.path.relpath(MANIFEST, ROOT)}, or use --from-archive."
        )
    url = entry["url"]
    log(f"  fetching {url}")
    tmp = os.path.join(RESOURCES, ".download")
    os.makedirs(RESOURCES, exist_ok=True)
    with urllib.request.urlopen(url, timeout=120) as response, open(tmp, "wb") as out:
        shutil.copyfileobj(response, out)

    digest = hashlib.sha256()
    with open(tmp, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    got = digest.hexdigest()
    expected = entry.get("sha256")
    if record:
        entry["sha256"] = got
        manifest.setdefault("platforms", {})[key] = entry
        with open(MANIFEST, "w", encoding="utf-8") as handle:
            json.dump(manifest, handle, indent=2, sort_keys=True)
            handle.write("\n")
        log(f"  recorded sha256 {got}")
    elif not expected:
        os.remove(tmp)
        raise SystemExit(
            f"The manifest has no sha256 for {key}. Re-run with --record to pin\n"
            "the hash of what you just downloaded, having satisfied yourself it\n"
            "is the right file."
        )
    elif expected != got:
        os.remove(tmp)
        raise SystemExit(f"sha256 mismatch\n  expected {expected}\n  got      {got}")

    named = tmp + os.path.splitext(url)[1]
    os.replace(tmp, named)
    extract(named, dest)
    os.remove(named)
    flatten(dest)
    binary = find_binary(dest)
    if not binary:
        raise SystemExit(f"no RetroArch binary inside {url}")
    os.chmod(binary, 0o755)
    if want_cores:
        log("  cores    a downloaded RetroArch fetches its own on first run")
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--from-system", action="store_true", help="copy the RetroArch installed here (default)")
    source.add_argument("--from-archive", metavar="PATH", help="an AppImage, archive, or unpacked directory")
    source.add_argument("--from-manifest", action="store_true", help="download per tools/runtime-manifest.json")
    parser.add_argument("--into", default=RUNTIME, help="where to stage (default: the bundle resources)")
    parser.add_argument("--no-cores", action="store_true", help="stage RetroArch only")
    parser.add_argument("--record", action="store_true", help="with --from-manifest, pin the hash of what was downloaded")
    parser.add_argument("--check", action="store_true", help="say what is here and what would be staged, and change nothing")
    parser.add_argument("--allow-missing", action="store_true", help="exit 0 when there is nothing to stage (for builds)")
    parser.add_argument("--keep-existing", action="store_true", help="leave an already-staged runtime alone (for builds)")
    args = parser.parse_args()

    if args.check:
        return check(args.into)

    dest = args.into
    # A build must never destroy a runtime that was staged on purpose. The
    # release flow is `--from-archive <portable build>` and then a build; if
    # the build re-staged from this machine it would silently swap a portable
    # RetroArch for a natively linked one.
    already = find_binary(dest) if os.path.isdir(dest) else None
    if args.keep_existing and already:
        log(f"Keeping the runtime already staged at {os.path.relpath(already, ROOT)}")
        return 0
    clear(dest)
    os.makedirs(dest, exist_ok=True)

    log(f"Staging a RetroArch into {os.path.relpath(dest, ROOT)}")
    try:
        if args.from_archive:
            log(f"  source   {args.from_archive}")
            extract(args.from_archive, dest)
            flatten(dest)
            binary = find_binary(dest)
            if not binary:
                raise SystemExit(f"no RetroArch binary inside {args.from_archive}")
            os.chmod(binary, 0o755)
        elif args.from_manifest:
            binary = stage_from_manifest(dest, not args.no_cores, args.record)
        else:
            binary = stage_from_system(dest, not args.no_cores)
    except SystemExit as e:
        if args.allow_missing:
            clear(dest)
            log(f"  nothing staged: {e}")
            log("  the build will fall back to whatever RetroArch is installed.")
            return 0
        raise

    log(f"\nStaged {os.path.relpath(binary, ROOT)}")
    log("This build of Den will prefer it over anything installed on the machine.")
    log("RetroArch is GPLv3; see the note at the top of this script before you")
    log("hand the bundle to anybody else.")
    return 0


def clear(dest):
    """Empty the staging directory, keeping the README that documents it.

    The directory itself is committed with a README in it so that a fresh
    clone has somewhere for the bundler to look; wiping it wholesale would
    delete a tracked file every time this ran.
    """
    if not os.path.isdir(dest):
        return
    for name in os.listdir(dest):
        if name == "README.md":
            continue
        path = os.path.join(dest, name)
        shutil.rmtree(path) if os.path.isdir(path) else os.remove(path)


def check(dest):
    log("What Den would bundle")
    retroarch = find_system_retroarch()
    log(f"  on this machine   {retroarch or 'nothing found'}")
    if retroarch:
        cores = find_core_dir(retroarch)
        log(f"  its cores         {cores or 'none found'}")
        if cores:
            have = [c for c in CORE_LICENCES if os.path.isfile(os.path.join(cores, f"{c}_libretro{CORE_EXT}"))]
            log(f"  of Den's defaults {len(have)}/{len(CORE_LICENCES)} installed")
    staged = find_binary(dest) if os.path.isdir(dest) else None
    log(f"  already staged    {staged or 'nothing'}")
    manifest = load_manifest() if os.path.isfile(MANIFEST) else {}
    entry = manifest.get("platforms", {}).get(platform_key(), {})
    log(f"  manifest for {platform_key():<12} {entry.get('url') or 'no url'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
