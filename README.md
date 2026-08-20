# Den

A quiet place to play the games you already own.

Den is a local-first launcher. Point it at a folder of downloads and it
unpacks, identifies, repairs, names, and shelves what it finds; then it boots
those games through RetroArch and remembers where you left off. Nothing leaves
the machine, nothing in the drop folder is modified, and nothing is ever
deleted.

Every file that goes in comes back with a word:

| Word | What happened |
| --- | --- |
| `added` | shelved as a new game, named from a hash match |
| `duplicate` | byte-for-byte identical to something already on the shelf |
| `repaired` | shelved after a fix — a missing `.cue` written, a `.m3u` built |
| `probable` | identified by header or extension rather than by hash |
| `bios` | a BIOS file, recognised and filed |
| `extra` | a manual, a scan, a readme, an imported save |
| `quarantined` | could not be used, with the reason and a way to retry |
| `unsupported` | a format Den does not handle |

Colour never carries any of that. Status is always a word — see
[`docs/DESIGN.md`](docs/DESIGN.md) for why, and
[`docs/design-notes.md`](docs/design-notes.md) for how Den in particular wears
the shared design system.

## Layout

```
crates/den-ident     hashing, magic bytes, the DAT index
crates/den-intake    stage, unpack, identify, repair, shelve, report
crates/den-db        SQLite (WAL): games, saves, sessions, BIOS, reports
crates/den-runner    RetroArch process control and per-session config
crates/den-input     controller detection
crates/den-core      the one object the shell talks to
crates/den-doctor    `den-doctor`: what Den can and cannot find on this machine
apps/desktop         the Tauri v2 shell: four screens over a typed IPC layer
tools/               the brand sheet, the icon generator, the runtime bundler
```

The six crates are one Cargo workspace and build headless: no WebView, no
window, no system packages. The shell is deliberately *outside* that
workspace, in `apps/desktop/src-tauri`, so the crates can be tested on any
machine and in CI without dragging platform GUI dependencies in.

## Building

The core workspace needs nothing but a Rust toolchain:

```sh
cargo test --workspace          # 78 tests, headless
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The desktop shell needs Node and, on Linux, the WebKitGTK development
packages Tauri lists for your distribution:

```sh
cd apps/desktop
npm ci
npm run build                   # types and bundle only
npm run tauri dev               # the app
npm run deb                     # a .deb
```

The application icons are generated, never hand-drawn. To change the mark,
edit `tools/brand.py`, then:

```sh
python3 tools/generate_icons.py
python3 tools/check_brand.py     # also runs in CI
```

## The emulator

Den does not emulate anything; it drives RetroArch. There are three ways that
happens, and Den takes the first one that works:

1. **Bundled inside Den.** A build made with a runtime staged (see below) has
   its own RetroArch and needs nothing installed.
2. **Chosen by hand.** The Library screen has **Choose RetroArch…** if the
   search comes up empty. The choice is kept with the library, so it holds.
   `RETROARCH=/path/to/retroarch` does the same from the environment.
3. **Found on the machine.** `PATH` under every name RetroArch goes by, the
   places each platform's installers use — including the macOS app bundle,
   `retroarch.exe`, Homebrew, Snap and Flatpak — and, on Linux, whatever the
   desktop entries point at.

If none of that finds yours, ask:

```sh
cargo run -p den-doctor
```

It prints every path Den tried, what was actually at each one, which answer it
settled on, and which libretro cores are installed. It builds headless, so it
runs without building the app.

### Bundling one into the app

```sh
cd apps/desktop
npm run runtime:check      # what is here, and what would be staged
npm run tauri build        # stages a runtime, then builds
```

`tools/bundle_runtime.py` puts a RetroArch in
`apps/desktop/src-tauri/resources/runtime/`, which the bundle ships and the
shell hands to the runner. Three sources:

| | |
| --- | --- |
| `--from-archive PATH` | an AppImage, `.zip`, `.tar.*`, `.7z`, or unpacked directory you downloaded — **use this for a release**, because those builds carry their own libraries |
| `--from-system` | the RetroArch installed here (the default): immediate, no network, but linked against *this* machine's libraries |
| `--from-manifest` | download per `tools/runtime-manifest.json`, verified against a pinned SHA-256 |

A build with nothing staged still works; it falls back to the machine. That is
why the build step passes `--allow-missing`.

**Licences matter once you bundle.** RetroArch is GPLv3. Den runs it as a
separate process, which is aggregation rather than linking, so Den's own MIT
terms are unaffected — but *distributing* a bundle carries GPLv3's
obligations, including offering the corresponding source. The cores are not
uniform: `mesen`, `mupen64plus_next` and `swanstation` are GPL, `mgba` is
MPL-2.0, and `snes9x`, `genesis_plus_gx` and `fbneo` carry non-commercial
terms that restrict redistribution. Bundling for yourself is unproblematic;
publishing a bundle means reading those terms. The script prints what it
staged and under what licence, so the question is at least visible.

**Cores are downloaded by RetroArch**, not by Den — Online Updater → Core
Downloader. `--from-system` copies the ones you already have; `den-doctor`
lists which of Den's defaults are present.

## Running it

On first launch Den creates a library under your platform's data directory
(`~/.local/share/den` on Linux) and opens on an empty shelf. Go to **Intake**,
press the drop zone or **Choose a folder…**, and point it at a folder of
downloads. Everything is copied into the library; the originals are left
exactly as they were.

**BIOS files are your own.** Den recognises and files the common ones by name
and by hash; it does not ship any.

PlayStation 2, GameCube, and Wii are shelved and named but not launched — they
need external emulator profiles that are not wired yet, and Den says so rather
than failing quietly.

## Licence

MIT.
