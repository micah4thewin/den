mod commands;

use std::sync::{Arc, Mutex};

use play_core::Play;
use tauri::Manager;

struct AppState {
    play: play_web::SharedPlay,
    library: std::path::PathBuf,
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
        // Covers live in the library folder, which the webview cannot read;
        // `cover://localhost/<id>` hands it one game's cover, and nothing else.
        .register_uri_scheme_protocol("cover", |ctx, request| {
            let id = request
                .uri()
                .path()
                .trim_start_matches('/')
                .split(['/', '?'])
                .next()
                .and_then(|s| s.parse::<i64>().ok());
            let state = ctx.app_handle().state::<AppState>();
            // No lock: intake can hold the library for minutes, and a
            // cover should not wait for it.
            let found = id.and_then(|id| {
                let path = play_core::art::cover_file(&state.library, id)?;
                std::fs::read(&path).ok().map(|bytes| (path, bytes))
            });
            let builder = tauri::http::Response::builder();
            match found {
                Some((path, bytes)) => {
                    let mime = if path.extension().is_some_and(|e| e == "jpg") {
                        "image/jpeg"
                    } else {
                        "image/png"
                    };
                    builder
                        .header("Content-Type", mime)
                        .header("Cache-Control", "max-age=86400")
                        .body(bytes)
                }
                None => builder.status(404).body(Vec::new()),
            }
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
        })
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
            play.fetch_art_in_background();
            let library = play.library.clone();
            let shared: play_web::SharedPlay = Arc::new(Mutex::new(play));
            if let Some(addr) = play_web::addr_from_env() {
                let for_server = shared.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = play_web::serve(for_server, addr).await {
                        log::warn!("the web remote could not start on {addr}: {e}");
                    }
                });
            }
            app.manage(AppState {
                play: shared,
                library,
            });
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
            commands::choose_files,
            commands::choose_retroarch,
            commands::clear_retroarch,
            commands::assign_pad,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Play shell");
}
