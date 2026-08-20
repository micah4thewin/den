//! RetroArch process control, private config generation, and a watchdog.
//!
//! Den never reimplements emulation: it launches RetroArch fullscreen with a
//! private config per session, points RetroArch's save and state directories
//! into the library, and watches the process so a core crash never takes the
//! library down.
//!
//! Finding RetroArch is its own small job. It is not always called
//! `retroarch`, it is not always on `PATH`, and an application started from a
//! dock or a launcher does not inherit the `PATH` a terminal would: on macOS
//! that is `/usr/bin:/bin:/usr/sbin:/sbin` and nothing else, which is why a
//! Homebrew install is invisible to a GUI app that only asks `PATH`. So Den
//! asks `PATH` and then looks in the places the installers actually put it.

use den_db::Game;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Why a launch did not happen.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    /// No RetroArch could be found anywhere Den knows to look.
    #[error(
        "RetroArch was not found. Install it and put `retroarch` on your PATH, \
         or set the RETROARCH environment variable to the binary."
    )]
    NotFound,
    /// `RETROARCH` is set, and points at something that cannot be run.
    #[error("RETROARCH is set to `{0}`, which is not a program Den can run")]
    OverrideNotRunnable(String),
    /// The filesystem or the spawn said no.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Anything else worth a sentence.
    #[error("{0}")]
    Other(String),
}

