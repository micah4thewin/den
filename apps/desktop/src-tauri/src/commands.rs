//! The IPC surface: every command turns a library operation into a
//! serializable answer. The UI holds no file handles and never touches the
//! filesystem directly; it asks Den, which owns the library.

use crate::{with_den, AppState, CommandResult};
use den_core::{ControllerInfo, Game, LaunchInfo, Report, RetroArchStatus, Save};
use serde::Serialize;
use std::path::Path;
use tauri::State;

/// One system shelf and how many games sit on it.
#[derive(Serialize)]
pub(crate) struct SystemRow {
    pub(crate) name: String,
    pub(crate) count: i64,
}

/// Everything the Library screen needs in one call.
#[derive(Serialize)]
pub(crate) struct LibraryView {
    pub(crate) path: String,
    pub(crate) games: Vec<Game>,
    pub(crate) systems: Vec<SystemRow>,
    pub(crate) continue_game: Option<Game>,
    pub(crate) recent: Vec<Game>,
    pub(crate) retroarch: RetroArchStatus,
}

/// One game and its saves, for the Game screen.
#[derive(Serialize)]
pub(crate) struct GameView {
    pub(crate) game: Game,
    pub(crate) saves: Vec<Save>,
    /// Carried here too: Play lives on this screen, so this is where somebody
    /// needs to be told it cannot work yet.
    pub(crate) retroarch: RetroArchStatus,
}

#[tauri::command]
pub(crate) fn get_library(state: State<'_, AppState>) -> CommandResult<LibraryView> {
    with_den(&state, |den| {
        // Any emulator that has quit since the last look closes its session
        // here, so playtime and the Continue row are current by the time the
        // screen is drawn.
        den.reap();
        let games = den.db().list_games("", None).map_err(|e| e.to_string())?;
        let systems = den
            .db()
            .list_systems()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(name, count)| SystemRow { name, count })
            .collect();
        let continue_game = den.db().continue_game().map_err(|e| e.to_string())?;
        let recent = den.db().recent_games(8).map_err(|e| e.to_string())?;
        Ok(LibraryView {
            path: den.library.display().to_string(),
            games,
            systems,
            continue_game,
            recent,
            retroarch: den.retroarch_status(),
        })
    })
}

#[tauri::command]
pub(crate) fn get_game(state: State<'_, AppState>, id: i64) -> CommandResult<GameView> {
    with_den(&state, |den| {
        den.reap();
        let game = den
            .db()
            .get_game(id)
            .map_err(|e| e.to_string())?
            .ok_or("game not found")?;
        let saves = den.db().list_saves(id).map_err(|e| e.to_string())?;
        Ok(GameView {
            game,
            saves,
            retroarch: den.retroarch_status(),
        })
    })
}

#[tauri::command]
pub(crate) fn launch_game(state: State<'_, AppState>, id: i64) -> CommandResult<LaunchInfo> {
    with_den(&state, |den| den.launch(id).map_err(|e| e.to_string()))
}

#[tauri::command]
pub(crate) fn run_intake(
    state: State<'_, AppState>,
    folder: String,
    password: Option<String>,
) -> CommandResult<Report> {
    with_den(&state, |den| {
        den.intake(Path::new(&folder), password)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub(crate) fn list_controllers(state: State<'_, AppState>) -> CommandResult<Vec<ControllerInfo>> {
    with_den(&state, |den| Ok(den.controllers()))
}

#[tauri::command]
pub(crate) fn retroarch_available(state: State<'_, AppState>) -> CommandResult<RetroArchStatus> {
    with_den(&state, |den| Ok(den.retroarch_status()))
}

#[tauri::command]
pub(crate) fn choose_folder() -> CommandResult<Option<String>> {
    let picked = rfd::FileDialog::new().pick_folder();
    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
pub(crate) fn open_library_folder(state: State<'_, AppState>) -> CommandResult<()> {
    with_den(&state, |den| {
        let path = den.library.display().to_string();
        open_in_file_manager(&path)
    })
}

/// Open a directory in the platform's file manager.
fn open_in_file_manager(path: &str) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");

    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        all(unix, not(target_os = "macos"))
    ))]
    {
        // `spawn`, not `status`: a file manager that stays in the foreground
        // would otherwise hold the command -- and the library lock with it --
        // for as long as the window is open.
        cmd.arg(path);
        cmd.spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}
