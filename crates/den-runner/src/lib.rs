//! RetroArch process control, private config generation, and a watchdog.
//!
//! Den never reimplements emulation: it launches RetroArch fullscreen with a
//! private config per session, points RetroArch's save and state directories
//! into the library, and watches the process so a core crash never takes the
//! library down.

use den_db::Game;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Why a launch did not happen.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    /// No RetroArch binary was found to launch.
    #[error("RetroArch binary not found on PATH")]
    NotFound,
    /// The filesystem or the spawn said no.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Anything else worth a sentence.
    #[error("{0}")]
    Other(String),
}

/// Where RetroArch lives and where Den keeps session state.
pub struct Runner {
    /// The RetroArch binary this runner will launch.
    pub retroarch: PathBuf,
    /// Where the private per-session configs are written.
    pub config_dir: PathBuf,
    /// Where RetroArch is pointed for battery saves.
    pub save_dir: PathBuf,
    /// Where RetroArch is pointed for save states.
    pub state_dir: PathBuf,
}

/// A launched RetroArch process, owned here.
pub struct Running {
    child: Child,
    /// The private config this process was launched with.
    pub config_path: PathBuf,
}

impl Running {
    /// The process id of the launched RetroArch.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether the process is still running (non-blocking).
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the process to exit; returns its exit code.
    pub fn wait(&mut self) -> std::io::Result<Option<i32>> {
        match self.child.try_wait()? {
            Some(status) => Ok(status.code()),
            None => Ok(self.child.wait()?.code()),
        }
    }

    /// Stop the process.
    pub fn stop(&mut self) -> std::io::Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Runner {
    /// A runner rooted at a library directory.
    pub fn new(library: &Path, config_dir: &Path) -> Runner {
        Runner {
            retroarch: find_on_path("retroarch").unwrap_or_else(|| PathBuf::from("retroarch")),
            config_dir: config_dir.to_path_buf(),
            save_dir: library.join("saves"),
            state_dir: library.join("savestates"),
        }
    }

    /// Whether a RetroArch binary can actually be launched.
    pub fn available(&self) -> bool {
        self.retroarch.is_absolute() && self.retroarch.is_file()
    }

    /// Launch a game with a libretro core, fullscreen, private config.
    pub fn launch(&self, game: &Game, core: &str) -> Result<Running, RunnerError> {
        if !self.available() {
            return Err(RunnerError::NotFound);
        }
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.save_dir)?;
        fs::create_dir_all(&self.state_dir)?;

        let config_path = self.config_dir.join(format!("den-{}.cfg", game.id));
        write_config(&config_path, &self.save_dir, &self.state_dir)?;

        let mut cmd = Command::new(&self.retroarch);
        cmd.arg("-L").arg(format!("{}_libretro", core));
        cmd.arg("--config").arg(&config_path);
        cmd.arg("--fullscreen");
        cmd.arg(&game.path);
        let child = cmd.spawn().map_err(RunnerError::Io)?;
        Ok(Running { child, config_path })
    }
}

fn write_config(path: &Path, save_dir: &Path, state_dir: &Path) -> std::io::Result<()> {
    let content = format!(
        "# Den private session config (generated)\n\
         video_fullscreen = \"true\"\n\
         input_autodetect_enable = \"true\"\n\
         savestate_auto_save = \"true\"\n\
         savestate_auto_load = \"false\"\n\
         savestate_auto_save_interval = \"60\"\n\
         savefile_directory = \"{}\"\n\
         savestate_directory = \"{}\"\n\
         rgui_show_start_screen = \"false\"\n",
        save_dir.display(),
        state_dir.display()
    );
    fs::write(path, content)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    // An explicit RETROARCH wins, absolute or not: `available()` only trusts
    // an absolute path, so a relative override is resolved here rather than
    // silently reported as missing.
    if let Ok(p) = std::env::var("RETROARCH") {
        if !p.is_empty() {
            let path = PathBuf::from(p);
            return Some(fs::canonicalize(&path).unwrap_or(path));
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_includes_save_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(&cfg, Path::new("/lib/saves"), Path::new("/lib/savestates")).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("/lib/saves"));
        assert!(text.contains("savestate_auto_save_interval"));
    }

    // `RETROARCH` is process-global, so everything that reads it lives in one
    // test rather than racing another thread that set it.
    #[test]
    fn binary_lookup() {
        assert!(std::env::var_os("RETROARCH").is_none());
        assert!(find_on_path("definitely-not-a-real-binary-xyz").is_none());

        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("retroarch");
        std::fs::write(&fake, b"#!/bin/sh\nexit 0\n").unwrap();
        let relative = pathdiff(&fake);
        // SAFETY-adjacent: these two tests are the only readers of the var,
        // and this one restores it before returning.
        std::env::set_var("RETROARCH", &relative);
        let found = find_on_path("retroarch");
        std::env::remove_var("RETROARCH");
        let found = found.expect("an explicit override is always found");
        assert!(found.is_absolute(), "{found:?} should have been resolved");
        assert!(found.is_file());
    }

    /// The path to `target`, expressed relative to the current directory when
    /// that is possible and left absolute when it is not.
    fn pathdiff(target: &Path) -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_default();
        target
            .strip_prefix(&cwd)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| target.to_path_buf())
    }
}
