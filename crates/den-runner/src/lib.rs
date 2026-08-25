mod config;
mod locate;
mod process;

pub use config::{PlayerBinding, KEYBOARD_CHROME, KEYBOARD_SCHEME, MAX_PLAYERS};
pub use locate::{core_file_name, is_runnable, user_config};
pub use process::Running;

use config::write_config;
use den_db::Game;
use locate::{
    binary_names, candidates, core_argument, core_dir, explicit_override, runnable, runtime_dirs,
    why_not,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(
        "RetroArch was not found. Install it and put `retroarch` on your PATH, \
         or set the RETROARCH environment variable to the binary."
    )]
    NotFound,
    #[error("RETROARCH is set to `{0}`, which {1}")]
    OverrideNotRunnable(String, &'static str),
    #[error("the chosen RetroArch, `{0}`, {1}")]
    ChosenNotRunnable(String, &'static str),
    #[error(
        "the `{core}` core is not installed. RetroArch downloads cores itself: \
         open it, then Main Menu -> Online Updater -> Core Downloader, and get \
         `{core}`. Den looked in {dir}"
    )]
    CoreMissing { core: String, dir: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Chosen,
    Environment,
    Bundled,
    System,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Source::Chosen => "chosen",
            Source::Environment => "environment",
            Source::Bundled => "bundled",
            Source::System => "system",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub source: Source,
}

pub struct Runner {
    pub config_dir: PathBuf,
    pub save_dir: PathBuf,
    pub state_dir: PathBuf,
    managed_dir: PathBuf,
    chosen: Mutex<Option<PathBuf>>,
    bundled: Mutex<Option<PathBuf>>,
}

impl Runner {
    pub fn new(library: &Path, config_dir: &Path) -> Runner {
        Runner {
            config_dir: config_dir.to_path_buf(),
            save_dir: library.join("saves"),
            state_dir: library.join("savestates"),
            managed_dir: library.join("_runtime"),
            chosen: Mutex::new(None),
            bundled: Mutex::new(None),
        }
    }

    pub fn set_chosen(&self, path: Option<PathBuf>) {
        if let Ok(mut chosen) = self.chosen.lock() {
            *chosen = path;
        }
    }

    pub fn chosen(&self) -> Option<PathBuf> {
        self.chosen.lock().ok().and_then(|c| c.clone())
    }

    pub fn managed_dir(&self) -> &Path {
        &self.managed_dir
    }

    pub fn set_bundled_dir(&self, dir: Option<PathBuf>) {
        if let Ok(mut bundled) = self.bundled.lock() {
            *bundled = dir;
        }
    }

    fn bundled_dir(&self) -> Option<PathBuf> {
        self.bundled.lock().ok().and_then(|b| b.clone())
    }

    pub fn locate(&self) -> Result<Found, RunnerError> {
        if let Some(chosen) = self.chosen() {
            return runnable(&chosen)
                .map(|path| Found {
                    path,
                    source: Source::Chosen,
                })
                .ok_or_else(|| {
                    RunnerError::ChosenNotRunnable(chosen.display().to_string(), why_not(&chosen))
                });
        }
        if let Some(raw) = explicit_override() {
            return runnable(&raw)
                .map(|path| Found {
                    path,
                    source: Source::Environment,
                })
                .ok_or_else(|| {
                    RunnerError::OverrideNotRunnable(raw.display().to_string(), why_not(&raw))
                });
        }
        for dir in runtime_dirs(self.bundled_dir(), &self.managed_dir) {
            for name in binary_names() {
                if let Some(path) = runnable(&dir.join(name)) {
                    return Ok(Found {
                        path,
                        source: Source::Bundled,
                    });
                }
            }
        }
        candidates()
            .iter()
            .find_map(|c| runnable(c))
            .map(|path| Found {
                path,
                source: Source::System,
            })
            .ok_or(RunnerError::NotFound)
    }

    pub fn available(&self) -> bool {
        self.locate().is_ok()
    }

