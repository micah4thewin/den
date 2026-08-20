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

### Verified
- `cargo test --workspace` green: 38 tests across the six crates.

### Known gaps
- RetroArch is not installed on this machine; launch reports `NotFound`
  gracefully until it is.
- `den-input` reads evdev names directly; gilrs + SDL gamecontrollerdb is the
  planned upgrade.
