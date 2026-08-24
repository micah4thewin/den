//! RetroArch process control, private config generation, and a watchdog.
//!
//! Den never reimplements emulation: it launches RetroArch fullscreen with a
//! private config per session, points RetroArch's save and state directories
//! into the library, and watches the process so a core crash never takes the
//! library down.
//!
//! Finding RetroArch is its own small job, and it is done in a fixed order:
//!
//! 1. **Chosen** -- a path the person picked in the interface, kept with the
//!    library. Nothing overrules somebody who pointed at the thing directly.
//! 2. **`RETROARCH`** -- the same statement, made by the environment.
//! 3. **Bundled** -- the copy shipped inside the application, which is what a
//!    release build of Den contains so that nothing has to be installed.
//! 4. **The system** -- `PATH` under every name RetroArch goes by, the places
//!    each platform's installers use, and, on Linux, whatever the desktop
//!    entries say, which is the one list that knows about installs nobody
//!    predicted.
//!
//! The reason for the length of step 4: RetroArch is not always called
//! `retroarch`, it is not always on `PATH`, and an application started from a
//! dock or a launcher does not inherit the `PATH` a terminal would -- on macOS
//! launchd hands it `/usr/bin:/bin:/usr/sbin:/sbin` and nothing else, which is
//! why a Homebrew install is invisible to a GUI app that only asks `PATH`.

use den_db::Game;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;

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
    #[error("RETROARCH is set to `{0}`, which {1}")]
    OverrideNotRunnable(String, &'static str),
    /// A path was chosen in the interface and no longer works.
    #[error("the chosen RetroArch, `{0}`, {1}")]
    ChosenNotRunnable(String, &'static str),
    /// RetroArch is there, but the core this game needs is not installed.
    #[error(
        "the `{core}` core is not installed. RetroArch downloads cores itself: \
         open it, then Main Menu -> Online Updater -> Core Downloader, and get \
         `{core}`. Den looked in {dir}"
    )]
    CoreMissing {
        /// The libretro core Den asked for.
        core: String,
        /// The directory it looked in.
        dir: String,
    },
    /// The filesystem or the spawn said no.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Anything else worth a sentence.
    #[error("{0}")]
    Other(String),
}

/// How the RetroArch in hand was arrived at, so the interface can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Picked in the interface and kept with the library.
    Chosen,
    /// Named by the `RETROARCH` environment variable.
    Environment,
    /// Shipped inside this copy of Den.
    Bundled,
    /// Found already installed on the machine.
    System,
}

impl Source {
    /// The word the interface shows for this source.
    pub fn word(self) -> &'static str {
        match self {
            Source::Chosen => "chosen",
            Source::Environment => "environment",
            Source::Bundled => "bundled",
            Source::System => "system",
        }
    }
}

/// A RetroArch Den can launch, and how it was found.
#[derive(Debug, Clone)]
pub struct Found {
    /// The binary itself.
    pub path: PathBuf,
    /// Where it came from.
    pub source: Source,
}

