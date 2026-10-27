pub use play_db::{Db, Game, Save};
use play_ident::dat::Index;
use play_ident::System;
pub use play_input::ControllerInfo;
use play_intake::IntakeOptions;
pub use play_intake::Report;
use play_runner::{PlayerBinding, Runner, Running};
pub use play_setup::SetupStatus;
use play_setup::{Need, Setup};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("intake error: {0}")]
    Intake(#[from] play_intake::IntakeError),
    #[error("runner error: {0}")]
    Runner(#[from] play_runner::RunnerError),
    #[error("game not found")]
    NotFound,
    #[error("external emulator profile required for {0} (not wired in v1)")]
    External(String),
    #[error("{0}")]
    Unusable(String),
}

#[derive(Debug, Serialize)]
pub struct LaunchInfo {
    pub pid: u32,
    pub core: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct RetroArchStatus {
    pub available: bool,
    pub path: Option<String>,
    pub source: Option<String>,
    pub chosen: bool,
    pub problem: Option<String>,
    pub searched: Vec<String>,
    pub runtime_dir: String,
}

#[derive(Debug, Serialize)]
pub struct KeyBinding {
    pub action: String,
    pub key: String,
}

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

fn chrome_label(setting: &str) -> String {
    match setting {
        "input_menu_toggle" => "RetroArch menu",
        "input_exit_emulator" => "Quit back to Play",
        "input_save_state" => "Save state",
        "input_load_state" => "Load state",
        "input_toggle_fullscreen" => "Windowed / fullscreen",
        other => other,
    }
    .to_string()
}

#[derive(Debug, Serialize)]
pub struct CoreStatus {
    pub name: String,
    pub installed: Option<bool>,
    pub unsupported: Option<String>,
}

const RETROARCH_SETTING: &str = "retroarch_path";

const PAD_SETTING: &str = "pad_player:";

const PAD_NOBODY: &str = "none";

pub use play_runner::MAX_PLAYERS;

struct Live {
    game_id: i64,
    title: String,
    system: String,
    started: i64,
    session_id: Option<i64>,
    process: Running,
}

/// A game that is playing right now, as the remote needs to see it.
#[derive(Debug, Serialize)]
pub struct RunningGame {
    pub game_id: i64,
    pub title: String,
    pub system: String,
    pub pid: u32,
    pub started: i64,
    /// How long it has been playing. The phone asking may not keep the same
    /// clock as the machine answering, so the machine does the subtraction.
    pub seconds: i64,
}

pub struct Play {
    pub library: PathBuf,
    db: Db,
    dat: Index,
    runner: Runner,
    setup: Arc<Setup>,
    /// Whether a launch may download a missing core. Off until the shell asks
    /// for setup, so a library opened by a tool or a test never reaches out.
    downloads: AtomicBool,
    input: play_input::Input,
    live: Mutex<Vec<Live>>,
}

impl Play {
    pub fn open(library: &Path) -> Result<Play, Error> {
        fs::create_dir_all(library)?;
        let db = Db::open(&library.join("library.db"))?;
        let dat = load_dat(library);
        let runner = Runner::new(library, &library.join("_config"));
        if let Ok(Some(chosen)) = db.setting(RETROARCH_SETTING) {
            runner.set_chosen(Some(PathBuf::from(chosen)));
        }
        let input = play_input::Input::new();
        let setup = Arc::new(Setup::new(runner.managed_dir()));
        Ok(Play {
            library: library.to_path_buf(),
            db,
            dat,
            runner,
            setup,
            downloads: AtomicBool::new(false),
            input,
            live: Mutex::new(Vec::new()),
        })
    }

    pub fn default_library() -> PathBuf {
        dirs::data_dir()
            .map(|d| d.join("play"))
            .unwrap_or_else(|| PathBuf::from("play"))
    }

    /// Builds from before the rename kept the library in a folder with the
    /// old name. Move it to the default place, once, so nothing is left
    /// behind. Returns where it came from when it moved.
    pub fn adopt_old_library() -> Result<Option<PathBuf>, Error> {
        let Some(data) = dirs::data_dir() else {
            return Ok(None);
        };
        let old = data.join(OLD_LIBRARY_DIR);
        Ok(move_library(&old, &Play::default_library())?.then_some(old))
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

    pub fn intake(&self, drop: &Path, password: Option<String>) -> Result<Report, Error> {
        let opts = IntakeOptions {
            library: self.library.clone(),
            dat: &self.dat,
            db: Some(&self.db),
            password,
        };
        Ok(play_intake::run_intake(drop, &opts)?)
    }

    /// Get this machine ready to play, in the background: a RetroArch if none
    /// can be found, and a core for each common system and each system on the
    /// shelf that has none. Returns at once; [`Play::setup_status`] says how
    /// it is going.
    pub fn start_setup(&self) {
        self.downloads.store(true, Ordering::Relaxed);
        let retroarch = self.runner.locate().is_err()
            && self.runner.chosen().is_none()
            && std::env::var_os("RETROARCH").is_none();
        let mut systems: Vec<System> = System::COMMON.to_vec();
        systems.extend(self.shelved_systems());
        self.fetch_cores_for(retroarch, systems);
    }

    /// Fetch, in the background, a core for each system on the shelf that
    /// has none — what a new intake calls once its games are shelved.
    pub fn fetch_missing_cores(&self) {
        if !self.downloads.load(Ordering::Relaxed) {
            return;
        }
        self.fetch_cores_for(false, self.shelved_systems());
    }

    pub fn setup_status(&self) -> SetupStatus {
        self.setup.status()
    }

    fn shelved_systems(&self) -> Vec<System> {
        self.db
            .list_systems()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, _)| System::from_name(&name))
            .collect()
    }

    fn fetch_cores_for(&self, retroarch: bool, systems: Vec<System>) {
        let mut needs: Vec<Need> = Vec::new();
        let mut ready: Vec<String> = Vec::new();
        for system in systems {
            if external_only(system.name()).is_some() {
                continue;
            }
            // GB and GBC share their cores; one need covers both.
            if needs.iter().any(|n| n.cores[0] == system.default_core()) {
                continue;
            }
            if self.installed_core(system.cores()).is_some() {
                ready.push(system.name().to_string());
                continue;
            }
            needs.push(Need {
                system: system.name().to_string(),
                cores: system.cores().iter().map(|c| c.to_string()).collect(),
            });
        }
        self.setup.already_ready(ready);
        self.setup.prepare(retroarch, needs);
    }

    /// The first of `cores` that is installed anywhere RetroArch can load it.
    fn installed_core<S: AsRef<str>>(&self, cores: &[S]) -> Option<String> {
        cores
            .iter()
            .map(|c| c.as_ref())
            .find(|c| self.runner.find_core(c).is_some())
            .map(|c| c.to_string())
    }

    /// The core to launch `game` with: the first of its system's cores that is
    /// installed, downloading one on the spot if none is.
    fn ready_core(&self, game: &Game) -> String {
        let cores = cores_for(game);
        if let Some(core) = self.installed_core(&cores) {
            return core;
        }
        if cores[0].is_empty()
            || !self.downloads.load(Ordering::Relaxed)
            || self.runner.locate().is_err()
        {
            return cores[0].clone();
        }
        let need = Need {
            system: game.system.clone(),
            cores: cores.clone(),
        };
        match self.setup.get_core(&need) {
            Ok(core) => core,
            Err(e) => {
                eprintln!("play: could not download a core for {}: {e}", game.system);
                cores[0].clone()
            }
        }
    }

    pub fn launch(&self, game_id: i64) -> Result<LaunchInfo, Error> {
        let game = self.db.get_game(game_id)?.ok_or(Error::NotFound)?;
        let system = System::from_name(&game.system);
        if matches!(
            system,
            Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii)
        ) {
            return Err(Error::External(game.system.clone()));
        }
        if self.runner.locate().is_err() {
            let status = self.setup.status();
            if status.working {
                return Err(Error::Unusable(format!(
                    "Play is still getting ready{}. Try again in a minute.",
                    status
                        .doing
                        .map(|d| format!(" ({})", d.to_lowercase()))
                        .unwrap_or_default()
                )));
            }
        }
        let core = self.ready_core(&game);
        // One game, one process: a second copy would share this game's config
        // file and its save RAM with the first, and the loser of that race
        // takes the session with it. A double-tap on the remote is the usual
        // way to ask for this.
        if self.running().iter().any(|live| live.game_id == game_id) {
            return Err(Error::Unusable(format!(
                "{} is already running",
                game.title
            )));
        }
        let players = self.player_bindings();
        let process = self.runner.launch(&game, &core, &players)?;
        let pid = process.pid();
        let session_id = match self.db.start_session(game_id) {
            Ok(id) => Some(id),
            Err(e) => {
                log_session_error(&e);
                None
            }
        };
        if let Ok(mut live) = self.live.lock() {
            live.push(Live {
                game_id,
                title: game.title.clone(),
                system: game.system.clone(),
                started: play_db::now(),
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

    pub fn running_count(&self) -> usize {
        self.running().len()
    }

    /// What is playing, oldest first.
    pub fn running(&self) -> Vec<RunningGame> {
        self.reap();
        let Ok(live) = self.live.lock() else {
            return Vec::new();
        };
        let now = play_db::now();
        live.iter()
            .map(|entry| RunningGame {
                game_id: entry.game_id,
                title: entry.title.clone(),
                system: entry.system.clone(),
                pid: entry.process.pid(),
                started: entry.started,
                seconds: (now - entry.started).max(0),
            })
            .collect()
    }

    /// Quit a game the way Escape would, so RetroArch saves on the way out.
    /// Answers with how many processes went; nothing running is not an error,
    /// because a game that ended on its own is the outcome that was asked for.
    pub fn stop(&self, game_id: i64) -> usize {
        self.stop_matching(|entry| entry.game_id == game_id)
    }

    /// Quit everything, for the remote's "stop what is playing" with no game
    /// named — which is what someone reaching for their phone usually means.
    pub fn stop_all(&self) -> usize {
        self.stop_matching(|_| true)
    }

    fn stop_matching(&self, wanted: impl Fn(&Live) -> bool) -> usize {
        let mut going = Vec::new();
        {
            let Ok(mut live) = self.live.lock() else {
                return 0;
            };
            let mut keep = Vec::with_capacity(live.len());
            for entry in live.drain(..) {
                if wanted(&entry) {
                    going.push(entry);
                } else {
                    keep.push(entry);
                }
            }
            *live = keep;
        }
        let stopped = going.len();
        for mut entry in going {
            if let Err(e) = entry.process.quit() {
                eprintln!("play: could not stop {}: {e}", entry.title);
            }
            if let Some(session_id) = entry.session_id {
                let _ = self.db.end_session(session_id);
            }
        }
        stopped
    }

    pub fn controllers(&self) -> Vec<ControllerInfo> {
        let mut pads = self.input.controllers();
        let mut taken: Vec<usize> = Vec::new();

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

        for (pad, deliberate) in pads.iter_mut().zip(spoken_for) {
            if pad.player.is_some() || deliberate {
                continue;
            }
            let Some(free) = (1..=MAX_PLAYERS).find(|n| !taken.contains(n)) else {
                break;
            };
            pad.player = Some(free);
            taken.push(free);
            let _ = self
                .db
                .set_setting(&pad_key(&pad.identity), Some(&free.to_string()));
        }
        pads
    }

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

    pub fn keyboard_scheme(&self) -> Vec<KeyBinding> {
        play_runner::KEYBOARD_SCHEME
            .iter()
            .map(|(button, _key, shown)| KeyBinding {
                action: button_label(button),
                key: shown.to_string(),
            })
            .chain(
                play_runner::KEYBOARD_CHROME
                    .iter()
                    .map(|(setting, _key, shown)| KeyBinding {
                        action: chrome_label(setting),
                        key: shown.to_string(),
                    }),
            )
            .collect()
    }

    pub fn assign_pad(&self, identity: &str, player: Option<usize>) -> Result<(), Error> {
        if let Some(player) = player {
            if !(1..=MAX_PLAYERS).contains(&player) {
                return Err(Error::Unusable(format!(
                    "there is no player {player}; Play drives {MAX_PLAYERS}"
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
                    let swap = mine.map(|p| p.to_string());
                    self.db
                        .set_setting(&pad_key(&pad.identity), swap.as_deref())?;
                }
            }
            self.db
                .set_setting(&pad_key(identity), Some(&player.to_string()))?;
        } else {
            self.db.set_setting(&pad_key(identity), Some(PAD_NOBODY))?;
        }
        Ok(())
    }

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

    pub fn core_status(&self, game: &Game) -> CoreStatus {
        let cores = cores_for(game);
        let name = self
            .installed_core(&cores)
            .unwrap_or_else(|| cores[0].clone());
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

    pub fn set_runtime_dir(&self, dir: Option<PathBuf>) {
        self.runner.set_bundled_dir(dir);
    }

    pub fn set_retroarch_path(&self, path: Option<PathBuf>) -> Result<RetroArchStatus, Error> {
        let previous = self.runner.chosen();
        match path {
            Some(path) => {
                let Some(text) = path.to_str() else {
                    return Err(Error::Unusable(format!(
                        "`{}` is not valid UTF-8, so Play cannot store it. \
                         Set RETROARCH to it instead.",
                        path.display()
                    )));
                };
                self.runner.set_chosen(Some(path.clone()));
                if let Err(e) = self.runner.locate() {
                    self.runner.set_chosen(previous);
                    return Err(Error::Runner(e));
                }
                if let Err(e) = self.db.set_setting(RETROARCH_SETTING, Some(text)) {
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

impl Drop for Play {
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
    eprintln!("play: could not record the play session: {e}");
}

fn external_only(system: &str) -> Option<String> {
    match System::from_name(system) {
        Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii) => Some(format!(
            "{system} needs an external emulator, which Play does not drive yet"
        )),
        _ => None,
    }
}

/// The cores that can run `game`, best first: the one it was set to if any,
/// otherwise its system's. Never empty; an unknown system gets one empty name,
/// which nothing will find.
fn cores_for(game: &Game) -> Vec<String> {
    if let Some(core) = &game.core {
        return vec![core.clone()];
    }
    System::from_name(&game.system)
        .map(|s| s.cores().iter().map(|c| c.to_string()).collect())
        .unwrap_or_else(|| vec![String::new()])
}

fn pad_key(identity: &str) -> String {
    format!("{PAD_SETTING}{identity}")
}

/// What the library folder was called before the program was named Play.
const OLD_LIBRARY_DIR: &str = "den";

/// Move a library from `from` to `to` and rewrite the paths it stores, but
/// only when there is something at `from` and nothing yet at `to`. Returns
/// whether it moved.
pub fn move_library(from: &Path, to: &Path) -> Result<bool, Error> {
    if !from.join("library.db").is_file() || to.exists() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(from, to)?;
    Db::open(&to.join("library.db"))?.rebase_paths(from, to)?;
    Ok(true)
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
    play_ident::dat::bundled()
}
