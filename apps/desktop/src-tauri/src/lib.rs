mod commands;

use std::sync::Mutex;

use den_core::Den;
use tauri::Manager;

struct AppState {
    den: Mutex<Den>,
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
            app.manage(AppState {
                den: Mutex::new(den),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
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
        .expect("error while running the Den shell");
}