/// Where RetroArch keeps session state for this library.
pub struct Runner {
    /// Where the private per-session configs are written.
    pub config_dir: PathBuf,
    /// Where RetroArch is pointed for battery saves.
    pub save_dir: PathBuf,
    /// Where RetroArch is pointed for save states.
    pub state_dir: PathBuf,
    /// A copy of RetroArch Den keeps for itself, under the library. This is
    /// where an unbundled build installs one rather than asking the person to.
    managed_dir: PathBuf,
    /// A path chosen in the interface, which outranks every search. Behind a
    /// lock so choosing one is a `&self` operation like everything else the
    /// shell calls through its single library handle.
    chosen: Mutex<Option<PathBuf>>,
    /// Where this build of Den keeps the RetroArch shipped inside it. Only
    /// the shell knows where a bundle put its resources on this platform, so
    /// it tells us rather than this crate guessing at bundle layouts.
    bundled: Mutex<Option<PathBuf>>,
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
            managed_dir: library.join("_runtime"),
            chosen: Mutex::new(None),
            bundled: Mutex::new(None),
        }
    }

    /// Point this runner at a RetroArch somebody picked by hand.
    pub fn set_chosen(&self, path: Option<PathBuf>) {
        if let Ok(mut chosen) = self.chosen.lock() {
            *chosen = path;
        }
    }

    /// The RetroArch chosen by hand, if there is one.
    pub fn chosen(&self) -> Option<PathBuf> {
        self.chosen.lock().ok().and_then(|c| c.clone())
    }

    /// Where Den keeps a RetroArch of its own for this library.
    pub fn managed_dir(&self) -> &Path {
        &self.managed_dir
    }

    /// Tell the runner where this build's bundled runtime was installed.
    pub fn set_bundled_dir(&self, dir: Option<PathBuf>) {
        if let Ok(mut bundled) = self.bundled.lock() {
            *bundled = dir;
        }
    }

    fn bundled_dir(&self) -> Option<PathBuf> {
        self.bundled.lock().ok().and_then(|b| b.clone())
    }

    /// Find RetroArch, now rather than at startup.
    ///
    /// Resolved on every call on purpose: somebody who hits "RetroArch was not
    /// found", installs it, and comes back should be able to press Play, not
    /// have to restart Den first.
    pub fn locate(&self) -> Result<Found, RunnerError> {
        // A statement beats a search, and a broken statement is reported
        // rather than quietly worked around: somebody who pointed Den at a
        // path wants to know that path stopped working.
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

    /// Whether a RetroArch binary can actually be launched.
    pub fn available(&self) -> bool {
        self.locate().is_ok()
    }

    /// Every place `locate` looks, in order. The interface shows this when it
    /// has to tell somebody Den came up empty, because "not found" without
    /// "here is where I looked" is not something anybody can act on.
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

    /// The directory of libretro cores that goes with a given RetroArch, if
    /// one can be found.
    pub fn cores_for(&self, retroarch: &Path) -> Option<PathBuf> {
        core_dir(retroarch)
    }

    /// Whether a named core is installed: `None` when Den cannot tell,
    /// because it never found a cores directory to look in.
    pub fn core_installed(&self, core: &str) -> Option<bool> {
        let retroarch = self.locate().ok()?;
        let dir = core_dir(&retroarch.path)?;
        Some(dir.join(core_file_name(core)).is_file())
    }

    /// Launch a game with a libretro core, fullscreen, private config.
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

        // Say what is wrong before RetroArch does. Handed a core it cannot
        // open, RetroArch dies with `Fatal error received in:
        // "init_libretro_symbols()"`, which tells somebody nothing about
        // which core, or that cores are a thing they have to download.
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

/// The file name of a libretro core on this platform.
pub fn core_file_name(core: &str) -> String {
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

/// RetroArch's own configuration file, if this install has one.
///
/// Worth finding for two reasons. It is the only authority on where this
/// person's cores actually are -- `libretro_directory` is a setting, not
/// something to be guessed at from a list of conventional paths. And Den
/// launches with `--config`, which *replaces* their configuration rather than
/// adding to it, so without reading it first every video, input and shader
/// setting they ever chose is silently dropped for the duration of the game.
pub fn user_config(retroarch: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    // A portable install keeps its config beside the binary.
    if let Some(parent) = retroarch.parent() {
        candidates.push(parent.join("retroarch.cfg"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config/retroarch/retroarch.cfg"));
        candidates
            .push(home.join(".var/app/org.libretro.RetroArch/config/retroarch/retroarch.cfg"));
        candidates.push(home.join("snap/retroarch/current/.config/retroarch/retroarch.cfg"));
        candidates.push(home.join("Library/Application Support/RetroArch/retroarch.cfg"));
    }
    if let Some(config) = dirs::config_dir() {
        candidates.push(config.join("retroarch/retroarch.cfg"));
    }
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("RetroArch/retroarch.cfg"));
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// The value of one key in a RetroArch config: `key = "value"`.
fn config_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            continue;
        }
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() || value == "default" {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

/// A path out of a RetroArch config, with the shorthands it uses expanded:
/// `~` for home, and a leading `:` for RetroArch's own install directory.
fn config_path(raw: &str, retroarch: &Path) -> Option<PathBuf> {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix(":\\").or_else(|| raw.strip_prefix(":/")) {
        return retroarch.parent().map(|d| d.join(rest));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs::home_dir().map(|h| h.join(rest));
    }
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// The directory a RetroArch install keeps its cores in, if one can be found.
fn core_dir(retroarch: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // What RetroArch itself says, before anything Den would guess.
    if let Some(config) = user_config(retroarch) {
        if let Ok(text) = fs::read_to_string(&config) {
            if let Some(dir) =
                config_value(&text, "libretro_directory").and_then(|v| config_path(&v, retroarch))
            {
                candidates.push(dir);
            }
        }
    }

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

    // Last: the binary's own directory. A bundler that flattens the staged
    // tree leaves the cores beside RetroArch rather than under `cores/`, and
    // this is checked last so it never shadows a real cores directory.
    if let Some(parent) = retroarch.parent() {
        candidates.push(parent.to_path_buf());
    }

    // A directory only counts if it actually holds a core; several of the
    // paths above exist on a machine that has no cores in them at all, and
    // naming an empty one in the config would point RetroArch at nothing.
    candidates
        .into_iter()
        .find(|d| d.is_dir() && holds_a_core(d))
}

/// Whether a directory holds at least one libretro core for this platform.
fn holds_a_core(dir: &Path) -> bool {
    let ext = core_file_name("x");
    let ext = ext.rsplit_once('.').map(|(_, e)| e.to_string());
    let Some(ext) = ext else { return false };
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let path = e.path();
        path.extension().and_then(|x| x.to_str()) == Some(ext.as_str())
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("_libretro"))
                .unwrap_or(false)
    })
}

/// `RETROARCH`, if it is set to anything.
fn explicit_override() -> Option<PathBuf> {
    let raw = std::env::var_os("RETROARCH")?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// Whether `path` is a file this process could execute.
pub fn is_runnable(path: &Path) -> bool {
    runnable(path).is_some()
}

/// Why a path is not something Den can run, in words that say what to do.
/// "Not there any more" for a file that is sitting right there, with only its
/// executable bit missing, sends somebody looking in the wrong place.
pub fn why_not(path: &Path) -> &'static str {
    let resolved = app_bundle_binary(path).unwrap_or_else(|| path.to_path_buf());
    if !resolved.exists() {
        "is not there any more"
    } else if resolved.is_dir() {
        "is a directory, not a program"
    } else if !is_executable(&resolved) {
        "is not marked executable"
    } else {
        "cannot be run"
    }
}

/// `path` as an absolute path, if it is something this process could execute.
///
/// Absolute, so `available()` and `launch()` cannot disagree because the
/// working directory moved between them -- but **not** canonicalized. The
/// Flatpak and Snap wrappers are symlinks to a multiplexer (`/usr/bin/flatpak`,
/// `/usr/bin/snap`) that decides what to run by looking at the name it was
/// invoked under. Resolving the link throws that name away and leaves Den
/// spawning the multiplexer with RetroArch's arguments, which exits with a
/// usage message while Den reports a successful launch.
fn runnable(path: &Path) -> Option<PathBuf> {
    let path = app_bundle_binary(path).unwrap_or_else(|| path.to_path_buf());
    if !path.is_file() || !is_executable(&path) {
        return None;
    }
    Some(absolute(&path))
}

/// `path` made absolute without following any symlink along the way.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// The program inside a macOS application bundle.
///
/// A file dialog on macOS hands back `/Applications/RetroArch.app`, because
/// that is what a person sees and clicks. It is a directory, so taking it at
/// face value means the one way out of "RetroArch was not found" refuses the
/// only answer the platform will give.
fn app_bundle_binary(path: &Path) -> Option<PathBuf> {
    if path.extension()? != "app" || !path.is_dir() {
        return None;
    }
    let macos = path.join("Contents").join("MacOS");
    // The bundle's own name first -- RetroArch.app/Contents/MacOS/RetroArch --
    // then whatever single program is in there.
    let stem = path.file_stem()?.to_owned();
    let named = macos.join(&stem);
    if named.is_file() {
        return Some(named);
    }
    fs::read_dir(&macos)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && is_executable(p))
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
        out.push(PathBuf::from("/opt/retroarch/retroarch"));
        out.push(PathBuf::from("/opt/RetroArch/retroarch"));
    }

    // Last, because it is the widest net and the slowest: anything the
    // desktop entries point at.
    out.extend(from_desktop_entries());

    dedup_keeping_order(&mut out);
    out
}

