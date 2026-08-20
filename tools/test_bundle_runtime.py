#!/usr/bin/env python3
"""Exercise tools/bundle_runtime.py end to end, with a stand-in RetroArch.

    python3 tools/test_bundle_runtime.py

Nothing here touches the network or a real RetroArch: it plants a script that
answers to the name, then checks that every way of staging one lands a
runnable binary where den-runner looks for it. The download path is driven
against a local server, so the hash check is exercised without depending on
anybody's buildbot being up.
"""

import http.server
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(ROOT, "tools", "bundle_runtime.py")
FAILURES = []


def check(name, condition, detail=""):
    if condition:
        print(f"  ok    {name}")
    else:
        print(f"  FAIL  {name}  {detail}")
        FAILURES.append(name)


def run(args, env=None, expect=0):
    result = subprocess.run(
        [sys.executable, SCRIPT] + args,
        capture_output=True,
        text=True,
        env={**os.environ, **(env or {})},
    )
    if result.returncode != expect:
        print(result.stdout)
        print(result.stderr)
    return result


def plant_retroarch(directory, name="retroarch"):
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, name)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("#!/bin/sh\nexit 0\n")
    os.chmod(path, 0o755)
    return path


def plant_cores(directory, cores=("mesen", "mupen64plus_next", "snes9x")):
    os.makedirs(directory, exist_ok=True)
    ext = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")
    for core in cores:
        with open(os.path.join(directory, f"{core}_libretro{ext}"), "wb") as handle:
            handle.write(b"not really a core")
    return directory


def staged_binary(dest):
    for dirpath, _dirs, files in os.walk(dest):
        for name in ("retroarch", "retroarch.exe"):
            if name in files:
                return os.path.join(dirpath, name)
    return None


def test_from_system(tmp):
    fake_bin = os.path.join(tmp, "fakeprefix", "bin")
    plant_retroarch(fake_bin)
    plant_cores(os.path.join(tmp, "fakeprefix", "bin", "cores"))
    dest = os.path.join(tmp, "staged-system")

    result = run(
        ["--from-system", "--into", dest],
        env={"PATH": fake_bin + os.pathsep + os.environ.get("PATH", ""), "RETROARCH": ""},
    )
    binary = staged_binary(dest)
    check("--from-system stages a binary", binary is not None, result.stderr)
    check("  and it is executable", binary and os.access(binary, os.X_OK))
    ext = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")
    check(
        "  and it brings the cores",
        os.path.isfile(os.path.join(dest, "cores", f"mesen_libretro{ext}")),
    )
    check("  and it says which licence each is under", "GPLv3" in result.stdout, result.stdout)


def test_from_archive_zip(tmp):
    src = os.path.join(tmp, "ra-zip-src", "RetroArch-Linux")
    plant_retroarch(src)
    archive = os.path.join(tmp, "RetroArch.zip")
    with zipfile.ZipFile(archive, "w") as zf:
        zf.write(os.path.join(src, "retroarch"), "RetroArch-Linux/retroarch")
    dest = os.path.join(tmp, "staged-zip")
    result = run(["--from-archive", archive, "--into", dest])
    binary = staged_binary(dest)
    check("--from-archive unpacks a zip", binary is not None, result.stderr)
    # The wrapper directory inside the zip is lifted away.
    check(
        "  and lifts the wrapper directory",
        binary == os.path.join(dest, "retroarch"),
        f"got {binary}",
    )


