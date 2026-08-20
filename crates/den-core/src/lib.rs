//! Glue the shell talks to: one Den instance holding the library, database,
//! DAT index, runner, and input manager.

pub use den_db::{Db, Game, Save};
use den_ident::dat::Index;
use den_ident::System;
pub use den_input::ControllerInfo;
use den_intake::IntakeOptions;
pub use den_intake::Report;
use den_runner::{PlayerBinding, Runner, Running};
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
    /// Something the interface asked for that Den cannot do with it.
    #[error("{0}")]
    Unusable(String),
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

/// One line of the keyboard scheme, as the interface shows it.
#[derive(Debug, Serialize)]
pub struct KeyBinding {
    /// What it does, in the words a person uses.
    pub action: String,
    /// The key that does it.
    pub key: String,
}

/// The word for a controller button, rather than its RetroArch key.
fn button_label(button: &str) -> String {
    match button {
        "up" | "down" | "left" | "right" => {
            let mut c = button.chars();
            let first = c.next().map(|f| f.to_ascii_uppercase());
            format!("D-pad {}{}", first.unwrap_or('?'), c.as_str())
        }
        "a" | "b" | "x" | "y" => format!("{} button", button.to_ascii_uppercase()),
        "l" | "r" => format!("{} shoulder", button.to_ascii_uppercase()),
        "start" => "Start".to_string(),
        "select" => "Select".to_string(),
        other => other.to_string(),
    }
}

/// The word for one of RetroArch's own actions.
fn chrome_label(setting: &str) -> String {
    match setting {
        "input_menu_toggle" => "RetroArch menu",
        "input_exit_emulator" => "Quit back to Den",
        "input_save_state" => "Save state",
        "input_load_state" => "Load state",
        "input_toggle_fullscreen" => "Windowed / fullscreen",
        other => other,
    }
    .to_string()
}

/// What Den knows about the libretro core a game needs.
#[derive(Debug, Serialize)]
pub struct CoreStatus {
    /// The core Den would ask RetroArch for.
    pub name: String,
    /// Whether it is installed. `None` when Den could not find a cores
    /// directory to look in, and so has nothing honest to say either way.
    pub installed: Option<bool>,
    /// Set when this system cannot be launched at all yet, whatever is
    /// installed: PlayStation 2, GameCube and Wii need external emulator
    /// profiles. Saying "the core is missing" there would send somebody to
    /// the Core Downloader for a core that would not help.
    pub unsupported: Option<String>,
}

/// The library setting holding a RetroArch picked by hand.
const RETROARCH_SETTING: &str = "retroarch_path";

/// The prefix under which a pad's player number is kept.
const PAD_SETTING: &str = "pad_player:";

/// What is written down for a pad somebody deliberately gave to nobody.
///
/// Distinct from having no setting at all, which means "never seen, decide
/// for it". Without the difference, "Nobody" is a button that does nothing:
/// the next look at the controllers finds a pad with no player and helpfully
/// gives it one back.
const PAD_NOBODY: &str = "none";

/// How many players RetroArch is configured for.
pub const MAX_PLAYERS: usize = 4;

/// One launched emulator and the session row that is open for it.
struct Live {
    /// `None` when the row could not be written. The process is still ours to
    /// wait on -- dropping the handle would leave an emulator Den can never
    /// reap, and a zombie behind it once it exits.
    session_id: Option<i64>,
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
        // The pads as they stand right now, so a controller plugged in while
        // Den was open is player one by the time the game starts.
        let players = self.player_bindings();
        let process = self.runner.launch(&game, &core, &players)?;
        let pid = process.pid();
        // A launch that is not written down is a launch the library forgets:
        // playtime, Recent, and the Continue row are all read back out of the
        // sessions table.
        let session_id = match self.db.start_session(game_id) {
            Ok(id) => Some(id),
            Err(e) => {
                // The emulator is already up; losing the row costs a line of
                // history, not the game someone asked to play.
                log_session_error(&e);
                None
            }
        };
        if let Ok(mut live) = self.live.lock() {
            live.push(Live {
                session_id,
                process,
            });
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
        for session_id in finished.into_iter().flatten() {
            let _ = self.db.end_session(session_id);
        }
    }

    /// How many emulators this instance still has running.
    pub fn running_count(&self) -> usize {
        self.reap();
        self.live.lock().map(|live| live.len()).unwrap_or(0)
    }

    /// Controllers currently attached, each with the player it answers for.
    ///
    /// A pad nobody has assigned takes the lowest free player, and that
    /// sticks. Plugging a controller in and having it be Player 1 is the
    /// whole of what most people want; making them say so first is a step
    /// that exists only because the software could not decide.
    pub fn controllers(&self) -> Vec<ControllerInfo> {
        let mut pads = self.input.controllers();
        let mut taken: Vec<usize> = Vec::new();

        // Assignments somebody made stand, and claim their number first.
        let mut spoken_for: Vec<bool> = Vec::with_capacity(pads.len());
        for pad in pads.iter_mut() {
            let setting = self.db.setting(&pad_key(&pad.identity)).ok().flatten();
            let deliberate = setting.as_deref() == Some(PAD_NOBODY);
            if let Some(player) = setting.as_deref().and_then(|v| v.parse::<usize>().ok()) {
                if (1..=MAX_PLAYERS).contains(&player) && !taken.contains(&player) {
                    pad.player = Some(player);
                    taken.push(player);
                }
            }
            spoken_for.push(deliberate);
        }

        // Everything else takes the lowest free number, in joystick order --
        // except a pad somebody said belongs to nobody, which stays that way.
        for (pad, deliberate) in pads.iter_mut().zip(spoken_for) {
            if pad.player.is_some() || deliberate {
                continue;
            }
            let Some(free) = (1..=MAX_PLAYERS).find(|n| !taken.contains(n)) else {
                break; // more pads than players; the rest stay unassigned
            };
            pad.player = Some(free);
            taken.push(free);
            // Written down, so it is the same pad's number next time even if
            // the order they enumerate in changes.
            let _ = self
                .db
                .set_setting(&pad_key(&pad.identity), Some(&free.to_string()));
        }
        pads
    }