/// The directories that may hold a RetroArch shipped with Den, or one Den
/// installed for itself.
///
/// The shell passes the bundled directory in, because only it can ask Tauri
/// where this platform's bundle put its resources; keeping that knowledge out
/// of here is what lets this crate stay headless. `DEN_RUNTIME_DIR` does the
/// same job for anything without a shell -- `den-doctor`, a test, a script.
fn runtime_dirs(bundled: Option<PathBuf>, managed: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // The shell hands us the resource directory. Where inside it a bundler
    // put a resource depends on the bundler and the platform, so rather than
    // betting on one layout we look at the two it can be, plus the directory
    // itself for a staged tree that was copied in whole.
    if let Some(dir) = bundled {
        for base in [dir.join("runtime"), dir.join("resources/runtime"), dir] {
            out.push(base.join("retroarch"));
            out.push(base);
        }
    }
    if let Some(dir) = std::env::var_os("DEN_RUNTIME_DIR") {
        let dir = PathBuf::from(dir);
        out.push(dir.join("retroarch"));
        out.push(dir);
    }
    // Beside the executable, which covers a portable build and a dev run.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("runtime/retroarch"));
            out.push(dir.join("runtime"));
            // macOS: Den.app/Contents/MacOS/den -> Contents/Resources
            if let Some(contents) = dir.parent() {
                out.push(contents.join("Resources/runtime/retroarch"));
                out.push(contents.join("Resources/runtime"));
            }
        }
    }
    out.push(managed.join("retroarch"));
    out.push(managed.to_path_buf());
    out.retain(|d| !d.as_os_str().is_empty());
    dedup_keeping_order(&mut out);
    out
}

