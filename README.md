# Play

*(This is the `den` repository; the program it builds now ships under the name **Play**.)*

A quiet place to play the games you already own.

Play is a local-first launcher. Point it at a folder of downloads and it
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
| `unsupported` | a format Play does not handle |

Colour never carries any of that. Status is always a word — see
[`docs/DESIGN.md`](docs/DESIGN.md) for why, and
[`docs/design-notes.md`](docs/design-notes.md) for how Play in particular wears
the shared design system.

## Layout

```
crates/den-ident     hashing, magic bytes, the DAT index
crates/den-intake    stage, unpack, identify, repair, shelve, report
crates/den-db        SQLite (WAL): games, saves, sessions, BIOS, reports
crates/den-runner    RetroArch process control and per-session config
crates/den-input     controller detection
crates/den-core      the one object the shell talks to
crates/den-web       the remote: the shelf in any browser on your network or tailnet
crates/den-doctor    `den-doctor`: what Play can and cannot find on this machine
apps/desktop         the Tauri v2 shell: four screens over a typed IPC layer
tools/               the brand sheet, the icon generator, the runtime bundler
```

The eight crates are one Cargo workspace and build headless: no WebView, no
window, no system packages. The shell is deliberately *outside* that
workspace, in `apps/desktop/src-tauri`, so the crates can be tested on any
machine and in CI without dragging platform GUI dependencies in.

## Building

The core workspace needs nothing but a Rust toolchain:

```sh
cargo test --workspace          # 130 tests, headless
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

`npm run tauri build` asks for the `.deb` and the AppImage together. The
AppImage step runs `linuxdeploy`, which is not a system package — Tauri
downloads it into `~/.cache/tauri` and runs it, and since it is itself an
AppImage it needs FUSE 2: `sudo apt install libfuse2t64` on Ubuntu 24.04
(`libfuse2` on older releases), plus `patchelf`, `file`, and
`librsvg2-dev`. When that step fails with "failed to run linuxdeploy",
the `.deb` has already been written and installs on its own; a
half-downloaded tool fails the same way, and deleting `~/.cache/tauri`
makes the next build fetch it fresh. `npm run deb` skips the AppImage
entirely. One more note for machines that installed a build from before
the rename: the package used to be called `den` and is now `play`, and
both carry `/usr/bin/den-desktop`, so `sudo apt remove den` clears the
way for the first `play` install.

The application icons are generated, never hand-drawn. To change the mark,
edit `tools/brand.py`, then:

```sh
python3 tools/generate_icons.py
python3 tools/check_brand.py     # also runs in CI
```

## The emulator

Play does not emulate anything; it drives RetroArch. There are three ways that
happens, and Play takes the first one that works:

1. **Bundled inside Play.** A build made with a runtime staged (see below) has
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

It prints every path Play tried, what was actually at each one, which answer it
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
| `--from-manifest` | download per `tools/runtime-manifest.json`. The manifest ships with no SHA-256 pinned, and the bundler refuses an unpinned entry: run once with `--record` to pin the hash of what you downloaded, and every later run verifies against it |

A build with nothing staged still works; it falls back to the machine. That is
why the build step passes `--allow-missing`.

**Licences matter once you bundle.** RetroArch is GPLv3. Play runs it as a
separate process, which is aggregation rather than linking, so Play's own MIT
terms are unaffected — but *distributing* a bundle carries GPLv3's
obligations, including offering the corresponding source. The cores are not
uniform: `mesen`, `mupen64plus_next` and `swanstation` are GPL, `mgba` is
MPL-2.0, and `snes9x`, `genesis_plus_gx` and `fbneo` carry non-commercial
terms that restrict redistribution. Bundling for yourself is unproblematic;
publishing a bundle means reading those terms. The script prints what it
staged and under what licence, so the question is at least visible.

**Cores are downloaded by RetroArch**, not by Play — Online Updater → Core
Downloader. `--from-system` copies the ones you already have; `den-doctor`
lists which of Play's defaults are present.

## Controls

A gamepad you plug in is **Player 1** by the time you look at it — Play asks
the kernel what the device is rather than matching its name, assigns the
lowest free player, and remembers that pad by vendor, product and name so it
keeps its number when you unplug it and bring it back. The Controllers screen
changes any of it; giving a player to a pad that already has one swaps them.

There is always a keyboard, whether or not a pad turned up:

| | | | | |
| --- | --- | --- | --- | --- |
| D-pad | arrow keys | | Start | <kbd>Enter</kbd> |
| B / A | <kbd>Z</kbd> <kbd>X</kbd> | | Select | <kbd>Right Shift</kbd> |
| Y / X | <kbd>A</kbd> <kbd>S</kbd> | | RetroArch menu | <kbd>F1</kbd> |
| L / R | <kbd>Q</kbd> <kbd>W</kbd> | | Quit back to Play | <kbd>Esc</kbd> |
| Save / load state | <kbd>F2</kbd> <kbd>F4</kbd> | | Fullscreen | <kbd>F11</kbd> |

Play writes these into the config it hands RetroArch and shows the same table
on the Controllers screen, both from one table in `den-runner`, so what is on
screen is what the keys do. Everything else in your RetroArch configuration
is inherited untouched.

## From the couch

While the desktop app is open it also serves the shelf to every device on
your network, at **port 5555** — the same idea as its sibling Watch on 7777,
Chat on 8888, and Write on 9999. Open `http://<the machine's address>:5555`
on a phone or tablet: filter the shelf, narrow it to one system, tap a game
to see what it is, and start it on the machine the library lives on — the one
plugged into the TV. What is playing sits at the top of the page with a
**Stop** beside it, so getting back out of a game does not mean getting up.

