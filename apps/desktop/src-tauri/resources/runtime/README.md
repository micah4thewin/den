# The bundled runtime

This directory is where `tools/bundle_runtime.py` stages a RetroArch, and
`tauri.conf.json` ships whatever is in it as a bundle resource. A build with
something here is self-contained; a build with only this file falls back to
whatever RetroArch is installed on the machine, which is why the build step
passes `--allow-missing`.

Nothing staged here is committed — see `.gitignore`. Emulator binaries are
large, they are somebody else's to distribute, and they are reproducible from
the script. This file is committed so the directory exists in a fresh clone
and the bundler always has something to copy.

    cd apps/desktop
    npm run runtime:check                      # what is here now
    python3 ../../tools/bundle_runtime.py      # stage the system RetroArch
    npm run tauri build                        # stage, then build

Read the licence section of the root `README.md` before handing a bundle to
anybody else: RetroArch is GPLv3, and several of the default cores carry
non-commercial terms.