/// Every RetroArch a desktop entry knows about.
///
/// This is the list that catches an install nobody predicted -- a package from
/// a third-party repository, an AppImage someone registered, a build in
/// `/opt`. If it can be started from the applications menu, its `Exec=` line
/// says where it is.
#[cfg(target_os = "linux")]
fn from_desktop_entries() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }

    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !name.to_ascii_lowercase().contains("retroarch") || !name.ends_with(".desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            out.extend(exec_paths(&text));
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn from_desktop_entries() -> Vec<PathBuf> {
    Vec::new()
}

/// The program named by each `Exec=` line in a desktop entry.
///
/// `Exec` carries arguments and `%`-codes after the program; only the first
/// token is a path, and it is only useful to us if it is an absolute one --
/// a bare `retroarch` here tells us nothing `PATH` did not already.
///
/// The program must also *be* RetroArch rather than something that knows how
/// to start it. A Flatpak entry reads `Exec=/usr/bin/flatpak run
/// org.libretro.RetroArch`, and the first token there is `flatpak`: running
/// that on its own, with the arguments Den appends instead of its own, gets
/// you flatpak's usage message and no emulator. The exported wrapper under
/// `/var/lib/flatpak/exports/bin` is the one that behaves like RetroArch, and
/// the candidate list already names it directly.
fn exec_paths(desktop_entry: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in desktop_entry.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("Exec=") else {
            continue;
        };
        let program = match value.trim().strip_prefix('"') {
            // A quoted program may contain spaces.
            Some(rest) => rest.split('"').next().unwrap_or_default(),
            None => value.split_whitespace().next().unwrap_or_default(),
        };
        let program = PathBuf::from(program);
        if program.is_absolute() && names_retroarch(&program) {
            out.push(program);
        }
    }
    out
}

/// Whether a path's own file name says it is RetroArch.
fn names_retroarch(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().contains("retroarch"))
        .unwrap_or(false)
}

/// Drop repeats while keeping the order, which is the whole point of the
/// list: `Vec::dedup` only removes neighbours, and sorting would throw the
/// priority away.
fn dedup_keeping_order(paths: &mut Vec<PathBuf>) {
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
}

/// `home/suffix`, when there is a home directory to hang it on.
fn under(home: &Option<PathBuf>, suffix: &str) -> Option<PathBuf> {
    home.as_ref().map(|h| h.join(suffix))
}

/// One player's controls for a session.
#[derive(Debug, Clone)]
pub struct PlayerBinding {
    /// Which player, from 1.
    pub player: usize,
    /// The joystick index RetroArch should use for them, if a pad is theirs.
    pub joypad_index: Option<usize>,
}

