//! Glue the shell talks to: one Den instance holding the library, database,
//! DAT index, runner, and input manager.

pub use den_db::{Db, Game, Save};
use den_ident::dat::Index;
use den_ident::System;
pub use den_input::ControllerInfo;
use den_intake::IntakeOptions;
pub use den_intake::Report;
use den_runner::{Runner, Running};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Anything that can go wrong between the shell and the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The library database said no.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// The filesystem said no.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Intake could not finish.
    #[error("intake error: {0}")]
    Intake(#[from] den_intake::IntakeError),
    /// The emulator could not be started.
    #[error("runner error: {0}")]
    Runner(#[from] den_runner::RunnerError),
    /// No game with that id.
    #[error("game not found")]
    NotFound,
    /// A system Den knows about but cannot boot through RetroArch yet.
    #[error("external emulator profile required for {0} (not wired in v1)")]
    External(String),
}

/// What a successful launch reports back to the shell.
#[derive(Debug, Serialize)]
pub struct LaunchInfo {
    /// The process id of the emulator that was started.
    pub pid: u32,
    /// The libretro core it was started with.
    pub core: String,
    /// The file it was pointed at.
    pub content: String,
}

/// One launched emulator and the session row that is open for it.
struct Live {
    session_id: i64,
    process: Running,
}

/// The one object the Tauri shell talks to.
pub struct Den {
    /// The library directory this instance owns.
    pub library: PathBuf,
    db: Db,
    dat: Index,
    runner: Runner,
    input: den_input::Input,
    /// Emulators started by this instance, with their open session rows.
    live: Mutex<Vec<Live>>,
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
            live: Mutex::new(Vec::new()),
        })
    }

    /// The default library directory on this machine.
    pub fn default_library() -> PathBuf {
        dirs::data_dir()
            .map(|d| d.join("den"))
            .unwrap_or_else(|| PathBuf::from("den"))
    }

    /// The library database.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The loaded hash database.
    pub fn dat(&self) -> &Index {
        &self.dat
    }

    /// The RetroArch runner.
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
        self.reap();
        let process = self.runner.launch(&game, &core)?;
        let pid = process.pid();
        // A launch that is not written down is a launch the library forgets:
        // playtime, Recent, and the Continue row are all read back out of the
        // sessions table.
        match self.db.start_session(game_id) {
            Ok(session_id) => {
                if let Ok(mut live) = self.live.lock() {
                    live.push(Live {
                        session_id,
                        process,
                    });
                }
            }
            Err(e) => {
                // The emulator is already up; losing the row costs a line of
                // history, not the game someone asked to play.
                log_session_error(&e);
            }
        }
        Ok(LaunchInfo {
            pid,
            core,
            content: game.path,
        })
    }

    /// Close the session row of every emulator that has exited since the last
    /// look, and reap the process while we are there. Cheap and non-blocking,
    /// so the shell can call it before any read of the library.
    pub fn reap(&self) {
        let Ok(mut live) = self.live.lock() else {
            return;
        };
        let mut finished = Vec::new();
        live.retain_mut(|entry| {
            if entry.process.is_running() {
                true
            } else {
                finished.push(entry.session_id);
                false
            }
        });
        for session_id in finished {
            let _ = self.db.end_session(session_id);
        }
    }

    /// How many emulators this instance still has running.
    pub fn running_count(&self) -> usize {
        self.reap();
        self.live.lock().map(|live| live.len()).unwrap_or(0)
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

/// Every emulator this instance started is still its child; closing their
/// session rows on the way out keeps a quit from losing the last play.
impl Drop for Den {
    fn drop(&mut self) {
        let sessions: Vec<i64> = self
            .live
            .get_mut()
            .map(|live| live.iter().map(|e| e.session_id).collect())
            .unwrap_or_default();
        for session_id in sessions {
            let _ = self.db.end_session(session_id);
        }
    }
}

fn log_session_error(e: &rusqlite::Error) {
    // den-core has no logger of its own; the shell installs one and this is
    // the only line that would use it, so it goes to stderr plainly.
    eprintln!("den: could not record the play session: {e}");
}

fn load_dat(library: &Path) -> Index {
    for candidate in [
        library.join("dat").join("index.tsv"),
        library.join("dat.tsv"),
    ] {
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
