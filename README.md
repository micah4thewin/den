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
apps/desktop         the Tauri v2 shell: four screens over a typed IPC layer
tools/               the brand sheet and the icon generator
```

The six crates are one Cargo workspace and build headless: no WebView, no
window, no system packages. The shell is deliberately *outside* that
workspace, in `apps/desktop/src-tauri`, so the crates can be tested on any
machine and in CI without dragging platform GUI dependencies in.

## Building

The core workspace needs nothing but a Rust toolchain:

```sh
cargo test --workspace          # 60 tests, headless
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

## Running it

On first launch Den creates a library under your platform's data directory
(`~/.local/share/den` on Linux) and opens on an empty shelf. Go to **Intake**,
press the drop zone or **Choose a folder…**, and point it at a folder of
downloads. Everything is copied into the library; the originals are left
exactly as they were.

Two things are worth knowing:

- **RetroArch is not bundled.** Install it yourself and put it on `PATH`, or
  set `RETROARCH` to the binary. Until then the library still works and
  **Play** says plainly that it cannot find it.
- **BIOS files are your own.** Den recognises and files the common ones by
  name and by hash; it does not ship any.

PlayStation 2, GameCube, and Wii are shelved and named but not launched — they
need external emulator profiles that are not wired yet, and Den says so rather
than failing quietly.

## Licence

MIT.
