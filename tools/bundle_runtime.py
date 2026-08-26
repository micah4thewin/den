#!/usr/bin/env python3
"""Put a RetroArch inside Play, so a built Play needs nothing installed."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RESOURCES = os.path.join(ROOT, "apps", "desktop", "src-tauri", "resources")
RUNTIME = os.path.join(RESOURCES, "runtime")
MANIFEST = os.path.join(ROOT, "tools", "runtime-manifest.json")

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
        for var in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LOCALAPPDATA"):
            base = os.environ.get(var)
            if base:
                out += [
                    os.path.join(base, "RetroArch", "retroarch.exe"),
                    os.path.join(base, "RetroArch-Win64", "retroarch.exe"),
                    os.path.join(base, "Programs", "RetroArch", "retroarch.exe"),
                ]
        out += [
            os.path.join(home, "scoop/apps/retroarch/current/retroarch.exe"),
            "C:/RetroArch-Win64/retroarch.exe",
            "C:/RetroArch/retroarch.exe",
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
            "/opt/retroarch/retroarch",
            "/opt/RetroArch/retroarch",
        ]
    return out


def find_system_retroarch():
    """The RetroArch on this machine, as found -- symlinks left alone."""
    override = os.environ.get("RETROARCH")
    if override and os.path.isfile(override) and os.access(override, os.X_OK):
        return override
    for candidate in system_candidates():
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    return None


def is_a_wrapper(path):
    """Whether this is a launcher that starts RetroArch rather than RetroArch."""
    target = os.path.basename(os.path.realpath(path)).lower()
    return target in ("flatpak", "snap")


def config_and_data_dirs(home):
    if sys.platform == "win32":
        base = os.environ.get("APPDATA")
        return base, base
    if sys.platform == "darwin":
        base = os.path.join(home, "Library", "Application Support")
        return base, base
    return (
        os.environ.get("XDG_CONFIG_HOME") or os.path.join(home, ".config"),
        os.environ.get("XDG_DATA_HOME") or os.path.join(home, ".local", "share"),
    )


def find_core_dir(retroarch):
    """The cores directory that goes with a RetroArch, if there is one."""
    home = os.path.expanduser("~")
    parent = os.path.dirname(retroarch)
    config_dir, data_dir = config_and_data_dirs(home)
    candidates = [
        os.path.join(parent, "cores"),
        os.path.join(os.path.dirname(parent), "Resources", "cores"),
        os.path.join(home, ".config/retroarch/cores"),
        os.path.join(home, ".var/app/org.libretro.RetroArch/config/retroarch/cores"),
        os.path.join(home, "snap/retroarch/current/.config/retroarch/cores"),
        os.path.join(home, "Library/Application Support/RetroArch/cores"),
    ]
    if config_dir:
        candidates.append(os.path.join(config_dir, "retroarch", "cores"))
    if data_dir:
        candidates.append(os.path.join(data_dir, "RetroArch", "cores"))
    candidates += [
        "/usr/lib/libretro",
        "/usr/local/lib/libretro",
        "/usr/lib/x86_64-linux-gnu/libretro",
        parent,
    ]
    for candidate in candidates:
        if os.path.isdir(candidate) and any(
            name.endswith(CORE_EXT) and "_libretro" in name
            for name in os.listdir(candidate)
        ):
            return candidate
    return None


def extract(archive, into):
    """Unpack `archive` into `into`, whatever shape it arrived in."""
    lower = archive.lower()
    if os.path.isdir(archive):
        shutil.copytree(archive, into, dirs_exist_ok=True)
        return
    if lower.endswith(".appimage"):
        os.makedirs(into, exist_ok=True)
        target = os.path.join(into, "retroarch")
        shutil.copy2(archive, target)
        os.chmod(target, 0o755)
        return
    if lower.endswith(".zip"):
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(into)
            for member in zf.infolist():
                mode = member.external_attr >> 16
                if mode & 0o111:
                    target = os.path.join(into, member.filename)
                    if os.path.isfile(target):
                        os.chmod(target, os.stat(target).st_mode | 0o755)
        return
    if any(lower.endswith(s) for s in (".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2")):
        with tarfile.open(archive) as tf:
            try:
                tf.extractall(into, filter="data")
            except TypeError:
                for member in tf.getmembers():
                    if member.name.startswith(("/", "..")) or ".." in member.name.split("/"):
                        raise SystemExit(f"{archive} contains an unsafe path: {member.name}")
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


def promote(into):
    """Lift the binary and everything beside it to the top of `into`."""
    binary = find_binary(into)
    if not binary:
        return
    home = os.path.dirname(binary)
    if os.path.abspath(home) != os.path.abspath(into):
        for name in os.listdir(home):
            source = os.path.join(home, name)
            target = os.path.join(into, name)
            if os.path.exists(target):
                shutil.rmtree(target) if os.path.isdir(target) else os.remove(target)
            shutil.move(source, target)
    for name in list(os.listdir(into)):
        path = os.path.join(into, name)
        if not os.path.isdir(path):
            continue
        if name == "__MACOSX" or not os.listdir(path):
            shutil.rmtree(path, ignore_errors=True)


def find_binary(root):
    """The RetroArch inside a staged directory."""
    for dirpath, _dirnames, filenames in os.walk(root):
        for name in BINARY_NAMES:
            if name in filenames:
                return os.path.join(dirpath, name)
    return None


def stage_from_system(dest, want_cores):
    retroarch = find_system_retroarch()
    if not retroarch:
        raise SystemExit(
            "No RetroArch found on this machine.\n"
            "Install one, or use --from-archive with a download."
        )
    if sys.platform == "darwin" and ".app/Contents/MacOS/" in retroarch:
        bundle = retroarch.split("/Contents/MacOS/")[0]
        log(f"  bundle   {bundle}")
        shutil.copytree(bundle, os.path.join(dest, os.path.basename(bundle)), symlinks=True)
        inner = os.path.join(
            dest, os.path.basename(bundle), "Contents", "MacOS", os.path.basename(retroarch)
        )
        os.chmod(inner, 0o755)
        log("  note     a copied .app may need re-signing to run on another Mac")
        return inner
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
        log(f"           {missing} of Play's default cores were not installed here")
    return copied


def load_manifest(path=None):
    path = path or MANIFEST
    if not os.path.isfile(path):
        raise SystemExit(f"no manifest at {path}")
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def platform_key():
    machine = (os.uname().machine if hasattr(os, "uname") else "x86_64").lower()
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}
    return f"{sys.platform}-{arch.get(machine, machine)}"


def archive_suffix(url):
    """The suffix a URL's file has, `.tar.gz` included."""
    name = os.path.basename(url.split("?")[0]).lower()
    for suffix in (".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".7z", ".zip", ".appimage", ".tar"):
        if name.endswith(suffix):
            return suffix
    return os.path.splitext(name)[1]


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def stage_from_manifest(dest, want_cores, record, force, manifest_path):
    import urllib.request

    manifest = load_manifest(manifest_path)
    key = platform_key()
    entry = manifest.get("platforms", {}).get(key)
    if not entry or not entry.get("url"):
        raise SystemExit(
            f"The manifest has no download for {key}.\n"
            f"Add one to {os.path.relpath(manifest_path, ROOT)}, or use --from-archive."
        )
    url = entry["url"]
    expected = entry.get("sha256")
    log(f"  fetching {url}")

    handle, tmp = tempfile.mkstemp(prefix="den-runtime-", suffix=archive_suffix(url))
    os.close(handle)
    try:
        with urllib.request.urlopen(url, timeout=120) as response, open(tmp, "wb") as out:
            shutil.copyfileobj(response, out)
        got = sha256_of(tmp)

        if expected and expected != got and not (record and force):
            raise SystemExit(
                f"sha256 mismatch\n  expected {expected}\n  got      {got}\n"
                "The file at that URL is not the one this manifest was pinned to.\n"
                "If the change is expected, re-pin deliberately: --record --force."
            )
        if not expected and not record:
            raise SystemExit(
                f"The manifest has no sha256 for {key}. Re-run with --record to pin\n"
                "the hash of what you just downloaded, having satisfied yourself it\n"
                "is the right file."
            )

        extract(tmp, dest)
        promote(dest)
        binary = find_binary(dest)
        if not binary:
            raise SystemExit(f"no RetroArch binary inside {url}")
        os.chmod(binary, 0o755)

        if record and got != expected:
            entry["sha256"] = got
            manifest.setdefault("platforms", {})[key] = entry
            with open(manifest_path, "w", encoding="utf-8") as out:
                json.dump(manifest, out, indent=2, sort_keys=True)
                out.write("\n")
            was = f" (was {expected})" if expected else ""
            log(f"  recorded sha256 {got}{was}")
    finally:
        if os.path.exists(tmp):
            os.remove(tmp)

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
    parser.add_argument("--force", action="store_true", help="with --record, replace a hash that is already pinned")
    parser.add_argument("--manifest", default=MANIFEST, help="the manifest to read (default: tools/runtime-manifest.json)")
    parser.add_argument("--check", action="store_true", help="say what is here and what would be staged, and change nothing")
    parser.add_argument("--allow-missing", action="store_true", help="exit 0 when there is nothing to stage (for builds)")
    parser.add_argument("--keep-existing", action="store_true", help="leave an already-staged runtime alone (for builds)")
    args = parser.parse_args()

    if args.check:
        return check(args.into, args.manifest)

    dest = args.into
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
            promote(dest)
            binary = find_binary(dest)
            if not binary:
                raise SystemExit(f"no RetroArch binary inside {args.from_archive}")
            os.chmod(binary, 0o755)
        elif args.from_manifest:
            binary = stage_from_manifest(
                dest, not args.no_cores, args.record, args.force, args.manifest
            )
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
    log("This build of Play will prefer it over anything installed on the machine.")
    log("RetroArch is GPLv3; see the note at the top of this script before you")
    log("hand the bundle to anybody else.")
    return 0


def clear(dest):
    """Empty the staging directory, keeping the README that documents it."""
    if not os.path.isdir(dest):
        return
    for name in os.listdir(dest):
        if name == "README.md":
            continue
        path = os.path.join(dest, name)
        shutil.rmtree(path) if os.path.isdir(path) else os.remove(path)


def check(dest, manifest_path=None):
    log("What Play would bundle")
    retroarch = find_system_retroarch()
    log(f"  on this machine   {retroarch or 'nothing found'}")
    if retroarch:
        cores = find_core_dir(retroarch)
        log(f"  its cores         {cores or 'none found'}")
        if cores:
            have = [c for c in CORE_LICENCES if os.path.isfile(os.path.join(cores, f"{c}_libretro{CORE_EXT}"))]
            log(f"  of Play's defaults {len(have)}/{len(CORE_LICENCES)} installed")
    staged = find_binary(dest) if os.path.isdir(dest) else None
    log(f"  already staged    {staged or 'nothing'}")
    manifest_path = manifest_path or MANIFEST
    manifest = load_manifest(manifest_path) if os.path.isfile(manifest_path) else {}
    entry = manifest.get("platforms", {}).get(platform_key(), {})
    log(f"  manifest for {platform_key():<12} {entry.get('url') or 'no url'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
