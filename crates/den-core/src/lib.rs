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
        "input_exit_emulator" => "Quit back to Den",
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

pub use den_runner::MAX_PLAYERS;

struct Live {
    session_id: Option<i64>,
    process: Running,
}

pub struct Den {
    pub library: PathBuf,
    db: Db,
    dat: Index,
    runner: Runner,
    input: den_input::Input,
    live: Mutex<Vec<Live>>,
}

impl Den {
    pub fn open(library: &Path) -> Result<Den, Error> {
        fs::create_dir_all(library)?;
        let db = Db::open(&library.join("library.db"))?;
        let dat = load_dat(library);
        let runner = Runner::new(library, &library.join("_config"));
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

    pub fn intake(&self, drop: &Path, password: Option<String>) -> Result<Report, Error> {
        let opts = IntakeOptions {
            library: self.library.clone(),
            dat: &self.dat,
            db: Some(&self.db),
            password,
        };
        Ok(den_intake::run_intake(drop, &opts)?)
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
        let core = core_for(&game);
        self.reap();
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
        self.reap();
        self.live.lock().map(|live| live.len()).unwrap_or(0)
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

    pub fn set_runtime_dir(&self, dir: Option<PathBuf>) {
        self.runner.set_bundled_dir(dir);
    }

    pub fn set_retroarch_path(&self, path: Option<PathBuf>) -> Result<RetroArchStatus, Error> {
        let previous = self.runner.chosen();
        match path {
            Some(path) => {
                let Some(text) = path.to_str() else {
                    return Err(Error::Unusable(format!(
                        "`{}` is not valid UTF-8, so Den cannot store it. \
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
    eprintln!("den: could not record the play session: {e}");
}

fn external_only(system: &str) -> Option<String> {
    match System::from_name(system) {
        Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii) => Some(format!(
            "{system} needs an external emulator, which Den does not drive yet"
        )),
        _ => None,
    }
}

fn core_for(game: &Game) -> String {
    game.core.clone().unwrap_or_else(|| {
        System::from_name(&game.system)
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