/// Where RetroArch keeps session state for this library.
pub struct Runner {
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
            config_dir: config_dir.to_path_buf(),
            save_dir: library.join("saves"),
            state_dir: library.join("savestates"),
        }
    }

    /// Find RetroArch, now rather than at startup.
    ///
    /// Resolved on every call on purpose: somebody who hits "RetroArch was not
    /// found", installs it, and comes back should be able to press Play, not
    /// have to restart Den first.
    pub fn locate(&self) -> Result<PathBuf, RunnerError> {
        if let Some(raw) = explicit_override() {
            return runnable(&raw)
                .ok_or_else(|| RunnerError::OverrideNotRunnable(raw.display().to_string()));
        }
        candidates()
            .iter()
            .find_map(|c| runnable(c))
            .ok_or(RunnerError::NotFound)
    }

    /// Whether a RetroArch binary can actually be launched.
    pub fn available(&self) -> bool {
        self.locate().is_ok()
    }

    /// Every place `locate` looks, in order. The interface shows this when it
    /// has to tell somebody Den came up empty, because "not found" without
    /// "here is where I looked" is not something anybody can act on.
    pub fn searched(&self) -> Vec<PathBuf> {
        match explicit_override() {
            Some(raw) => vec![raw],
            None => candidates(),
        }
    }

    /// Launch a game with a libretro core, fullscreen, private config.
    pub fn launch(&self, game: &Game, core: &str) -> Result<Running, RunnerError> {
        let retroarch = self.locate()?;
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.save_dir)?;
        fs::create_dir_all(&self.state_dir)?;

        let cores = core_dir(&retroarch);
        let config_path = self.config_dir.join(format!("den-{}.cfg", game.id));
        write_config(
            &config_path,
            &self.save_dir,
            &self.state_dir,
            cores.as_deref(),
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

/// The file name of a libretro core on this platform.
fn core_file_name(core: &str) -> String {
    let ext = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    format!("{core}_libretro.{ext}")
}

/// What to hand `-L`.
///
/// A full path when the core is where we expected it, because that is the one
/// form every RetroArch build accepts. Otherwise the file name, which a build
/// with a correct `libretro_directory` resolves for itself -- still better
/// than the bare `mesen_libretro` this used to pass, which has no extension
/// and so is not the name of a file on any platform.
fn core_argument(core: &str, cores: Option<&Path>) -> PathBuf {
    let file = core_file_name(core);
    if let Some(dir) = cores {
        let full = dir.join(&file);
        if full.is_file() {
            return full;
        }
    }
    PathBuf::from(file)
}

/// The directory a RetroArch install keeps its cores in, if one can be found.
fn core_dir(retroarch: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Beside the binary is where portable and Windows installs put them.
    if let Some(parent) = retroarch.parent() {
        candidates.push(parent.join("cores"));
        // macOS: .../RetroArch.app/Contents/MacOS/RetroArch
        if let Some(contents) = parent.parent() {
            candidates.push(contents.join("Resources").join("cores"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config/retroarch/cores"));
        // Flatpak and snap keep their own config trees.
        candidates.push(home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores"));
        candidates.push(home.join("snap/retroarch/current/.config/retroarch/cores"));
        candidates.push(home.join("Library/Application Support/RetroArch/cores"));
    }
    if let Some(config) = dirs::config_dir() {
        candidates.push(config.join("retroarch/cores"));
    }
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("RetroArch/cores"));
    }
    candidates.push(PathBuf::from("/usr/lib/libretro"));
    candidates.push(PathBuf::from("/usr/local/lib/libretro"));
    candidates.push(PathBuf::from("/usr/lib/x86_64-linux-gnu/libretro"));

    candidates.into_iter().find(|d| d.is_dir())
}

/// `RETROARCH`, if it is set to anything.
fn explicit_override() -> Option<PathBuf> {
    let raw = std::env::var_os("RETROARCH")?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// `path` as an absolute path, if it is a file this process could execute.
fn runnable(path: &Path) -> Option<PathBuf> {
    if !path.is_file() || !is_executable(path) {
        return None;
    }
    // Absolute, so `available()` and `launch()` cannot disagree because the
    // working directory moved between them.
    Some(fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// The names a RetroArch binary goes by.
fn binary_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["retroarch.exe", "retroarch"]
    } else {
        // The Flatpak build exports a wrapper under its application id rather
        // than as `retroarch`, and that wrapper takes the same arguments.
        &["retroarch", "org.libretro.RetroArch"]
    }
}

/// Every place Den looks for RetroArch, in order: `PATH` first, because a
/// person who put it there meant it, then the places the installers use.
fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in binary_names() {
                out.push(dir.join(name));
            }
        }
    }

    let home = dirs::home_dir();

    if cfg!(target_os = "windows") {
        for var in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "LOCALAPPDATA",
        ] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                out.push(base.join("RetroArch/retroarch.exe"));
                out.push(base.join("RetroArch-Win64/retroarch.exe"));
                out.push(base.join("Programs/RetroArch/retroarch.exe"));
            }
        }
        out.extend(under(&home, "scoop/apps/retroarch/current/retroarch.exe"));
        out.push(PathBuf::from("C:/RetroArch-Win64/retroarch.exe"));
        out.push(PathBuf::from("C:/RetroArch/retroarch.exe"));
    } else if cfg!(target_os = "macos") {
        // An application bundle is never on PATH.
        out.push(PathBuf::from(
            "/Applications/RetroArch.app/Contents/MacOS/RetroArch",
        ));
        out.extend(under(
            &home,
            "Applications/RetroArch.app/Contents/MacOS/RetroArch",
        ));
        // Homebrew, which a GUI app's PATH does not include.
        out.push(PathBuf::from("/opt/homebrew/bin/retroarch"));
        out.push(PathBuf::from("/usr/local/bin/retroarch"));
    } else {
        out.push(PathBuf::from("/usr/bin/retroarch"));
        out.push(PathBuf::from("/usr/local/bin/retroarch"));
        out.push(PathBuf::from("/usr/games/retroarch"));
        out.push(PathBuf::from("/snap/bin/retroarch"));
        // Flatpak's exported wrappers, system-wide and per-user.
        out.push(PathBuf::from(
            "/var/lib/flatpak/exports/bin/org.libretro.RetroArch",
        ));
        out.extend(under(
            &home,
            ".local/share/flatpak/exports/bin/org.libretro.RetroArch",
        ));
        out.extend(under(&home, ".local/bin/retroarch"));
    }

    out.dedup();
    out
}

