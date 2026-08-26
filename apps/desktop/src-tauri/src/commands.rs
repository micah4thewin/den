use crate::{with_den, AppState, CommandResult};
use den_core::{ControllerInfo, KeyBinding, LaunchInfo, Report, RetroArchStatus};
use den_web::views::{game_view, library_view, GameView, LibraryView};
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[tauri::command]
pub(crate) fn get_library(state: State<'_, AppState>) -> CommandResult<LibraryView> {
    with_den(&state, library_view)
}

#[tauri::command]
pub(crate) fn get_game(state: State<'_, AppState>, id: i64) -> CommandResult<GameView> {
    with_den(&state, |den| game_view(den, id))
}

#[tauri::command]
pub(crate) fn web_remote_urls() -> CommandResult<Vec<String>> {
    Ok(match den_web::addr_from_env() {
        Some(addr) => den_web::reachable_urls(addr),
        None => Vec::new(),
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