def test_from_archive_wrapper_with_siblings(tmp):
    """The shape a real build actually has: a wrapper directory, plus debris.

    This is the case the destination's own committed README used to break:
    with anything else present, the wrapper was never lifted and the binary
    landed where den-runner does not look.
    """
    src = os.path.join(tmp, "wrapper-src", "RetroArch-Linux-x86_64")
    plant_retroarch(src)
    plant_cores(os.path.join(src, "cores"))
    archive = os.path.join(tmp, "RetroArch-wrapped.zip")
    with zipfile.ZipFile(archive, "w") as zf:
        zf.write(os.path.join(src, "retroarch"), "RetroArch-Linux-x86_64/retroarch")
        ext = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")
        zf.write(
            os.path.join(src, "cores", f"mesen_libretro{ext}"),
            f"RetroArch-Linux-x86_64/cores/mesen_libretro{ext}",
        )
        # What a zip built on macOS carries alongside.
        zf.writestr("__MACOSX/._retroarch", b"junk")

    dest = os.path.join(tmp, "staged-wrapper-dir")
    os.makedirs(dest, exist_ok=True)
    with open(os.path.join(dest, "README.md"), "w", encoding="utf-8") as handle:
        handle.write("# committed\n")

    result = run(["--from-archive", archive, "--into", dest])
    binary = staged_binary(dest)
    check("a wrapped build lands at the top level", binary == os.path.join(dest, "retroarch"), f"got {binary}")
    ext = {"win32": ".dll", "darwin": ".dylib"}.get(sys.platform, ".so")
    check(
        "  and its cores travel with it",
        os.path.isfile(os.path.join(dest, "cores", f"mesen_libretro{ext}")),
        str(os.listdir(dest)),
    )
    check("  and the macOS sidecar is swept up", not os.path.isdir(os.path.join(dest, "__MACOSX")))
    check("  and the committed README survives", os.path.isfile(os.path.join(dest, "README.md")))


def test_tar_traversal_is_refused(tmp):
    """A tarball that writes outside the destination must not be unpacked."""
    outside = os.path.join(tmp, "ESCAPED")
    evil = os.path.join(tmp, "evil.tar.gz")
    payload = os.path.join(tmp, "payload")
    with open(payload, "w", encoding="utf-8") as handle:
        handle.write("owned")
    with tarfile.open(evil, "w:gz") as tf:
        tf.add(payload, "../ESCAPED")

    dest = os.path.join(tmp, "staged-evil")
    run(["--from-archive", evil, "--into", dest], expect=1)
    check("a tar member escaping the destination is refused", not os.path.exists(outside))


def test_from_archive_tar(tmp):
    src = os.path.join(tmp, "ra-tar-src")
    binary = plant_retroarch(src)
    archive = os.path.join(tmp, "RetroArch.tar.gz")
    with tarfile.open(archive, "w:gz") as tf:
        tf.add(binary, "retroarch")
    dest = os.path.join(tmp, "staged-tar")
    result = run(["--from-archive", archive, "--into", dest])
    check("--from-archive unpacks a tar.gz", staged_binary(dest) is not None, result.stderr)


def test_from_archive_appimage(tmp):
    appimage = os.path.join(tmp, "RetroArch-Linux-x86_64.AppImage")
    with open(appimage, "wb") as handle:
        handle.write(b"\x7fELF fake appimage")
    dest = os.path.join(tmp, "staged-appimage")
    result = run(["--from-archive", appimage, "--into", dest])
    binary = staged_binary(dest)
    check("--from-archive takes an AppImage whole", binary is not None, result.stderr)
    check("  and makes it executable", binary and os.access(binary, os.X_OK))


def test_from_archive_directory(tmp):
    src = os.path.join(tmp, "ra-dir")
    plant_retroarch(src)
    dest = os.path.join(tmp, "staged-dir")
    result = run(["--from-archive", src, "--into", dest])
    check("--from-archive takes an unpacked directory", staged_binary(dest) is not None, result.stderr)


def platform_key():
    """Exactly what the script computes, imported rather than re-derived."""
    sys.path.insert(0, os.path.join(ROOT, "tools"))
    import bundle_runtime

    return bundle_runtime.platform_key()