    pub fn searched(&self) -> Vec<PathBuf> {
        if let Some(chosen) = self.chosen() {
            return vec![chosen];
        }
        if let Some(raw) = explicit_override() {
            return vec![raw];
        }
        let mut out: Vec<PathBuf> = runtime_dirs(self.bundled_dir(), &self.managed_dir)
            .into_iter()
            .flat_map(|dir| binary_names().iter().map(move |n| dir.join(n)))
            .collect();
        out.extend(candidates());
        out
    }

    pub fn cores_for(&self, retroarch: &Path) -> Option<PathBuf> {
        core_dir(retroarch)
    }

    pub fn core_installed(&self, core: &str) -> Option<bool> {
        let retroarch = self.locate().ok()?;
        let dir = core_dir(&retroarch.path)?;
        Some(dir.join(core_file_name(core)).is_file())
    }

    pub fn launch(
        &self,
        game: &Game,
        core: &str,
        players: &[PlayerBinding],
    ) -> Result<Running, RunnerError> {
        let retroarch = self.locate()?.path;
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.save_dir)?;
        fs::create_dir_all(&self.state_dir)?;

        let cores = core_dir(&retroarch);

        if let Some(dir) = cores.as_deref() {
            let file = core_file_name(core);
            if !dir.join(&file).is_file() {
                return Err(RunnerError::CoreMissing {
                    core: core.to_string(),
                    dir: dir.display().to_string(),
                });
            }
        }

        let config_path = self.config_dir.join(format!("den-{}.cfg", game.id));
        write_config(
            &config_path,
            &self.save_dir,
            &self.state_dir,
            cores.as_deref(),
            user_config(&retroarch).as_deref(),
            players,
        )?;

        let mut cmd = Command::new(&retroarch);
        cmd.arg("-L").arg(core_argument(core, cores.as_deref()));
        cmd.arg("--config").arg(&config_path);
        cmd.arg("--fullscreen");
        cmd.arg(&game.path);
        let child = cmd.spawn().map_err(RunnerError::Io)?;
        Ok(Running { child, config_path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn plant(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn a_missing_core_is_named_before_retroarch_dies_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        plant(&bin_dir, "retroarch");
        fs::write(bin_dir.join(core_file_name("snes9x")), b"x").unwrap();

        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
        runner.set_chosen(Some(bin_dir.join("retroarch")));

        assert_eq!(runner.core_installed("snes9x"), Some(true));
        assert_eq!(runner.core_installed("mupen64plus_next"), Some(false));

        let game = Game {
            id: 1,
            title: "Mario Kart 64".into(),
            system: "N64".into(),
            path: tmp.path().join("mk64.z64").display().to_string(),
            hash: None,
            size: None,
            status: "added".into(),
            core: None,
            art: None,
            created_at: 0,
            updated_at: 0,
            playtime: 0,
            last_played: None,
        };
        let err = match runner.launch(&game, "mupen64plus_next", &[]) {
            Err(e) => e,
            Ok(_) => panic!("a missing core should not have launched"),
        };
        assert!(
            matches!(err, RunnerError::CoreMissing { .. }),
            "expected a named core, got {err}"
        );
        let message = err.to_string();
        assert!(message.contains("mupen64plus_next"), "{message}");
        assert!(message.contains("Core Downloader"), "{message}");

        let mut running = match runner.launch(&game, "snes9x", &[]) {
            Ok(r) => r,
            Err(e) => panic!("an installed core should launch: {e}"),
        };
        running.stop().ok();
    }

    #[test]
    fn the_cores_directory_must_actually_hold_cores() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        let retroarch = plant(&bin_dir, "retroarch");

        fs::create_dir_all(bin_dir.join("cores")).unwrap();
        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
        assert_ne!(
            runner.cores_for(&retroarch),
            Some(bin_dir.join("cores")),
            "an empty directory should not be taken for a cores directory"
        );

        fs::write(bin_dir.join("cores").join(core_file_name("mesen")), b"x").unwrap();
        assert_eq!(runner.cores_for(&retroarch), Some(bin_dir.join("cores")));
    }

    #[test]
    fn cores_beside_the_binary_are_found_when_a_bundle_flattened_them() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("runtime");
        let retroarch = plant(&bin_dir, "retroarch");
        fs::write(bin_dir.join(core_file_name("mupen64plus_next")), b"x").unwrap();
        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
        assert_eq!(runner.cores_for(&retroarch), Some(bin_dir));
    }