/// `home/suffix`, when there is a home directory to hang it on.
fn under(home: &Option<PathBuf>, suffix: &str) -> Option<PathBuf> {
    home.as_ref().map(|h| h.join(suffix))
}

fn write_config(
    path: &Path,
    save_dir: &Path,
    state_dir: &Path,
    core_dir: Option<&Path>,
) -> std::io::Result<()> {
    let mut content = format!(
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
    // A config given with --config replaces the user's own, so a build that
    // would have known where its cores live no longer does unless we say.
    if let Some(cores) = core_dir {
        content.push_str(&format!("libretro_directory = \"{}\"\n", cores.display()));
    }
    fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_includes_save_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("/lib/saves"));
        assert!(text.contains("savestate_auto_save_interval"));
        assert!(!text.contains("libretro_directory"));
    }

    #[test]
    fn config_names_the_core_directory_when_one_was_found() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            Some(Path::new("/usr/lib/libretro")),
        )
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("libretro_directory = \"/usr/lib/libretro\""));
    }

    #[test]
    fn a_core_is_named_as_a_file_not_as_a_bare_word() {
        // `-L mesen_libretro` names no file on any platform.
        let bare = core_argument("mesen", None);
        assert_eq!(bare, PathBuf::from(core_file_name("mesen")));
        assert!(bare.to_string_lossy().contains("mesen_libretro."));

        // A core that is really there is passed by its full path.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(core_file_name("snes9x"));
        std::fs::write(&file, b"not really a core").unwrap();
        assert_eq!(core_argument("snes9x", Some(dir.path())), file);

        // A cores directory without this core falls back to the file name.
        assert_eq!(
            core_argument("mgba", Some(dir.path())),
            PathBuf::from(core_file_name("mgba"))
        );
    }

    #[test]
    fn the_search_looks_beyond_path() {
        let places = candidates();
        assert!(
            places.len() > 3,
            "the search should cover more than PATH: {places:?}"
        );
        // Whatever the platform, every candidate is a concrete file to stat.
        assert!(places.iter().all(|p| p.file_name().is_some()));
    }

    // `RETROARCH` is process-global, so everything that reads it lives in one
    // test rather than racing another thread that set it.
    #[test]
    fn binary_lookup() {
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

        // A relative override is resolved rather than reported as missing.
        let relative = relative_to_cwd(&fake);
        std::env::set_var("RETROARCH", &relative);
        let found = runner.locate();
        std::env::remove_var("RETROARCH");
        let found = found.expect("an explicit override is found");
        assert!(found.is_absolute(), "{found:?} should have been resolved");
        assert!(found.is_file());

        // An override that points nowhere says so, rather than quietly
        // falling back to a PATH lookup the person did not ask for.
        std::env::set_var("RETROARCH", dir.path().join("not-here"));
        let err = runner.locate().unwrap_err();
        std::env::remove_var("RETROARCH");
        assert!(
            matches!(err, RunnerError::OverrideNotRunnable(_)),
            "expected the override to be named, got {err}"
        );
        assert!(err.to_string().contains("not-here"));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_named_retroarch_is_not_a_retroarch() {
        let dir = tempfile::tempdir().unwrap();
        let decoy = dir.path().join("retroarch");
        std::fs::create_dir(&decoy).unwrap();
        assert_eq!(runnable(&decoy), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_the_executable_bit_is_not_a_retroarch() {
        let dir = tempfile::tempdir().unwrap();
        let unreadable = dir.path().join("retroarch");
        std::fs::write(&unreadable, b"text").unwrap();
        assert_eq!(runnable(&unreadable), None);
    }

    /// The path to `target` relative to the current directory when that is
    /// possible, and left absolute when it is not.
    fn relative_to_cwd(target: &Path) -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_default();
        target
            .strip_prefix(&cwd)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| target.to_path_buf())
    }
}
