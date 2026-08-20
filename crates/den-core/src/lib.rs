//! Glue the shell talks to: one Den instance holding the library, database,
//! DAT index, runner, and input manager.

pub use den_db::{Db, Game, Save};
use den_ident::dat::Index;
use den_ident::System;
pub use den_input::ControllerInfo;
pub use den_intake::Report;
use den_intake::IntakeOptions;
use den_runner::Runner;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("intake error: {0}")]
    Intake(#[from] den_intake::IntakeError),
    #[error("runner error: {0}")]
    Runner(#[from] den_runner::RunnerError),
    #[error("game not found")]
    NotFound,
    #[error("external emulator profile required for {0} (not wired in v1)")]
    External(String),
}

/// What a successful launch reports back to the shell.
#[derive(Debug, Serialize)]
pub struct LaunchInfo {
    pub pid: u32,
    pub core: String,
    pub content: String,
}

/// The one object the Tauri shell talks to.
pub struct Den {
    pub library: PathBuf,
    db: Db,
    dat: Index,
    runner: Runner,
    input: den_input::Input,
}

impl Den {
    /// Open (creating if needed) a library at `library`.
    pub fn open(library: &Path) -> Result<Den, Error> {
        fs::create_dir_all(library)?;
        let db = Db::open(&library.join("library.db"))?;
        let dat = load_dat(library);
        let runner = Runner::new(library, &library.join("_config"));
        let input = den_input::Input::new();
        Ok(Den {
            library: library.to_path_buf(),
            db,
            dat,
            runner,
            input,
        })
    }

    /// The default library directory on this machine.
    pub fn default_library() -> PathBuf {
        dirs::data_dir()
            .map(|d| d.join("den"))
            .unwrap_or_else(|| PathBuf::from("den"))
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn dat(&self) -> &Index {
        &self.dat
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    /// Run intake over a drop, shelving into the library.
    pub fn intake(&self, drop: &Path, password: Option<String>) -> Result<Report, Error> {
        let opts = IntakeOptions {
            library: self.library.clone(),
            dat: &self.dat,
            db: Some(&self.db),
            password,
        };
        Ok(den_intake::run_intake(drop, &opts)?)
    }

    /// Launch a game by id. External-emulator systems (PS2, GameCube, Wii) are
    /// rejected with a clear word until those profiles are wired.
    pub fn launch(&self, game_id: i64) -> Result<LaunchInfo, Error> {
        let game = self.db.get_game(game_id)?.ok_or(Error::NotFound)?;
        let system = system_from_name(&game.system);
        if matches!(
            system,
            Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii)
        ) {
            return Err(Error::External(game.system.clone()));
        }
        let core = game.core.clone().unwrap_or_else(|| {
            system
                .map(|s| s.default_core().to_string())
                .unwrap_or_default()
        });
        let running = self.runner.launch(&game, &core)?;
        Ok(LaunchInfo {
            pid: running.pid(),
            core,
            content: game.path,
        })
    }

    /// Controllers currently attached.
    pub fn controllers(&self) -> Vec<ControllerInfo> {
        self.input.controllers()
    }

    /// Whether RetroArch is available to launch.
    pub fn retroarch_available(&self) -> bool {
        self.runner.available()
    }
}

fn load_dat(library: &Path) -> Index {
    for candidate in [library.join("dat").join("index.tsv"), library.join("dat.tsv")] {
        if candidate.is_file() {
            if let Ok(index) = Index::load_tsv(&candidate) {
                return index;
            }
        }
    }
    den_ident::dat::bundled()
}

fn system_from_name(name: &str) -> Option<System> {
    use System::*;
    Some(match name {
        "NES" => Nes,
        "SNES" => Snes,
        "Genesis" => Genesis,
        "Sega CD" => SegaCd,
        "Sega 32X" => Sega32x,
        "N64" => N64,
        "PlayStation" => Ps1,
        "GB" => Gb,
        "GBC" => Gbc,
        "GBA" => Gba,
        "Arcade" => Arcade,
        "DOS" => Dos,
        "PlayStation 2" => Ps2,
        "GameCube" => Gamecube,
        "Wii" => Wii,
        _ => return None,
    })
}
