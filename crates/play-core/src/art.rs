//! Covers for the shelf, fetched once per game and kept in the library.

use play_art::{Fetcher, Sources};
use play_db::{now, Db, Game};
use play_ident::System;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// A game with no cover anywhere is asked about again after this long, in
/// case one has been added since.
const RETRY_AFTER: i64 = 14 * 24 * 60 * 60;

const MISS: &str = "art_miss:";

/// The setting that turns cover fetching off: `off`.
pub const COVERS_SETTING: &str = "covers";

static RUNNING: AtomicBool = AtomicBool::new(false);
static AGAIN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArtPass {
    /// Covers fetched and filed.
    pub fetched: usize,
    /// Games no cover was found for.
    pub missing: usize,
    /// Games skipped because they were asked about recently.
    pub skipped: usize,
    /// Set when the network was not there; the rest wait for next time.
    pub offline: Option<String>,
}

pub(crate) fn enabled(db: &Db) -> bool {
    if std::env::var("PLAY_COVERS").is_ok_and(|v| v.trim() == "0" || v.eq_ignore_ascii_case("off"))
    {
        return false;
    }
    !matches!(
        db.setting(COVERS_SETTING).ok().flatten().as_deref(),
        Some("off")
    )
}

/// Look for covers for every game that has none, on a thread of its own, so
/// nothing waits on the network. A second call while one is running makes it
/// go round once more when it is done, so a game shelved meanwhile is seen.
pub(crate) fn in_background(library: PathBuf) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("play-covers".to_string())
        .spawn(move || {
            loop {
                match Db::open(&library.join("library.db")) {
                    Ok(db) if enabled(&db) => {
                        let fetcher = Fetcher::new(Sources::default());
                        let pass = run_pass(&db, &library, &fetcher);
                        if pass.fetched + pass.missing > 0 || pass.offline.is_some() {
                            log::info!(
                                "covers: {} fetched, {} without one{}",
                                pass.fetched,
                                pass.missing,
                                pass.offline
                                    .as_deref()
                                    .map(|o| format!(", stopped: {o}"))
                                    .unwrap_or_default()
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("covers: could not open the library: {e}"),
                }
                if !AGAIN.swap(false, Ordering::SeqCst) {
                    break;
                }
            }
            RUNNING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// One pass over the shelf: every game without a cover gets one if a source
/// has it. Stops at the first sign the network is not there.
pub fn run_pass(db: &Db, library: &Path, fetcher: &Fetcher) -> ArtPass {
    let mut pass = ArtPass::default();
    let games = match db.list_games("", None) {
        Ok(g) => g,
        Err(e) => {
            log::warn!("covers: could not list games: {e}");
            return pass;
        }
    };
    let dir = library.join("_art");
    for game in games {
        if game.art.as_deref().is_some_and(|a| Path::new(a).is_file()) {
            continue;
        }
        let miss_key = format!("{MISS}{}", game.id);
        let last_miss = db
            .setting(&miss_key)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok());
        if last_miss.is_some_and(|t| now() - t < RETRY_AFTER) {
            pass.skipped += 1;
            continue;
        }
        let Some(system) = System::from_name(&game.system) else {
            continue;
        };
        let wanted = wanted_names(&game);
        let wanted: Vec<&str> = wanted.iter().map(String::as_str).collect();
        let Some(name) = play_art::cover_name(system, &wanted) else {
            let _ = db.set_setting(&miss_key, Some(&now().to_string()));
            pass.missing += 1;
            continue;
        };
        match fetcher.fetch(system, &name) {
            Ok(Some(bytes)) => {
                let ext = if bytes.starts_with(&[0xff, 0xd8]) {
                    "jpg"
                } else {
                    "png"
                };
                let path = dir.join(format!("{}.{ext}", game.id));
                let written = fs::create_dir_all(&dir).and_then(|()| {
                    let tmp = path.with_extension("part");
                    fs::write(&tmp, &bytes)?;
                    fs::rename(&tmp, &path)
                });
                match written
                    .ok()
                    .and_then(|()| path.to_str().map(str::to_string))
                {
                    Some(text) => {
                        let _ = db.set_art(game.id, &text);
                        let _ = db.set_setting(&miss_key, None);
                        pass.fetched += 1;
                    }
                    None => log::warn!("covers: could not write {}", path.display()),
                }
            }
            Ok(None) => {
                let _ = db.set_setting(&miss_key, Some(&now().to_string()));
                pass.missing += 1;
            }
            Err(e) => {
                pass.offline = Some(e.to_string());
                break;
            }
        }
    }
    pass
}

/// What a game is known by: its title, and the name of the file it came in
/// as, which is often the full No-Intro name with the region still on.
fn wanted_names(game: &Game) -> Vec<String> {
    let mut out = vec![game.title.clone()];
    if let Some(stem) = Path::new(&game.path).file_stem().and_then(|s| s.to_str()) {
        if !out.iter().any(|o| o == stem) {
            out.insert(0, stem.to_string());
        }
    }
    out
}

/// Where a game's cover is kept, if it has one. Covers are named by game, so
/// finding one needs neither the database nor the lock around it.
pub fn cover_file(library: &Path, game_id: i64) -> Option<PathBuf> {
    let dir = library.join("_art");
    ["png", "jpg"]
        .into_iter()
        .map(|ext| dir.join(format!("{game_id}.{ext}")))
        .find(|p| p.is_file())
}
