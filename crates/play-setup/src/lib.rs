//! Getting a fresh machine ready to play.
//!
//! A machine with nothing on it but Play is the case this crate is for: it
//! downloads a portable RetroArch when none can be found, and the libretro
//! cores the common systems — and whatever is actually on the shelf — need.
//! Everything lands in the library's `_runtime` directory, where the runner
//! already looks, so nothing is installed system-wide and nothing asks for a
//! password.
//!
//! Downloads happen one at a time. The work runs on its own thread and
//! reports through [`Setup::status`]; a launch that needs a core nobody has
//! fetched yet fetches it on the spot through [`Setup::get_core`].

mod install;
pub mod sources;

use serde::Serialize;
use sources::Target;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use sources::RETROARCH_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error(
        "libretro does not publish a RetroArch for this machine; install the `retroarch` package"
    )]
    NoRetroArchForThisMachine,
    #[error("Play does not know where to get emulators for {0}")]
    UnknownMachine(String),
    #[error("{0} is not there (HTTP {1})")]
    NotPublished(String, u16),
    #[error("could not download {0}: {1}")]
    Network(String, String),
    #[error("{0}")]
    Unusable(String),
    #[error("none of {0} could be downloaded: {1}")]
    NoCore(String, String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// What the setup is doing, for the screen.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SetupStatus {
    /// True while a download is going.
    pub working: bool,
    /// What it is doing right now, in words.
    pub doing: Option<String>,
    /// Systems that are ready, by name.
    pub ready: Vec<String>,
    /// What went wrong, one line each.
    pub problems: Vec<String>,
}

/// A system and the cores that run it, best first.
#[derive(Debug, Clone)]
pub struct Need {
    pub system: String,
    pub cores: Vec<String>,
}

pub struct Setup {
    managed: PathBuf,
    target: Option<Target>,
    status: Mutex<SetupStatus>,
    one_at_a_time: Mutex<()>,
}

impl Setup {
    /// `managed` is the runner's managed runtime directory.
    pub fn new(managed: &Path) -> Setup {
        Setup {
            managed: managed.to_path_buf(),
            target: Target::current(),
            status: Mutex::new(SetupStatus::default()),
            one_at_a_time: Mutex::new(()),
        }
    }

    pub fn cores_dir(&self) -> PathBuf {
        self.managed.join("cores")
    }