    /// The pad-to-player mapping a launch should be told about.
    fn player_bindings(&self) -> Vec<PlayerBinding> {
        self.controllers()
            .into_iter()
            .filter_map(|pad| {
                Some(PlayerBinding {
                    player: pad.player?,
                    joypad_index: pad.index,
                })
            })
            .collect()
    }

    /// How Den's keyboard scheme reads, for the interface to show.
    ///
    /// Taken from the same table the config is written from, so what is on
    /// screen is what the keys do.
    pub fn keyboard_scheme(&self) -> Vec<KeyBinding> {
        den_runner::KEYBOARD_SCHEME
            .iter()
            .map(|(button, _key, shown)| KeyBinding {
                action: button_label(button),
                key: shown.to_string(),
            })
            .chain(
                den_runner::KEYBOARD_CHROME
                    .iter()
                    .map(|(setting, _key, shown)| KeyBinding {
                        action: chrome_label(setting),
                        key: shown.to_string(),
                    }),
            )
            .collect()
    }

    /// Assign a pad to a player, or to nobody with `None`.
    ///
    /// Assigning a player that another pad holds swaps them, rather than
    /// leaving two pads claiming one player and one of them silently losing.
    pub fn assign_pad(&self, identity: &str, player: Option<usize>) -> Result<(), Error> {
        if let Some(player) = player {
            if !(1..=MAX_PLAYERS).contains(&player) {
                return Err(Error::Unusable(format!(
                    "there is no player {player}; Den drives {MAX_PLAYERS}"
                )));
            }
            let mine = self
                .db
                .setting(&pad_key(identity))?
                .and_then(|v| v.parse::<usize>().ok());
            for pad in self.input.controllers() {
                if pad.identity == identity {
                    continue;
                }
                let theirs = self
                    .db
                    .setting(&pad_key(&pad.identity))?
                    .and_then(|v| v.parse::<usize>().ok());
                if theirs == Some(player) {
                    // Hand them the number this pad is giving up. Without a
                    // number of its own to give, they become unassigned.
                    let swap = mine.map(|p| p.to_string());
                    self.db
                        .set_setting(&pad_key(&pad.identity), swap.as_deref())?;
                }
            }
            self.db
                .set_setting(&pad_key(identity), Some(&player.to_string()))?;
        } else {
            // Written down rather than cleared: a cleared setting reads as
            // "never seen", and the next look would assign it again.
            self.db.set_setting(&pad_key(identity), Some(PAD_NOBODY))?;
        }
        Ok(())
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
        if let Some(reason) = external_only(&game.system) {
            return CoreStatus {
                name,
                installed: None,
                unsupported: Some(reason),
            };
        }
        let installed = if name.is_empty() {
            Some(false)
        } else {
            self.runner.core_installed(&name)
        };
        CoreStatus {
            name,
            installed,
            unsupported: None,
        }
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
        let previous = self.runner.chosen();
        match path {
            Some(path) => {
                // A path that cannot be written down exactly cannot be read
                // back: `to_string_lossy` would replace the bytes it does not
                // understand and hand back a path to somewhere else entirely.
                let Some(text) = path.to_str() else {
                    return Err(Error::Unusable(format!(
                        "`{}` is not valid UTF-8, so Den cannot store it. \
                         Set RETROARCH to it instead.",
                        path.display()
                    )));
                };
                // Kept as picked, not canonicalized: resolving the symlink
                // behind a Flatpak or Snap wrapper turns it into the
                // multiplexer it points at, which is not RetroArch.
                self.runner.set_chosen(Some(path.clone()));
                if let Err(e) = self.runner.locate() {
                    self.runner.set_chosen(previous);
                    return Err(Error::Runner(e));
                }
                if let Err(e) = self.db.set_setting(RETROARCH_SETTING, Some(text)) {
                    // The runner and the library have to agree. Leaving the
                    // runner changed after the write failed means the choice
                    // works until the next restart and then silently does not.
                    self.runner.set_chosen(previous);
                    return Err(Error::Db(e));
                }
            }
            None => {
                self.runner.set_chosen(None);
                if let Err(e) = self.db.set_setting(RETROARCH_SETTING, None) {
                    self.runner.set_chosen(previous);
                    return Err(Error::Db(e));
                }
            }
        }
        Ok(self.retroarch_status())
    }
}

/// Every emulator this instance started is still its child; closing their
/// session rows on the way out keeps a quit from losing the last play.
impl Drop for Den {
    fn drop(&mut self) {
        let sessions: Vec<i64> = self
            .live
            .get_mut()
            .map(|live| live.iter().filter_map(|e| e.session_id).collect())
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

/// Why a system cannot be launched at all, if it cannot.
fn external_only(system: &str) -> Option<String> {
    match system_from_name(system) {
        Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii) => Some(format!(
            "{system} needs an external emulator, which Den does not drive yet"
        )),
        _ => None,
    }
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

fn pad_key(identity: &str) -> String {
    format!("{PAD_SETTING}{identity}")
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
