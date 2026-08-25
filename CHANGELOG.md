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
- **A keyboard scheme, written and shown from one table.** Den binds player
  one's keys in every session config and lists them on the Controllers screen
  from the same source, so the screen cannot promise a key the game does not
  answer to. Arrow keys, `Z`/`X`/`A`/`S` for the face buttons in the shape
  they sit on a pad, `Enter` and `Right Shift`, and `Escape` to get back to
  Den. It means there is a way to play before any controller is involved.
- **Pads are remembered by what they are**, not by which node they turned up
  as: vendor, product and reported name, so unplugging a pad and bringing it
  back keeps its player. Two identical pads get separate identities, in
  joystick order, so a pair of the same controller do not share one number.
- "Nobody" is a real answer for a pad, kept apart from "never seen" — without
  that difference it would be a control that does nothing, since the next look
  at the controllers would helpfully assign the pad again.
- `den-doctor` reports each pad's player and identity, and prints the keyboard
  scheme.
- **A RetroArch can be bundled inside Den.** `tools/bundle_runtime.py` stages
  one into `apps/desktop/src-tauri/resources/runtime/`, which the Tauri bundle
  ships and the shell hands to the runner as the first place it looks. Three
  sources: an archive you downloaded (an AppImage, `.zip`, `.tar.*`, `.7z`, or
  a directory), the RetroArch installed on this machine, or a download pinned
  by SHA-256 in `tools/runtime-manifest.json`. `npm run tauri build` stages one
  automatically and still succeeds when there is nothing to stage, so a build
  never depends on it. The script prints the licence of everything it stages,
  because bundling turns other people's licence terms into yours: RetroArch is
  GPLv3, and `snes9x`, `genesis_plus_gx` and `fbneo` carry non-commercial
  terms.
- **`den-doctor`** — a headless diagnostic. Every path Den tried, what was
  actually at each one, which answer it settled on, which cores are installed,
  which controllers are attached. It builds without a WebView, so it runs on a
  machine that cannot build the app.
- **A RetroArch picked by hand is kept with the library**, in a new `settings`
  table. It is checked before it is stored — a setting that does not work is
  worse than none, because it also switches the search off — and the interface
  offers the automatic search back when the chosen one stops working.
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
- **The report card broke its own vocabulary.** The eight promised words are
  lowercase — `added`, `duplicate`, `repaired` — and the pills and tally said
  `Added`, `Duplicate`, `Repaired`, because the serialized variant name leaked
  through as the word. The word is lowercase everywhere now, and reports
  already saved in the old capitalized form still read back.
- **The pad was plugged in and the game could not see it.** Den handed
  RetroArch the kernel's `jsN` number as `input_playerN_joypad_index`, but
  RetroArch's udev joypad driver fills its slots densely from 0 in its own
  attach order and never reads `jsN` numbers — so a lone controller that came
  back as `js1` (a Bluetooth reconnect, a replug with the old node still
  held open) was bound to an empty slot and dead in-game. A pad's index is
  now its rank among attached pads, dense from 0. Joydev nodes are also held
  to the same `BTN_GAMEPAD` capability check as event nodes, so an
  accelerometer or wheel on `js0` can no longer steal Player 1 or shift the
  real pad's slot.
- **A gamepad was detected and then nothing happened to it.** `player` was
  hard-coded `None`, the Controllers screen printed "Unassigned" with no
  control beside it, and no pad was ever mentioned to RetroArch at all. A pad
  is now Player 1 by the time you look at it, the screen changes that, and the
  assignment reaches RetroArch as `input_playerN_joypad_index`.
- **Detection asked what a device was called, not what it is.** The keyword
  list missed the one every Linux machine has: the kernel calls an Xbox pad
  `Microsoft X-Box 360 pad`, and `x-box` is not `xbox`. Den now takes the
  kernel's own answer — a `js` node, or `BTN_GAMEPAD` in the capability
  bitmask, which is what udev looks at — and reports pads in joystick order,
  which is the order RetroArch counts them in.
