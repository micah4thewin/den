use crate::{with_den, AppState, CommandResult};
use den_core::{
    ControllerInfo, CoreStatus, Game, KeyBinding, LaunchInfo, Report, RetroArchStatus, Save,
};
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[derive(Serialize)]
pub(crate) struct SystemRow {
    pub(crate) name: String,
    pub(crate) count: i64,
}

#[derive(Serialize)]
pub(crate) struct LibraryView {
    pub(crate) path: String,
    pub(crate) games: Vec<Game>,
    pub(crate) systems: Vec<SystemRow>,
    pub(crate) continue_game: Option<Game>,
    pub(crate) recent: Vec<Game>,
    pub(crate) retroarch: RetroArchStatus,
}

#[derive(Serialize)]
pub(crate) struct GameView {
    pub(crate) game: Game,
    pub(crate) saves: Vec<Save>,
    pub(crate) retroarch: RetroArchStatus,
    pub(crate) core: CoreStatus,
}

#[tauri::command]
pub(crate) fn get_library(state: State<'_, AppState>) -> CommandResult<LibraryView> {
    with_den(&state, |den| {
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
        let core = den.core_status(&game);
        Ok(GameView {
            game,
            saves,
            retroarch: den.retroarch_status(),
            core,
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

#[derive(Serialize)]
pub(crate) struct ControllerView {
    pub(crate) pads: Vec<ControllerInfo>,
    pub(crate) keyboard: Vec<KeyBinding>,
    pub(crate) players: usize,
}

#[tauri::command]
pub(crate) fn list_controllers(state: State<'_, AppState>) -> CommandResult<ControllerView> {
    with_den(&state, |den| {
        Ok(ControllerView {
            pads: den.controllers(),
            keyboard: den.keyboard_scheme(),
            players: den_core::MAX_PLAYERS,
        })
    })
}

#[tauri::command]
pub(crate) fn assign_pad(
    state: State<'_, AppState>,
    identity: String,
    player: Option<usize>,
) -> CommandResult<ControllerView> {
    with_den(&state, |den| {
        den.assign_pad(&identity, player)
            .map_err(|e| e.to_string())?;
        Ok(ControllerView {
            pads: den.controllers(),
            keyboard: den.keyboard_scheme(),
            players: den_core::MAX_PLAYERS,
        })
    })
}

#[tauri::command]
pub(crate) fn choose_folder() -> CommandResult<Option<String>> {
    let picked = rfd::FileDialog::new().pick_folder();
    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
pub(crate) fn choose_retroarch(state: State<'_, AppState>) -> CommandResult<RetroArchStatus> {
    let picked = rfd::FileDialog::new()
        .set_title("Choose the RetroArch program")
        .pick_file();
    let Some(picked) = picked else {
        return with_den(&state, |den| Ok(den.retroarch_status()));
    };
    with_den(&state, |den| {
        den.set_retroarch_path(Some(picked.clone()))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub(crate) fn clear_retroarch(state: State<'_, AppState>) -> CommandResult<RetroArchStatus> {
    with_den(&state, |den| {
        den.set_retroarch_path(None).map_err(|e| e.to_string())
    })
}
