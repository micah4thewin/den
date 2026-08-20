# Changelog

All notable changes to Den are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
the build plan's milestones, not semver.

## [Unreleased]

### In progress
- Building the Tauri v2 desktop shell (M0 scaffold) over the already-built
  core workspace.

### Added
- `crates/den-ident` — SHA-1/CRC32 hashing, magic-byte sniffing, and the DAT
  (No-Intro/Redump-style) index. Pure, builds headless.
- `crates/den-db` — SQLite (WAL) library: games, variants, saves, sessions,
  BIOS files, and intake reports.
- `crates/den-intake` — the drop pipeline: stage, unpack (zip/7z/rar/tar/gz,
  nested, salvage corrupt archives), identify, repair (cue generation, m3u
  playlists), shelve (hash dedupe, BIOS filing, `_extras`), and report with the
  house word vocabulary (`added`, `duplicate`, `repaired`, `probable`, `bios`,
  `extra`, `quarantined`, `unsupported`).
- `crates/den-runner` — RetroArch process control and private per-session config
  generation (autosave every 60s, atomic state dirs).
- `crates/den-input` — gamepad detection from `/sys/class/input`.
- `crates/den-core` — the glue object (`Den`) the shell talks to.
- `apps/desktop/` — Tauri v2 shell scaffold: design-system port (tokens, base,
  animations, six Gilroy/Avenir faces, `createElementNS` icon table, brand
  mark), and the four screens (Library, Game, Intake, Controllers) in HTML/CSS.
- **Play sessions are recorded.** A launch opens a row in `sessions` and
  closes it when the emulator exits, which is what fills the library's
  Continue row, its Recent shelf, and a game's playtime. `Den::reap` closes
  finished sessions and reaps the child process; the shell calls it before
  every read of the library. Nothing wrote to that table before, so those
  three surfaces could never show anything.
- **Imported saves are attached to their game.** A `.srm` or state that comes
  in beside a ROM is recorded against it, not merely copied into `_extras`.
- A tally on the intake report card: the same eight words, counted.
- `README.md`, `docs/design-notes.md` (how Den wears the shared design
  system), and `.github/workflows/ci.yml` — formatting, clippy, tests, the
  frontend build, the Tauri shell, and the brand check that `docs/DESIGN.md`
  says runs in CI.
- Integration tests for the whole drop pipeline (`crates/den-intake/tests`)
  and for the glue object and its sessions (`crates/den-core/tests`).

### Fixed
- **RetroArch was only ever looked for as `retroarch` on `PATH`,** resolved
  once at startup. That misses the macOS app bundle (never on `PATH`),
  `retroarch.exe` on Windows (so Den could not launch anything there at all),
  the Flatpak build (exported as `org.libretro.RetroArch`), and any install a
  GUI app's `PATH` does not reach — on macOS that is `/usr/bin:/bin:` and two
  more, so a Homebrew RetroArch is invisible. Den now asks `PATH` under every
  name RetroArch goes by, then the places the installers actually use, and it
  asks again on every launch rather than only when the app started.
- **The core was passed to `-L` as a bare word** (`mesen_libretro`), which is
  not the name of a file on any platform. Den resolves the core to its real
  path when it can find one, falls back to the platform's file name when it
  cannot, and writes `libretro_directory` into the generated config — which
  `--config` would otherwise blank out.
- **Nothing said RetroArch was missing until you pressed Play.** The Library
  screen carries a standing notice with the reason and every path Den tried,
  and Play is disabled with a sentence beside it rather than answering a press
  with an error toast. `LibraryView` had carried a `retroarch` flag all along
  that the interface never read.
- A `RETROARCH` that points at nothing is now named as the problem instead of
  silently falling through to a `PATH` lookup nobody asked for, and a
  directory or a non-executable file called `retroarch` is not mistaken for
  one.