def test_manifest_download(tmp):
    """The download path, its hash check, and its refusal to trust an unpinned file.

    Driven against a local server and a throwaway manifest: the tracked
    `tools/runtime-manifest.json` is never written to, so an interrupted run
    cannot leave the repository dirty.
    """
    served = os.path.join(tmp, "served")
    os.makedirs(served, exist_ok=True)
    payload = os.path.join(served, "RetroArch.zip")
    src = os.path.join(tmp, "ra-served", "RetroArch-Linux-x86_64")
    plant_retroarch(src)
    with zipfile.ZipFile(payload, "w") as zf:
        # A wrapper directory, as every real build has.
        zf.write(os.path.join(src, "retroarch"), "RetroArch-Linux-x86_64/retroarch")

    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=served, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/RetroArch.zip"

    manifest_path = os.path.join(tmp, "manifest.json")
    key = platform_key()

    def write_manifest(sha):
        with open(manifest_path, "w", encoding="utf-8") as handle:
            json.dump({"platforms": {key: {"url": url, "sha256": sha}}}, handle)

    def read_manifest():
        with open(manifest_path, encoding="utf-8") as handle:
            return json.load(handle)["platforms"][key]["sha256"]

    try:
        dest = os.path.join(tmp, "staged-manifest")

        write_manifest(None)
        result = run(["--from-manifest", "--into", dest, "--manifest", manifest_path], expect=1)
        check(
            "--from-manifest refuses an unpinned download",
            result.returncode == 1 and "sha256" in (result.stdout + result.stderr),
            result.stdout + result.stderr,
        )

        result = run(["--from-manifest", "--into", dest, "--manifest", manifest_path, "--record"])
        check("--record pins the hash and stages", staged_binary(dest) is not None, result.stderr)
        check("  and writes the hash into the manifest", bool(read_manifest()), str(read_manifest()))
        # The wrapper directory has to be lifted, or den-runner never sees it.
        check(
            "  and the binary lands where den-runner looks",
            staged_binary(dest) == os.path.join(dest, "retroarch"),
            f"got {staged_binary(dest)}",
        )

        result = run(["--from-manifest", "--into", dest, "--manifest", manifest_path])
        check("  and a pinned download verifies", staged_binary(dest) is not None, result.stderr)

        write_manifest("0" * 64)
        result = run(["--from-manifest", "--into", dest, "--manifest", manifest_path], expect=1)
        check(
            "  and a wrong hash is refused",
            result.returncode == 1 and "mismatch" in (result.stdout + result.stderr),
            result.stdout + result.stderr,
        )

        # --record must not quietly replace a hash somebody pinned by hand.
        result = run(
            ["--from-manifest", "--into", dest, "--manifest", manifest_path, "--record"], expect=1
        )
        check(
            "  and --record alone will not overwrite an existing pin",
            result.returncode == 1 and "mismatch" in (result.stdout + result.stderr),
            result.stdout + result.stderr,
        )
        check("  leaving the pinned hash untouched", read_manifest() == "0" * 64, read_manifest())

        result = run(
            ["--from-manifest", "--into", dest, "--manifest", manifest_path, "--record", "--force"]
        )
        check("  but --record --force re-pins deliberately", result.returncode == 0, result.stderr)
        check("  and records the new hash", read_manifest() != "0" * 64, read_manifest())
    finally:
        server.shutdown()


def test_download_failure_leaves_nothing_behind(tmp):
    """A 404 must not pin a hash or leave a part-file in the tracked tree."""
    served = os.path.join(tmp, "served-empty")
    os.makedirs(served, exist_ok=True)
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=served, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/not-a-real-build.zip"

    manifest_path = os.path.join(tmp, "manifest-404.json")
    key = platform_key()
    with open(manifest_path, "w", encoding="utf-8") as handle:
        json.dump({"platforms": {key: {"url": url, "sha256": None}}}, handle)

    dest = os.path.join(tmp, "staged-404")
    try:
        result = run(
            ["--from-manifest", "--into", dest, "--manifest", manifest_path, "--record"], expect=1
        )
        check("a failed download does not stage", result.returncode != 0, result.stdout)
        with open(manifest_path, encoding="utf-8") as handle:
            pinned = json.load(handle)["platforms"][key]["sha256"]
        check("  and does not pin a hash for it", pinned is None, str(pinned))
        leftovers = os.listdir(dest) if os.path.isdir(dest) else []
        check("  and leaves no part-file behind", leftovers in ([], ["README.md"]), str(leftovers))
    finally:
        server.shutdown()