    #[test]
    fn a_chosen_path_beats_every_search() {
        let tmp = tempfile::tempdir().unwrap();
        let planted = plant(&tmp.path().join("somewhere/odd"), "retroarch");
        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));

        runner.set_chosen(Some(planted.clone()));
        let found = runner.locate().expect("the chosen path is used");
        assert_eq!(found.source, Source::Chosen);
        assert_eq!(found.path, fs::canonicalize(&planted).unwrap());
        assert_eq!(runner.searched(), vec![planted.clone()]);

        fs::remove_file(&planted).unwrap();
        let err = runner.locate().unwrap_err();
        assert!(matches!(err, RunnerError::ChosenNotRunnable(..)), "{err}");
        assert!(
            err.to_string().contains("is not there any more"),
            "should say what is wrong with it: {err}"
        );
        assert!(err.to_string().contains("retroarch"));
    }

    #[test]
    fn a_bundled_runtime_is_found_under_either_layout() {
        let _env = ENV_LOCK.lock().unwrap();
        for layout in ["runtime", "resources/runtime"] {
            let tmp = tempfile::tempdir().unwrap();
            let resources = tmp.path().join("Resources");
            let planted = plant(&resources.join(layout).join("retroarch"), "retroarch");
            let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
            runner.set_bundled_dir(Some(resources.clone()));
            let found = runner
                .locate()
                .unwrap_or_else(|e| panic!("layout {layout}: {e}"));
            assert_eq!(found.source, Source::Bundled);
            assert_eq!(found.path, fs::canonicalize(&planted).unwrap());
        }
    }

    #[test]
    fn a_bundled_runtime_beats_one_installed_on_the_machine() {
        let _env = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let resources = tmp.path().join("Resources");
        let bundled = plant(&resources.join("runtime/retroarch"), "retroarch");
        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
        runner.set_bundled_dir(Some(resources));
        let found = runner.locate().unwrap();
        assert_eq!(
            found.path,
            fs::canonicalize(&bundled).unwrap(),
            "a build that ships its own RetroArch should use it"
        );
    }

    #[test]
    fn a_runtime_under_the_library_is_found_without_anything_installed() {
        let _env = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("den");
        let runner = Runner::new(&library, &library.join("_config"));
        assert_eq!(runner.managed_dir(), library.join("_runtime"));

        let planted = plant(&library.join("_runtime/retroarch"), "retroarch");
        let found = runner.locate().expect("the managed copy is found");
        assert_eq!(found.source, Source::Bundled);
        assert_eq!(found.path, fs::canonicalize(&planted).unwrap());
    }

    #[test]
    fn binary_lookup() {
        let _env = ENV_LOCK.lock().unwrap();
        assert!(std::env::var_os("RETROARCH").is_none());
        let runner = Runner::new(
            Path::new("/tmp/den-test"),
            Path::new("/tmp/den-test/_config"),
        );

        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("retroarch");
        std::fs::write(&fake, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let relative = relative_to_cwd(&fake);
        std::env::set_var("RETROARCH", &relative);
        let found = runner.locate();
        std::env::remove_var("RETROARCH");
        let found = found.expect("an explicit override is found");
        assert_eq!(found.source, Source::Environment);
        assert!(
            found.path.is_absolute(),
            "{found:?} should have been resolved"
        );
        assert!(found.path.is_file());

        std::env::set_var("RETROARCH", dir.path().join("not-here"));
        let err = runner.locate().unwrap_err();
        std::env::remove_var("RETROARCH");
        assert!(
            matches!(err, RunnerError::OverrideNotRunnable(..)),
            "expected the override to be named, got {err}"
        );
        assert!(err.to_string().contains("not-here"));
    }

    fn relative_to_cwd(target: &Path) -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_default();
        target
            .strip_prefix(&cwd)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| target.to_path_buf())
    }
}