/// Den's keyboard scheme for player one.
///
/// RetroArch has defaults, but nobody can see them, and a launcher whose
/// answer to "how do I play this" is "read the emulator's settings" has not
/// answered. These are written into every session and shown on the
/// Controllers screen, so what the interface says and what the keys do are
/// the same thing by construction.
///
/// The layout is the one an emulator keyboard player expects: the arrow keys
/// for the pad, the right hand on Z/X/A/S for the face buttons in the shape
/// they sit on a controller, Enter and Right Shift for Start and Select.
pub const KEYBOARD_SCHEME: &[(&str, &str, &str)] = &[
    ("up", "up", "Up"),
    ("down", "down", "Down"),
    ("left", "left", "Left"),
    ("right", "right", "Right"),
    ("b", "z", "Z"),
    ("a", "x", "X"),
    ("y", "a", "A"),
    ("x", "s", "S"),
    ("l", "q", "Q"),
    ("r", "w", "W"),
    ("start", "enter", "Enter"),
    ("select", "rshift", "Right Shift"),
];

/// The keys Den binds that are not one player's buttons.
pub const KEYBOARD_CHROME: &[(&str, &str, &str)] = &[
    ("input_menu_toggle", "f1", "F1"),
    ("input_exit_emulator", "escape", "Escape"),
    ("input_save_state", "f2", "F2"),
    ("input_load_state", "f4", "F4"),
    ("input_toggle_fullscreen", "f11", "F11"),
];

/// The keys Den sets for a session. Everything else is the person's own.
const DEN_KEYS: &[&str] = &[
    "video_fullscreen",
    "input_autodetect_enable",
    "savestate_auto_save",
    "savestate_auto_load",
    "savestate_auto_save_interval",
    "savefile_directory",
    "savestate_directory",
    "rgui_show_start_screen",
    "libretro_directory",
];

/// Write the private config for one session.
///
/// `--config` replaces RetroArch's configuration rather than adding to it, so
/// this starts from the person's own file and overrides only the handful of
/// keys Den has an opinion about. Otherwise every launch through Den would
/// quietly discard their video driver, their pad bindings, their shaders --
/// everything they set up in RetroArch itself.
fn write_config(
    path: &Path,
    save_dir: &Path,
    state_dir: &Path,
    core_dir: Option<&Path>,
    inherit: Option<&Path>,
    players: &[PlayerBinding],
) -> std::io::Result<()> {
    let mut content = String::new();
    let overridden = den_keys(players);
    if let Some(theirs) = inherit.and_then(|p| fs::read_to_string(p).ok()) {
        content.push_str("# Den session config: their RetroArch settings, then ours.\n");
        for line in theirs.lines() {
            let key = line.split_once('=').map(|(k, _)| k.trim()).unwrap_or("");
            if overridden.iter().any(|k| k == key) {
                continue; // set below, so it is not set twice
            }
            content.push_str(line);
            content.push('\n');
        }
        content.push('\n');
    }
    content.push_str("# Den, for this session\n");
    content.push_str(&format!(
        "video_fullscreen = \"true\"\n\
         input_autodetect_enable = \"true\"\n\
         savestate_auto_save = \"true\"\n\
         savestate_auto_load = \"false\"\n\
         savestate_auto_save_interval = \"60\"\n\
         savefile_directory = \"{}\"\n\
         savestate_directory = \"{}\"\n\
         rgui_show_start_screen = \"false\"\n",
        save_dir.display(),
        state_dir.display()
    ));
    if let Some(cores) = core_dir {
        content.push_str(&format!("libretro_directory = \"{}\"\n", cores.display()));
    }

    // Which pad answers for which player. RetroArch's udev joypad driver
    // fills its slots densely from 0 in attach order, so the index den-input
    // reports is the pad's rank among attached pads, not its js number.
    content.push_str("\n# Den, controllers\n");
    content.push_str(&format!("input_max_users = \"{}\"\n", MAX_PLAYERS));
    for binding in players {
        if let Some(index) = binding.joypad_index {
            content.push_str(&format!(
                "input_player{}_joypad_index = \"{index}\"\n",
                binding.player
            ));
        }
    }

    // And the keyboard, always, so there is a way to play whether or not a
    // pad turned up. These are player one's keys; a pad assigned to player
    // one works alongside them rather than instead of them.
    content.push_str("\n# Den, keyboard for player one\n");
    for (button, key, _shown) in KEYBOARD_SCHEME {
        content.push_str(&format!("input_player1_{button} = \"{key}\"\n"));
    }
    for (setting, key, _shown) in KEYBOARD_CHROME {
        content.push_str(&format!("{setting} = \"{key}\"\n"));
    }
    fs::write(path, content)
}