- **The runtime bundler put the binary where Den never looks.** Published
  builds unpack into a wrapper directory, and the staging directory has a
  committed README in it, so the "lift the wrapper if it is the only entry"
  rule never fired on the real case — and failed quietly, reporting a staged
  runtime that the runner could not see. Whatever directory the binary lands
  in now comes up to the top, cores and libraries with it.
- `--record` skipped hash verification entirely, so re-running it silently
  replaced a hash somebody had pinned by hand — including with the hash of a
  404 page. It now refuses a mismatch unless `--record --force` says so
  deliberately, and it pins only after the download has proved to be an
  archive with a RetroArch in it.
- `tarfile.extractall` ran with no `filter`, which on Python 3.11 trusts a
  member called `../ESCAPED` completely, and which Python 3.14 changes under
  us. Pinned to `data`.
- A download that failed left its part-file inside the tracked staging
  directory; it now goes to a temporary file that is removed either way. A
  `.tar.gz` URL was named `.gz` and handed to the wrong branch of the
  unpacker. `zipfile` dropped the executable bit off everything it wrote.
- `find_core_dir` in the bundler still took the first directory that merely
  existed — the same bug fixed in the runner.
- **`set_retroarch_path` changed the runner before writing the setting** and
  left it changed when the write failed, so a choice worked until the next
  restart and then silently did not. A non-UTF-8 path was stored lossily and
  came back pointing somewhere else; it is refused with a reason now.
- A launch whose session row could not be written dropped the process handle,
  leaking an emulator Den could never reap.
- **PlayStation 2, GameCube and Wii were blamed on a missing core.** They
  cannot be launched at all yet, so the Core Downloader would not have helped.
  Play now gives one reason, and it is the true one.
- Accessibility: the Play button carries its reason programmatically
  (`aria-describedby`) rather than merely beside it; the RetroArch notice is a
  live region, so choosing or clearing one is announced; a re-render no longer
  drops focus to `<body>` or folds the "Where Den looked" list shut under
  whoever was reading it.
- `den-doctor` created a library as a side effect of diagnosing one, and its
  closing advice omitted the only thing that fixes a stale chosen path.
- The build called `python3`, which a stock Windows Python install does not
  provide; `tools/run-python.mjs` asks the machine what it has, and a machine
  with no Python still builds, without a bundled runtime.
- **`Fatal error received in: "init_libretro_symbols()"`.** RetroArch's way of
  saying it could not load the core. Den passed `-L` a bare file name whenever
  it had not found a cores directory, and its guesses at that directory never
  asked RetroArch. Den now reads `libretro_directory` out of the person's own
  `retroarch.cfg` — the only authority on where their cores are — and **checks
  the core is there before launching**, so a missing core is named, with what
  to do about it, instead of the emulator dying of it. The Game screen says so
  before the press, and disables Play.
- **Launching through Den discarded every RetroArch setting.** `--config`
  replaces a configuration rather than adding to it, so a session ran with
  none of the person's video driver, pad bindings or shaders. Den now starts
  from their file and overrides only the handful of keys it has an opinion
  about.
- **The Flatpak and Snap wrappers were resolved into the multiplexer behind
  them.** `/var/lib/flatpak/exports/bin/org.libretro.RetroArch` is a symlink to
  `/usr/bin/flatpak`, which behaves like RetroArch only because it looks at
  the name it was invoked under; `canonicalize` threw that name away. Den
  spawned `flatpak` with a ROM appended, got a usage message, reported a
  successful launch and opened a session row — the worst kind of failure,
  the silent one. Paths are made absolute now without following symlinks.
- **The chooser rejected `/Applications/RetroArch.app`,** which is the only
  thing a macOS file dialog will hand back — so the one way out of "RetroArch
  was not found" refused the only answer that platform gives. An application
  bundle now resolves to the program inside it.
- A chosen path that worked could never be handed back to the automatic
  search: the way to undo it only appeared when it had already broken.
- "is not there any more" was also the message for a file sitting right there
  with no executable bit, which sends somebody looking in the wrong place.
