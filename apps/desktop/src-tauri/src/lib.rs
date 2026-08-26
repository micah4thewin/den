mod commands;

use std::sync::{Arc, Mutex};

use den_core::Den;
use tauri::Manager;

struct AppState {
    den: den_web::SharedDen,
}

type CommandResult<T> = Result<T, String>;

fn with_den<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&Den) -> CommandResult<T>,
) -> CommandResult<T> {
    let den = state
        .den
        .lock()
        .map_err(|_| "the library is busy".to_string())?;
    f(&den)
}

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .setup(|app| {
            let library = Den::default_library();
            let den = Den::open(&library)
                .map_err(|e| format!("could not open the library at {}: {e}", library.display()))
                .unwrap_or_else(|e| {
                    log::error!("{e}");
                    std::process::exit(1);
                });
            match app.path().resource_dir() {
                Ok(dir) => den.set_runtime_dir(Some(dir)),
                Err(e) => log::warn!("no resource directory: {e}"),
            }
            let shared: den_web::SharedDen = Arc::new(Mutex::new(den));
            if let Some(addr) = den_web::addr_from_env() {
                let for_server = shared.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = den_web::serve(for_server, addr).await {
                        log::warn!("the web remote could not start on {addr}: {e}");
                    }
                });
            }
            app.manage(AppState { den: shared });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::web_remote_urls,
            commands::get_game,
            commands::launch_game,
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
