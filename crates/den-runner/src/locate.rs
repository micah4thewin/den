use std::fs;
use std::path::{Path, PathBuf};

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
pub(crate) fn core_argument(core: &str, cores: Option<&Path>) -> PathBuf {
    let file = core_file_name(core);
    if let Some(dir) = cores {
        let full = dir.join(&file);
        if full.is_file() {
            return full;
        }
    }
    PathBuf::from(file)
}

fn install_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".config/retroarch"));
        roots.push(home.join(".var/app/org.libretro.RetroArch/config/retroarch"));
        roots.push(home.join("snap/retroarch/current/.config/retroarch"));
        roots.push(home.join("Library/Application Support/RetroArch"));
    }
    if let Some(config) = dirs::config_dir() {
        roots.push(config.join("retroarch"));
    }
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join("RetroArch"));
    }
    roots
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
    if let Some(parent) = retroarch.parent() {
        candidates.push(parent.join("retroarch.cfg"));
    }
    candidates.extend(install_roots().into_iter().map(|r| r.join("retroarch.cfg")));
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
pub(crate) fn core_dir(retroarch: &Path) -> Option<PathBuf> {
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
    candidates.extend(install_roots().into_iter().map(|r| r.join("cores")));
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
pub(crate) fn explicit_override() -> Option<PathBuf> {
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
pub(crate) fn why_not(path: &Path) -> &'static str {
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
pub(crate) fn runnable(path: &Path) -> Option<PathBuf> {
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
pub(crate) fn binary_names() -> &'static [&'static str] {
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
pub(crate) fn candidates() -> Vec<PathBuf> {
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
pub(crate) fn runtime_dirs(bundled: Option<PathBuf>, managed: &Path) -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