- **A BIOS was shelved as a PlayStation game.** Identification asked the
  extension before the name, and every bundled BIOS name ends in `.bin`, which
  also belongs to Sega CD and PlayStation. `scph1001.bin` was filed as a disc,
  handed a generated cue sheet, and listed on the shelf as a game called
  "scph1001". The name is asked first now, in both the identify path and the
  multi-disc grouping that bypassed it.
- **`magic::kinds` panicked** on any head between `0x8003` and `0x8005` bytes:
  the ISO9660 bound checked the first byte of the marker and then sliced all
  five.
- **No disc image was ever recognised by its contents.** `magic::sniff` read
  512 bytes and the ISO9660 volume descriptor sits at `0x8001`, so that branch
  could not fire; it also stopped at one `read` rather than filling the buffer.
- **`sanitize` panicked** on a title whose 120th byte fell inside a character —
  a Japanese or accented name, which is most of a real library.
- **Every disc in a multi-disc set past the first went unreported.** The
  pipeline promises a word for every input file and was quietly dropping some.
- A multi-disc set with no cue sheets recorded its game against an `.m3u` that
  was never written.
- **The Continue and Recent bands never hid.** They set the `hidden`
  attribute, and `.row-band { display: flex }` silently outranks the browser's
  rule for it, so both headings showed over an empty shelf.
- **The toast sat half its own width off its corner**, permanently: the shared
  `toast-in` keyframe ends at `translate(-50%, 0)` for a centred toast and
  carries a fill mode.
- **The Play button's glyph rendered at 0×0.** An inline SVG with a viewBox and
  no intrinsic size lays out at zero in a flex row; labelled controls now size
  their glyph the way icon buttons already did.
- **The drop zone did nothing when pressed**, though it was announced as a
  button and reachable with Tab. It opens the folder picker now, by pointer or
  by keyboard.
- The intake status pulsed through `prefers-reduced-motion`; it uses the shared
  `.working` class, which the token file already stops.
- One controller was reported once per kernel node — as `eventN` and again as
  `jsN`. Devices are keyed by the input node they resolve to.
- `unrar` was handed a destination with no trailing separator, which it reads
  as a file mask, so RAR archives extracted nothing; a missing `unrar` now says
  so instead of surfacing "No such file or directory".
- `list_games` treated `%` and `_` in a search term as wildcards.
- Intake read the whole `hash` column once per file; it reads it once per run.
- The archive walk read a directory while unpacking into it, which could queue
  the new directory twice.
- A relative `RETROARCH` override was resolved but then rejected as
  unavailable.
- `open_library_folder` waited for the file manager to exit while holding the
  library lock.
- Six `@font-face` rules asked for handwriting faces this repository does not
  ship (lockbox's whiteboard, not Den's), and `index.html` had no icon link.

### Changed
- `[lints] workspace = true` in every crate, so the `unsafe_code = "deny"` and
  `missing_docs` the workspace already declared actually apply. Public items
  are documented and the workspace is `rustfmt`-clean and clippy-clean at
  `-D warnings`.
- `.gitignore` no longer ignores `src-tauri/tauri.conf.json` (as a "secret") or
  `src-tauri/icons/`, both of which are committed and the second of which the
  brand check verifies.
- Removed `Projects/`, an early scaffold superseded by `crates/`, and the
  committed `__pycache__`.

### Verified
- `cargo test --workspace` green: 65 tests across the six crates.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` clean.
- `npm run build` (types and bundle) clean, with no unresolved assets.
- `python3 tools/check_brand.py` passes.
- All four screens rendered and driven in a real browser, light and dark.

### Known gaps
- RetroArch is not installed on this machine; launch reports it gracefully,
  and the interface says where it looked.
- Cores are not installed for you. Den points RetroArch at the right core for
  a system, but RetroArch has to have downloaded it.
- `den-input` reads evdev names directly; gilrs + SDL gamecontrollerdb is the
  planned upgrade.
- The intake screen has no password field, so an encrypted archive is
  quarantined as password-protected rather than prompting for one.
- Box art is not fetched; every tile falls back to its title.
