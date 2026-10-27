use crate::{with_play, AppState, CommandResult};
use play_core::{ControllerInfo, KeyBinding, LaunchInfo, Report, RetroArchStatus};
use play_web::views::{game_view, library_view, GameView, LibraryView};
use play_web::Reachable;
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[tauri::command]
pub(crate) fn get_library(state: State<'_, AppState>) -> CommandResult<LibraryView> {
    with_play(&state, library_view)
}

#[tauri::command]
pub(crate) fn get_game(state: State<'_, AppState>, id: i64) -> CommandResult<GameView> {
    with_play(&state, |play| game_view(play, id))
}

#[tauri::command]
pub(crate) fn web_remote_urls() -> CommandResult<Vec<Reachable>> {
    Ok(match play_web::addr_from_env() {
        Some(addr) => play_web::reachable(addr),
        None => Vec::new(),
    })
}

#[tauri::command]
pub(crate) fn launch_game(state: State<'_, AppState>, id: i64) -> CommandResult<LaunchInfo> {
    with_play(&state, |play| play.launch(id).map_err(|e| e.to_string()))
}

#[derive(Serialize)]
pub(crate) struct StoppedView {
    pub(crate) stopped: usize,
}

#[tauri::command]
pub(crate) fn stop_game(state: State<'_, AppState>, id: i64) -> CommandResult<StoppedView> {
    with_play(&state, |play| {
        Ok(StoppedView {
            stopped: play.stop(id),
        })
    })
}

#[tauri::command]
pub(crate) fn run_intake(
    state: State<'_, AppState>,
    folder: String,
    password: Option<String>,
) -> CommandResult<Report> {
    with_play(&state, |play| {
        play.intake(Path::new(&folder), password)
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
    with_play(&state, |play| {
        Ok(ControllerView {
            pads: play.controllers(),
            keyboard: play.keyboard_scheme(),
            players: play_core::MAX_PLAYERS,
        })
    })
}

#[tauri::command]
pub(crate) fn assign_pad(
    state: State<'_, AppState>,
    identity: String,
    player: Option<usize>,
) -> CommandResult<ControllerView> {
    with_play(&state, |play| {
        play.assign_pad(&identity, player)
            .map_err(|e| e.to_string())?;
        Ok(ControllerView {
            pads: play.controllers(),
            keyboard: play.keyboard_scheme(),
            players: play_core::MAX_PLAYERS,
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
        return with_play(&state, |play| Ok(play.retroarch_status()));
    };
    with_play(&state, |play| {
        play.set_retroarch_path(Some(picked.clone()))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub(crate) fn clear_retroarch(state: State<'_, AppState>) -> CommandResult<RetroArchStatus> {
    with_play(&state, |play| {
        play.set_retroarch_path(None).map_err(|e| e.to_string())
    })
}