Stopping asks RetroArch to quit rather than killing it, the same shutdown
<kbd>Esc</kbd> gives, so the save is written on the way out.

**The phone is the remote, not the screen.** The game runs on the machine the
library lives on and comes out of that machine's television. Play does not
stream video, and the page cannot press buttons — a controller or the keyboard
does that, at the machine.

The Library screen lists every address the remote answers on, each said with
where it works, and the same lines go to the log at startup.

### Over Tailscale

If the machine is on a tailnet, the remote is already on it: it listens on
every interface, and Play finds and prints the `100.x.y.z` address alongside
the one on your house network. From a phone signed in to the same tailnet,
`http://100.x.y.z:5555` reaches the shelf from anywhere. MagicDNS short names
work too — `http://<machine>:5555`.

For the better version of it, put the remote behind Tailscale's own HTTPS:

```sh
tailscale serve --bg 5555
```

That publishes it at `https://<machine>.<tailnet>.ts.net`, which is a secure
origin — so Android will offer to install the shelf properly rather than
bookmark it (see below), and the address is a name you can remember.

To keep the remote *only* on the tailnet, bind it to the tailnet address:
`DEN_WEB_BIND=100.x.y.z`, or `DEN_WEB_BIND=127.0.0.1` with `tailscale serve`
in front of it.

### On the phone

Open the shelf in Chrome and use **Add to Home Screen**. Play serves a web app
manifest and its own icons — including maskable ones, so Android's launcher
shapes the mark instead of putting it in a white box — and it lands beside
your other apps.

How far that goes depends on the address. A browser only installs an app, and
only runs a service worker, on a secure origin, so:

- **`http://<address>:5555`** — the home-screen icon is a shortcut that opens
  in a browser tab. Everything works; it is a tab.
- **`https://<machine>.<tailnet>.ts.net`**, behind `tailscale serve` — a real
  install: no browser chrome, and the worker keeps the app's own page, so it
  still opens when the machine is asleep and says it cannot reach the shelf
  rather than failing to load at all. The worker never caches the shelf
  itself, only the page around it, so what you see is always what the machine
  says right now.

### Who may talk to it

The remote has no password: anything already on the network may read the shelf
and start or stop what is on it, the same trust the family's media server
extends. Two things are refused, and both cost one header to check:

- **A name that is not ours.** Anyone can point a public name at a private
  address and serve you a page the browser then treats as Play's own. Play
  answers to addresses, to names with no dot in them, and to `.ts.net`,
  `.local`, `.internal` and `.lan`. `DEN_WEB_HOSTS=games.example.com` adds
  your own.
- **A press from somebody else's page.** A tab open on an unrelated site can
  post to a private address without being able to read the answer, which is
  enough to start a game on your television. Anything that changes something
  has to come from Play's own page.

`DEN_WEB_PORT` moves the remote, `DEN_WEB_PORT=0` turns it off, and
`DEN_WEB_BIND` narrows what it listens on.

### Working on it

The remote's page is the one part of Play that needs a browser to look at,
and the desktop shell needs system packages a headless machine will not have.
It can be served on its own, from the same code the app runs:

```sh
cargo run -p den-web --example shelf              # the real library
cargo run -p den-web --example shelf -- /some/den # another one
```

## Running it

On first launch Play creates a library under your platform's data directory
(`~/.local/share/den` on Linux) and opens on an empty shelf. Go to **Intake**,
press the drop zone or **Choose a folder…**, and point it at a folder of
downloads. Everything is copied into the library; the originals are left
exactly as they were.

**BIOS files are your own.** Play recognises and files the common ones by name
and by hash; it does not ship any.

PlayStation 2, GameCube, and Wii are shelved and named but not launched — they
need external emulator profiles that are not wired yet, and Play says so rather
than failing quietly.

## Licence

MIT.
