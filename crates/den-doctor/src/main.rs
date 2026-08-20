//! `den-doctor` — say what Den can and cannot find on this machine.
//!
//! When somebody reports that Den cannot find RetroArch, the useful reply is
//! not a guess, it is this: every place Den looked, what was actually at each
//! one, and which answer it settled on. It is a separate binary from the shell
//! on purpose — it builds headless, so it runs on a machine that cannot build
//! a WebView, and it can be run without rebuilding the application.
//!
//!     cargo run -p den-doctor

use den_core::Den;
use den_ident::System;
use std::path::{Path, PathBuf};

fn main() {
    let library = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(Den::default_library);

    println!("Den doctor");
    println!(
        "  platform      {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("  library       {}", library.display());

    // A diagnostic reports; it does not create things. Opening a library
    // makes one, so a mistyped path would leave an empty library behind and
    // report cheerfully on it.
    let exists = library.join("library.db").is_file();
    if !exists {
        println!("  (no library here yet — it is made on first run)");
    }
    let den = match Den::open(&library) {
        Ok(den) => den,
        Err(e) => {
            println!("\n  could not open the library: {e}");
            std::process::exit(1);
        }
    };
    if !exists {
        // Put back what opening it just made.
        let _ = std::fs::remove_file(library.join("library.db"));
        let _ = std::fs::remove_file(library.join("library.db-wal"));
        let _ = std::fs::remove_file(library.join("library.db-shm"));
        let _ = std::fs::remove_dir(&library);
    }

    println!(
        "  games         {}",
        den.db()
            .game_count()
            .map(|n| n.to_string())
            .unwrap_or_else(|e| e.to_string())
    );

    println!("\nEnvironment");
    for var in ["RETROARCH", "DEN_RUNTIME_DIR", "PATH"] {
        match std::env::var(var) {
            Ok(value) => println!("  {var:<15} {value}"),
            Err(_) => println!("  {var:<15} (not set)"),
        }
    }

    let status = den.retroarch_status();
    println!("\nRetroArch");
    if status.available {
        println!(
            "  found         {}",
            status.path.clone().unwrap_or_default()
        );
        println!(
            "  source        {}",
            status.source.clone().unwrap_or_default()
        );
    } else {
        println!("  found         no");
        println!(
            "  because       {}",
            status.problem.clone().unwrap_or_default()
        );
    }
    println!("  runtime dir   {}", status.runtime_dir);
    println!(
        "  chosen by hand {}",
        if status.chosen { "yes" } else { "no" }
    );

    println!("\nWhere Den looked ({} places)", status.searched.len());
    let mut existing = 0usize;
    for place in &status.searched {
        let path = Path::new(place);
        let note = if !path.exists() {
            "-"
        } else if path.is_dir() {
            existing += 1;
            "a directory, not a program"
        } else if den_runner::is_runnable(path) {
            existing += 1;
            "runnable"
        } else {
            existing += 1;
            "there, but not executable"
        };
        // Quiet about the many paths that simply are not there; loud about
        // anything that exists, because that is where a surprise hides.
        if note != "-" {
            println!("  {note:<26} {place}");
        }
    }
    if existing == 0 {
        println!("  nothing at any of them.");
        println!("\n  The full list:");
        for place in &status.searched {
            println!("    {place}");
        }
    }

    if let Some(found) = status.path.as_deref() {
        report_cores(&den, Path::new(found));
    }

    println!("\nControllers");
    let pads = den.controllers();
    if pads.is_empty() {
        println!("  none detected — the keyboard scheme below is the way in");
    }
    for pad in &pads {
        let player = pad
            .player
            .map(|p| format!("Player {p}"))
            .unwrap_or_else(|| "nobody".to_string());
        println!("  {:<6} {:<10} {}", pad.id, player, pad.name);
        println!("         {}", pad.identity);
    }

    println!("\nKeyboard (player one)");
    for binding in den.keyboard_scheme() {
        println!("  {:<24} {}", binding.action, binding.key);
    }

    if !status.available {
        println!("\nNothing here is fatal: Den shelves and names games without RetroArch.");
        if status.chosen {
            // The search is switched off while a chosen path stands, so
            // "install RetroArch" would not help until this is dealt with.
            println!(
                "A RetroArch was chosen by hand and no longer works, and while that\n\
                 stands Den does not search at all. Either choose another —\n\
                 \"Choose RetroArch…\" on the Library screen — or press \"Use the\n\
                 automatic search again\" beside it to hand the choice back."
            );
        } else {
            println!(
                "To play, either install RetroArch, or point Den at it — \"Choose RetroArch…\"\n\
                 on the Library screen, or set RETROARCH to the binary."
            );
        }
    }
}

/// Which of the cores Den would ask for are actually installed.
fn report_cores(den: &Den, retroarch: &Path) {
    println!("\nRetroArch's own configuration");
    match den_runner::user_config(retroarch) {
        Some(config) => println!("  {}", config.display()),
        None => println!("  none found — Den cannot read where your cores are"),
    }

    let Some(dir) = den.runner().cores_for(retroarch) else {
        println!("\nCores");
        println!("  No cores directory found. Den will pass RetroArch the core");
        println!("  file name and let it resolve one; if that fails, RetroArch");
        println!("  says `Fatal error received in: \"init_libretro_symbols()\"`.");
        return;
    };
    println!("\nCores  ({})", dir.display());
    let systems = [
        System::Nes,
        System::Snes,
        System::Genesis,
        System::N64,
        System::Ps1,
        System::Gb,
        System::Gba,
        System::Arcade,
        System::Dos,
    ];
    let mut missing = Vec::new();
    for system in systems {
        let core = system.default_core();
        let file = den_runner::core_file_name(core);
        if dir.join(&file).is_file() {
            println!("  present       {:<14} {file}", system.name());
        } else {
            missing.push((system.name(), file));
        }
    }
    for (name, file) in &missing {
        println!("  MISSING       {name:<14} {file}");
    }
    if !missing.is_empty() {
        println!(
            "\n  A missing core is not a Den problem: RetroArch downloads cores itself,\n\
             under Main Menu → Online Updater → Core Downloader. Den refuses to\n\
             launch a game whose core is not there, rather than letting RetroArch\n\
             die with `init_libretro_symbols()`."
        );
    }
}
