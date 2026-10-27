mod commands;

use std::sync::{Arc, Mutex};

use play_core::Play;
use tauri::Manager;

struct AppState {
    play: play_web::SharedPlay,
}

type CommandResult<T> = Result<T, String>;

fn with_play<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&Play) -> CommandResult<T>,
) -> CommandResult<T> {
    let play = state
        .play
        .lock()
        .map_err(|_| "the library is busy".to_string())?;
    f(&play)
}

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .setup(|app| {
            match Play::adopt_old_library() {
                Ok(Some(old)) => log::info!("moved the library from {}", old.display()),
                Ok(None) => {}
                Err(e) => log::warn!("could not move the old library: {e}"),
            }
            let library = Play::default_library();
            let play = Play::open(&library)
                .map_err(|e| format!("could not open the library at {}: {e}", library.display()))
                .unwrap_or_else(|e| {
                    log::error!("{e}");
                    std::process::exit(1);
                });
            match app.path().resource_dir() {
                Ok(dir) => play.set_runtime_dir(Some(dir)),
                Err(e) => log::warn!("no resource directory: {e}"),
            }
            let shared: play_web::SharedPlay = Arc::new(Mutex::new(play));
            if let Some(addr) = play_web::addr_from_env() {
                let for_server = shared.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = play_web::serve(for_server, addr).await {
                        log::warn!("the web remote could not start on {addr}: {e}");
                    }
                });
            }
            app.manage(AppState { play: shared });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::web_remote_urls,
            commands::get_game,
            commands::launch_game,
            commands::stop_game,
            commands::run_intake,
            commands::list_controllers,
            commands::choose_folder,
            commands::choose_retroarch,
            commands::clear_retroarch,
            commands::assign_pad,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Play shell");
}