- **A build re-staged the bundled runtime from the build machine**, deleting
  whatever `--from-archive` had put there — so the documented release flow
  could not produce a portable bundle. The build step keeps an existing one.
- `--from-system` would bundle `/usr/bin/flatpak` as if it were RetroArch. A
  launcher is refused, with a pointer to `--from-archive`.
- **A RetroArch that is installed could still go unfound, with no way to say
  where it is.** Three things now stand between that and a person who wants to
  play a game: the search covers more (Linux desktop entries, `/opt`, and the
  places the last round missed); **Choose RetroArch…** on the Library screen
  points Den at any binary and keeps the choice with the library; and
  `den-doctor` prints every path tried and what was at each one, so a report
  of "it still cannot find it" has an answer in it.
- A Flatpak desktop entry reads `Exec=/usr/bin/flatpak run
  org.libretro.RetroArch`, and taking the first token of that would have had
  Den launch `flatpak` itself with a ROM appended — a usage message and no
  emulator. Only a program whose own name says RetroArch is taken from a
  desktop entry; the Flatpak wrapper is already in the list under its own
  path.
- The search list deduplicated with `Vec::dedup`, which only removes
  neighbours, so overlapping sources left repeats in it. Order is the priority
  here, so it is deduplicated without sorting.
- A cores directory was taken on existence alone, so an empty
  `~/.config/retroarch/cores` — which plenty of installs have — shadowed the
  real one and wrote a `libretro_directory` pointing at nothing. A directory
  now has to hold a core to count, and the binary's own directory is checked
  last, for a bundle that flattened the staged tree.
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
- **The depth scale is softer.** The shadows and highlights that make a
  control read as raised or pressed are quieter now — smaller glow offsets,
  a lighter shade in light mode, a fainter glow in dark — so the surfaces
  keep their vocabulary (lift, press, seam) without the moulded look.
- `[lints] workspace = true` in every crate, so the `unsafe_code = "deny"`
  the workspace already declared actually applies. The workspace is
  `rustfmt`-clean and clippy-clean at `-D warnings`.
- `.gitignore` no longer ignores `src-tauri/tauri.conf.json` (as a "secret") or
  `src-tauri/icons/`, both of which are committed and the second of which the
  brand check verifies.
- Removed `Projects/`, an early scaffold superseded by `crates/`, and the
  committed `__pycache__`.

### Verified
- `cargo test --workspace` green: 94 tests across the seven crates.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` clean.
- `npm run build` (types and bundle) clean, with no unresolved assets.
- `python3 tools/check_brand.py` passes.
- All four screens rendered and driven in a real browser, light and dark.

### Known gaps
- Two findings from the review were left alone deliberately, both low:
  `choose_folder` and `choose_retroarch` open a blocking file dialog from a
  command thread, which can hold the window while it is up — moving to
  `rfd::AsyncFileDialog` changes feature flags that cannot be compiled in the
  environment this was written in, and breaking a build that works to fix a
  freeze that may not happen is the wrong trade. And `is_executable` reads the
  file's mode bits rather than asking whether *this* process may execute it,
  which differs only for a file owned by somebody else with a narrow mode.
- The URLs in `tools/runtime-manifest.json` were written without being reached:
  the environment this was built in cannot resolve that host. They are not
  pinned to a hash, and `--from-manifest` refuses to stage anything unpinned,
  so an unverified URL cannot be silently trusted. One
  `--from-manifest --record` run on a networked machine pins them.
  `--from-archive` and `--from-system` need no network and are tested.
- Cores are not installed for you. Den points RetroArch at the right core for
  a system; RetroArch downloads cores itself, and `--from-system` copies the
  ones already installed.
- A bundled `--from-system` RetroArch is linked against the libraries of the
  machine that built it. For a bundle to hand to somebody else, stage one of
  libretro's portable builds with `--from-archive`.
- `den-input` reads evdev names directly; gilrs + SDL gamecontrollerdb is the
  planned upgrade.
- The intake screen has no password field, so an encrypted archive is
  quarantined as password-protected rather than prompting for one.
- Box art is not fetched; every tile falls back to its title.