/// Every key this session's config sets, so none of them is inherited twice.
fn den_keys(players: &[PlayerBinding]) -> Vec<String> {
    let mut keys: Vec<String> = DEN_KEYS.iter().map(|k| k.to_string()).collect();
    keys.push("input_max_users".to_string());
    for player in 1..=MAX_PLAYERS {
        keys.push(format!("input_player{player}_joypad_index"));
    }
    for (button, _, _) in KEYBOARD_SCHEME {
        keys.push(format!("input_player1_{button}"));
    }
    for (setting, _, _) in KEYBOARD_CHROME {
        keys.push(setting.to_string());
    }
    let _ = players;
    keys
}

/// How many players a session is configured for.
pub const MAX_PLAYERS: usize = 4;

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
            None,
            &[],
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
            None,
            &[],
        )
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("libretro_directory = \"/usr/lib/libretro\""));
    }

    #[test]
    fn a_missing_core_is_named_before_retroarch_dies_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        plant(&bin_dir, "retroarch");
        // A cores directory with one core in it, so it counts as one.
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

        // And the core that is there launches.
        let mut running = match runner.launch(&game, "snes9x", &[]) {
            Ok(r) => r,
            Err(e) => panic!("an installed core should launch: {e}"),
        };
        running.stop().ok();
    }

    #[test]
    fn a_session_keeps_the_settings_the_person_chose() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("retroarch.cfg");
        fs::write(
            &theirs,
            "# their config\n\
             video_driver = \"vulkan\"\n\
             input_player1_a = \"x\"\n\
             video_fullscreen = \"false\"\n\
             libretro_directory = \"/somewhere/else\"\n\
             video_shader_enable = \"true\"\n",
        )
        .unwrap();

        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            Some(Path::new("/usr/lib/libretro")),
            Some(&theirs),
            &[],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();

        // Theirs, kept.
        assert!(text.contains("video_driver = \"vulkan\""));
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("video_shader_enable = \"true\""));
        // Ours, and only once, so RetroArch cannot read the wrong one.
        assert_eq!(text.matches("video_fullscreen =").count(), 1);
        assert!(text.contains("video_fullscreen = \"true\""));
        assert_eq!(text.matches("libretro_directory =").count(), 1);
        assert!(text.contains("libretro_directory = \"/usr/lib/libretro\""));
    }

    #[test]
    fn a_session_always_has_keys_to_play_with() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            None,
            &[],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        // With no pad at all there is still a way to play, and a way out.
        assert!(text.contains("input_player1_up = \"up\""));
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("input_player1_start = \"enter\""));
        assert!(text.contains("input_exit_emulator = \"escape\""));
        assert!(!text.contains("input_player1_joypad_index"));
    }

    #[test]
    fn a_pad_is_told_to_retroarch_as_the_player_it_answers_for() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            None,
            &[
                PlayerBinding {
                    player: 1,
                    joypad_index: Some(2),
                },
                PlayerBinding {
                    player: 2,
                    joypad_index: Some(0),
                },
                // A pad with no joystick node has no index to give.
                PlayerBinding {
                    player: 3,
                    joypad_index: None,
                },
            ],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(
            text.contains("input_player1_joypad_index = \"2\""),
            "{text}"
        );
        assert!(
            text.contains("input_player2_joypad_index = \"0\""),
            "{text}"
        );
        assert!(!text.contains("input_player3_joypad_index"), "{text}");
        assert!(text.contains("input_max_users = \"4\""));
        // The keyboard is still there beside the pad.
        assert!(text.contains("input_player1_up = \"up\""));
    }

    #[test]
    fn ours_replace_theirs_rather_than_being_set_twice() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("retroarch.cfg");
        fs::write(
            &theirs,
            "input_player1_a = \"k\"\n\
             input_player1_joypad_index = \"7\"\n\
             input_exit_emulator = \"nul\"\n\
             input_max_users = \"1\"\n\
             video_driver = \"gl\"\n",
        )
        .unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            Some(&theirs),
            &[PlayerBinding {
                player: 1,
                joypad_index: Some(0),
            }],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        for key in [
            "input_player1_a",
            "input_player1_joypad_index",
            "input_exit_emulator",
            "input_max_users",
        ] {
            assert_eq!(
                text.matches(&format!("{key} =")).count(),
                1,
                "{key} is set twice, and RetroArch would take the wrong one"
            );
        }
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("input_player1_joypad_index = \"0\""));
        // And what Den has no opinion about is left alone.
        assert!(text.contains("video_driver = \"gl\""));
    }

    #[test]
    fn a_config_value_is_read_the_way_retroarch_writes_it() {
        let text = "# comment\n\
             libretro_directory = \"/home/you/.config/retroarch/cores\"\n\
             video_driver = \"gl\"\n\
             cache_directory = \"default\"\n\
             empty_thing = \"\"\n";
        assert_eq!(
            config_value(text, "libretro_directory").as_deref(),
            Some("/home/you/.config/retroarch/cores")
        );
        // RetroArch writes "default" and "" for a setting nobody has set;
        // taking either literally would point Den at a directory named
        // "default".
        assert_eq!(config_value(text, "cache_directory"), None);
        assert_eq!(config_value(text, "empty_thing"), None);
        assert_eq!(config_value(text, "nothing_like_this"), None);
    }

    #[test]
    fn config_paths_expand_the_shorthands_retroarch_uses() {
        let retroarch = Path::new("/opt/RetroArch/retroarch");
        assert_eq!(
            config_path(":/cores", retroarch),
            Some(PathBuf::from("/opt/RetroArch/cores")),
            "a leading colon is RetroArch's own directory"
        );
        assert_eq!(
            config_path("/usr/lib/libretro", retroarch),
            Some(PathBuf::from("/usr/lib/libretro"))
        );
        if let Some(home) = dirs::home_dir() {
            assert_eq!(config_path("~/cores", retroarch), Some(home.join("cores")));
        }
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

    /// A stand-in RetroArch: a real, executable file.
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
    fn the_cores_directory_must_actually_hold_cores() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        let retroarch = plant(&bin_dir, "retroarch");

        // An empty `cores/` beside the binary is not the answer: naming it in
        // the config would point RetroArch at nothing.
        fs::create_dir_all(bin_dir.join("cores")).unwrap();
        let runner = Runner::new(tmp.path(), &tmp.path().join("_config"));
        assert_ne!(
            runner.cores_for(&retroarch),
            Some(bin_dir.join("cores")),
            "an empty directory should not be taken for a cores directory"
        );

        // One with a core in it is.
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

        // And when it stops working, it says so by name rather than falling
        // back to a search the person did not ask for.
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
        // Which of these a bundler produces depends on the bundler and the
        // platform, and this crate cannot ask; it looks at both.
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
    fn exec_lines_yield_retroarch_programs_only() {
        let entry = "[Desktop Entry]\n\
             Name=RetroArch\n\
             Exec=/usr/local/bin/retroarch %f\n\
             Exec=retroarch\n\
             Exec=\"/opt/My Emulators/retroarch\" --fullscreen %U\n\
             Exec=/usr/bin/flatpak run --branch=stable org.libretro.RetroArch\n\
             Exec=/usr/bin/env RETROARCH=1 /usr/bin/retroarch\n";
        assert_eq!(
            exec_paths(entry),
            vec![
                // Absolute and named for what it is.
                PathBuf::from("/usr/local/bin/retroarch"),
                PathBuf::from("/opt/My Emulators/retroarch"),
            ],
            "a launcher that knows how to start RetroArch is not RetroArch: \
             running `flatpak` or `env` with Den's arguments appended gets a \
             usage message and no emulator"
        );
    }

    #[test]
    fn the_search_list_has_no_repeats() {
        let mut paths = vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/a"),
            PathBuf::from("/c"),
            PathBuf::from("/b"),
        ];
        dedup_keeping_order(&mut paths);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ],
            "order is the priority, so it has to survive deduplication"
        );

        // And the real list, which is built from overlapping sources.
        let places = candidates();
        let mut unique = places.clone();
        dedup_keeping_order(&mut unique);
        assert_eq!(places.len(), unique.len(), "the search list repeats itself");
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
        assert_eq!(found.source, Source::Environment);
        assert!(
            found.path.is_absolute(),
            "{found:?} should have been resolved"
        );
        assert!(found.path.is_file());

        // An override that points nowhere says so, rather than quietly
        // falling back to a PATH lookup the person did not ask for.
        std::env::set_var("RETROARCH", dir.path().join("not-here"));
        let err = runner.locate().unwrap_err();
        std::env::remove_var("RETROARCH");
        assert!(
            matches!(err, RunnerError::OverrideNotRunnable(..)),
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
