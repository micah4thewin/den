//! The Den shell (Tauri v2), on desktop.
//!
//! A thin typed IPC layer over `den-core`. The shell depends on `den-core`
//! and nothing below it. Every command turns a library operation into a
//! serializable answer; the UI holds no file handles and never touches the
//! filesystem directly.

mod commands;

use std::sync::Mutex;

use den_core::Den;
use tauri::Manager;

/// The one place application state lives: the opened library.
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

/// Start the shell.
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
            // Only Tauri knows where this platform's bundle put its
            // resources, so the directory is asked for here and handed down
            // rather than guessed at by a crate that has never heard of an
            // application bundle. The runner looks for the runtime staged by
            // tools/bundle_runtime.py inside it.
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
            commands::retroarch_available,
            commands::choose_folder,
            commands::open_library_folder,
            commands::choose_retroarch,
            commands::clear_retroarch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Den shell");
}
