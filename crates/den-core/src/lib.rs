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

/// What Den knows about RetroArch right now, in the shape the interface
/// needs to say it: whether there is one, where it is, and -- when there is
/// not -- everywhere Den looked, so the answer is actionable rather than just
/// discouraging.
#[derive(Debug, Serialize)]
pub struct RetroArchStatus {
    /// Whether a RetroArch can be launched.
    pub available: bool,
    /// The binary Den would launch, when there is one.
    pub path: Option<String>,
    /// How it was found: `chosen`, `environment`, `bundled`, or `system`.
    pub source: Option<String>,
    /// Whether the path was picked by hand, and so can be given back.
    pub chosen: bool,
    /// The reason there is not one, in a sentence.
    pub problem: Option<String>,
    /// Every place Den looked, in the order it looked.
    pub searched: Vec<String>,
    /// Where a runtime installed for this library would go.
    pub runtime_dir: String,
}

/// What Den knows about the libretro core a game needs.
#[derive(Debug, Serialize)]
pub struct CoreStatus {
    /// The core Den would ask RetroArch for.
    pub name: String,
    /// Whether it is installed. `None` when Den could not find a cores
    /// directory to look in, and so has nothing honest to say either way.
    pub installed: Option<bool>,
}

/// The library setting holding a RetroArch picked by hand.
const RETROARCH_SETTING: &str = "retroarch_path";

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
        // A RetroArch picked by hand belongs to the library, so it comes back
        // with it rather than having to be picked again every launch.
        if let Ok(Some(chosen)) = db.setting(RETROARCH_SETTING) {
            runner.set_chosen(Some(PathBuf::from(chosen)));
        }
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
        let core = core_for(&game);
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

    /// Where RetroArch is, or where Den looked for it and came up empty.
    pub fn retroarch_status(&self) -> RetroArchStatus {
        let searched = self
            .runner
            .searched()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let chosen = self.runner.chosen().is_some();
        let runtime_dir = self.runner.managed_dir().display().to_string();
        match self.runner.locate() {
            Ok(found) => RetroArchStatus {
                available: true,
                path: Some(found.path.display().to_string()),
                source: Some(found.source.word().to_string()),
                chosen,
                problem: None,
                searched,
                runtime_dir,
            },
            Err(e) => RetroArchStatus {
                available: false,
                path: None,
                source: None,
                chosen,
                problem: Some(e.to_string()),
                searched,
                runtime_dir,
            },
        }
    }

    /// The core a game would launch with, and whether it is installed.
    pub fn core_status(&self, game: &Game) -> CoreStatus {
        let name = core_for(game);
        let installed = if name.is_empty() {
            Some(false)
        } else {
            self.runner.core_installed(&name)
        };
        CoreStatus { name, installed }
    }

    /// Tell Den where this build's bundled runtime lives. The shell calls
    /// this once at startup with the directory Tauri resolved for it.
    pub fn set_runtime_dir(&self, dir: Option<PathBuf>) {
        self.runner.set_bundled_dir(dir);
    }

    /// Point Den at a RetroArch by hand, or hand the choice back to the
    /// search with `None`. Kept with the library, so it survives a restart.
    ///
    /// The path is checked before it is kept: a setting that does not work is
    /// worse than no setting, because it also switches the search off.
    pub fn set_retroarch_path(&self, path: Option<PathBuf>) -> Result<RetroArchStatus, Error> {
        match path {
            Some(path) => {
                // Kept as picked, not canonicalized: resolving the symlink
                // behind a Flatpak or Snap wrapper turns it into the
                // multiplexer it points at, which is not RetroArch.
                self.runner.set_chosen(Some(path.clone()));
                if let Err(e) = self.runner.locate() {
                    // Put it back the way it was rather than leaving the
                    // library pointed at something that cannot run.
                    self.restore_chosen();
                    return Err(Error::Runner(e));
                }
                self.db
                    .set_setting(RETROARCH_SETTING, Some(&path.to_string_lossy()))?;
            }
            None => {
                self.runner.set_chosen(None);
                self.db.set_setting(RETROARCH_SETTING, None)?;
            }
        }
        Ok(self.retroarch_status())
    }

    /// Put the runner back on whatever the library has written down.
    fn restore_chosen(&self) {
        let saved = self
            .db
            .setting(RETROARCH_SETTING)
            .ok()
            .flatten()
            .map(PathBuf::from);
        self.runner.set_chosen(saved);
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

/// The core a game launches with: its own override, else its system's
/// default. One function, so what the interface reports and what the runner
/// asks for can never drift apart.
fn core_for(game: &Game) -> String {
    game.core.clone().unwrap_or_else(|| {
        system_from_name(&game.system)
            .map(|s| s.default_core().to_string())
            .unwrap_or_default()
    })
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