def test_allow_missing(tmp):
    """A build on a machine with no RetroArch must not fail the build."""
    dest = os.path.join(tmp, "staged-none")
    empty = os.path.join(tmp, "empty-path")
    os.makedirs(empty, exist_ok=True)
    result = run(
        ["--from-system", "--into", dest, "--allow-missing"],
        env={"PATH": empty, "RETROARCH": "", "HOME": os.path.join(tmp, "empty-home")},
    )
    check("--allow-missing exits cleanly with nothing to stage", result.returncode == 0, result.stderr)
    check(
        "  and leaves nothing half-staged",
        not os.path.isdir(dest) or os.listdir(dest) in ([], ["README.md"]),
        str(os.listdir(dest) if os.path.isdir(dest) else None),
    )


def test_keep_existing(tmp):
    """A build must not destroy a runtime staged on purpose for a release."""
    dest = os.path.join(tmp, "staged-keep")
    os.makedirs(dest, exist_ok=True)
    portable = plant_retroarch(dest)
    with open(portable, "w", encoding="utf-8") as handle:
        handle.write("#!/bin/sh\n# the portable one\n")
    os.chmod(portable, 0o755)

    other = os.path.join(tmp, "keep-prefix", "bin")
    plant_retroarch(other)
    result = run(
        ["--from-system", "--into", dest, "--keep-existing", "--allow-missing"],
        env={"PATH": other + os.pathsep + os.environ.get("PATH", ""), "RETROARCH": ""},
    )
    check("--keep-existing leaves a staged runtime alone", result.returncode == 0, result.stderr)
    with open(portable, encoding="utf-8") as handle:
        kept = "the portable one" in handle.read()
    check("  and does not restage over it", kept)


def test_wrapper_is_refused(tmp):
    """A Flatpak/Snap wrapper must not be bundled as if it were RetroArch."""
    real = os.path.join(tmp, "wrapper-real", "flatpak")
    os.makedirs(os.path.dirname(real), exist_ok=True)
    with open(real, "w", encoding="utf-8") as handle:
        handle.write("#!/bin/sh\nexit 1\n")
    os.chmod(real, 0o755)

    exports = os.path.join(tmp, "wrapper-exports")
    os.makedirs(exports, exist_ok=True)
    os.symlink(real, os.path.join(exports, "org.libretro.RetroArch"))

    dest = os.path.join(tmp, "staged-wrapper")
    result = run(
        ["--from-system", "--into", dest],
        env={"PATH": exports + os.pathsep + os.environ.get("PATH", ""), "RETROARCH": ""},
        expect=1,
    )
    out = result.stdout + result.stderr
    check("a launcher is not bundled as RetroArch", result.returncode == 1, out)
    check("  and the message says to use --from-archive", "--from-archive" in out, out)


def test_readme_survives(tmp):
    """The committed README that keeps the directory in a fresh clone."""
    dest = os.path.join(tmp, "staged-readme")
    os.makedirs(dest, exist_ok=True)
    readme = os.path.join(dest, "README.md")
    with open(readme, "w", encoding="utf-8") as handle:
        handle.write("# committed\n")

    fake_bin = os.path.join(tmp, "readme-prefix", "bin")
    plant_retroarch(fake_bin)
    run(
        ["--from-system", "--into", dest],
        env={"PATH": fake_bin + os.pathsep + os.environ.get("PATH", ""), "RETROARCH": ""},
    )
    check("staging keeps the committed README", os.path.isfile(readme))
    check("  and still stages the binary", staged_binary(dest) is not None)


def main():
    print("bundle_runtime")
    with tempfile.TemporaryDirectory() as tmp:
        test_from_system(tmp)
        test_from_archive_zip(tmp)
        test_from_archive_wrapper_with_siblings(tmp)
        test_tar_traversal_is_refused(tmp)
        test_from_archive_tar(tmp)
        test_from_archive_appimage(tmp)
        test_from_archive_directory(tmp)
        test_manifest_download(tmp)
        test_download_failure_leaves_nothing_behind(tmp)
        test_allow_missing(tmp)
        test_readme_survives(tmp)
        test_keep_existing(tmp)
        test_wrapper_is_refused(tmp)
    if FAILURES:
        print(f"\n{len(FAILURES)} failed")
        return 1
    print("\nall good")
    return 0


if __name__ == "__main__":
    sys.exit(main())