    pub fn status(&self) -> SetupStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut SetupStatus)) {
        if let Ok(mut status) = self.status.lock() {
            f(&mut status);
        }
    }

    fn target(&self) -> Result<Target, SetupError> {
        self.target.ok_or_else(|| {
            SetupError::UnknownMachine(format!(
                "{} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        })
    }

    /// Download a portable RetroArch into the managed directory.
    pub fn get_retroarch(&self) -> Result<PathBuf, SetupError> {
        let target = self.target()?;
        let _one = self.one_at_a_time.lock();
        self.update(|s| s.doing = Some(format!("Downloading RetroArch {RETROARCH_VERSION}")));
        let result = install::install_retroarch(target, &self.managed);
        self.update(|s| s.doing = None);
        result
    }

    /// Make sure one of `cores` is in the managed cores directory, trying them
    /// in order; answers with the one that is there.
    pub fn get_core(&self, need: &Need) -> Result<String, SetupError> {
        let target = self.target()?;
        let _one = self.one_at_a_time.lock();
        let dir = self.cores_dir();
        // Somebody else may have fetched it while this waited its turn.
        let ext = target.core_extension();
        if let Some(have) = need
            .cores
            .iter()
            .find(|c| dir.join(format!("{c}_libretro.{ext}")).is_file())
        {
            return Ok(have.clone());
        }
        std::fs::create_dir_all(&dir)?;
        let mut failures = Vec::new();
        for core in &need.cores {
            self.update(|s| s.doing = Some(format!("Getting {} ready ({core})", need.system)));
            match install::install_core(target, core, &dir) {
                Ok(_) => {
                    self.update(|s| s.doing = None);
                    return Ok(core.clone());
                }
                Err(e) => failures.push(e.to_string()),
            }
        }
        self.update(|s| s.doing = None);
        // An empty cores directory would read as "cores live here, and yours
        // is not one of them" to the runner; leave none behind.
        let _ = std::fs::remove_dir(&dir);
        Err(SetupError::NoCore(
            need.cores.join(", "),
            failures.join("; "),
        ))
    }

    /// Do the whole first-run job on its own thread: a RetroArch if
    /// `retroarch` says one is needed, then a core for every need.
    pub fn prepare(self: &Arc<Self>, retroarch: bool, needs: Vec<Need>) {
        if !retroarch && needs.is_empty() {
            return;
        }
        let me = Arc::clone(self);
        me.update(|s| s.working = true);
        std::thread::spawn(move || {
            if retroarch {
                if let Err(e) = me.get_retroarch() {
                    me.update(|s| s.problems.push(format!("RetroArch: {e}")));
                }
            }
            for need in needs {
                match me.get_core(&need) {
                    Ok(_) => me.update(|s| {
                        s.problems
                            .retain(|p| !p.starts_with(&format!("{}:", need.system)));
                        if !s.ready.contains(&need.system) {
                            s.ready.push(need.system.clone());
                        }
                    }),
                    Err(e) => me.update(|s| s.problems.push(format!("{}: {e}", need.system))),
                }
            }
            me.update(|s| {
                s.working = false;
                s.doing = None;
            });
        });
    }

    /// Mark systems as ready without downloading anything, because a core for
    /// them is already somewhere RetroArch can load it from.
    pub fn already_ready(&self, systems: impl IntoIterator<Item = String>) {
        self.update(|s| {
            for system in systems {
                if !s.ready.contains(&system) {
                    s.ready.push(system);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Serve one zip, as the buildbot would, to whoever asks for `wanted`;
    /// anything else gets a 404.
    fn buildbot(wanted: String, zip: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(4) {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 4096];
                let n = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                if path.ends_with(&wanted) {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        zip.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&zip);
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_missing_core_is_downloaded_falling_back_down_the_list() {
        let Some(target) = Target::current() else {
            return;
        };
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let file = format!("parallel_n64_libretro.{}", target.core_extension());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(file.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"a core").unwrap();
        let zip = zip.finish().unwrap().into_inner();

        std::env::set_var("PLAY_BUILDBOT", buildbot(format!("/{file}.zip"), zip));
        let dir = tempfile::tempdir().unwrap();
        let setup = Setup::new(dir.path());
        let got = setup.get_core(&Need {
            system: "N64".into(),
            cores: vec!["mupen64plus_next".into(), "parallel_n64".into()],
        });
        std::env::remove_var("PLAY_BUILDBOT");

        assert_eq!(got.unwrap(), "parallel_n64");
        assert_eq!(
            std::fs::read(setup.cores_dir().join(&file)).unwrap(),
            b"a core"
        );
        assert!(
            !setup.cores_dir().join(".download").exists(),
            "the scratch directory was left behind"
        );
    }

    #[test]
    fn a_core_already_downloaded_is_not_downloaded_again() {
        let Some(target) = Target::current() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let setup = Setup::new(dir.path());
        std::fs::create_dir_all(setup.cores_dir()).unwrap();
        std::fs::write(
            setup
                .cores_dir()
                .join(format!("parallel_n64_libretro.{}", target.core_extension())),
            b"x",
        )
        .unwrap();
        // Pointed at nowhere, so a download would fail rather than happen.
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("PLAY_BUILDBOT", "http://127.0.0.1:9");
        let got = setup
            .get_core(&Need {
                system: "N64".into(),
                cores: vec!["mupen64plus_next".into(), "parallel_n64".into()],
            })
            .unwrap();
        std::env::remove_var("PLAY_BUILDBOT");
        assert_eq!(got, "parallel_n64");
    }

    #[test]
    fn nothing_to_do_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let setup = Arc::new(Setup::new(dir.path()));
        setup.prepare(false, Vec::new());
        assert!(!setup.status().working);
    }
}
