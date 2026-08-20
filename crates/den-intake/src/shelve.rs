//! Shelving: canonical names, hash dedupe, BIOS filing, and _extras.
//!
//! Originals (in staging) are read-only here; every copy lands in the
//! library before the staging copy is considered done.

use crate::identify::{identify, is_disc_system, FileKind, Identification};
use crate::unpack::{is_disc_ext, is_rider_ext, is_save_ext};
use crate::util::{clean_title, ext_of, sanitize, stem_of};
use crate::{Outcome, ReportEntry};
use den_db::Db;
use den_ident::dat::Index;
use den_ident::hash;
use den_ident::System;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Shelves leaf files into a library directory, tracking dedupe state for the
/// duration of one intake run.
pub struct Shelf<'a> {
    library: PathBuf,
    dat: &'a Index,
    db: Option<&'a Db>,
    bios: crate::bios::BiosIndex,
    seen_hashes: HashSet<String>,
    shelved: HashMap<String, Shelved>,
    /// The hashes already in the library when this run started. Read once:
    /// asking the database per file turned intake into an O(n^2) table scan.
    known_hashes: Option<HashSet<String>>,
}

/// Where a title ended up, so a rider or a save that names the same game can
/// be filed beside it rather than in the library-wide `_extras`.
struct Shelved {
    dir: PathBuf,
    game_id: Option<i64>,
}

impl<'a> Shelf<'a> {
    /// A shelf over `library`, naming games from `dat` and recording them in
    /// `db` when one is given.
    pub fn new(library: &Path, dat: &'a Index, db: Option<&'a Db>) -> Self {
        Shelf {
            library: library.to_path_buf(),
            dat,
            db,
            bios: crate::bios::BiosIndex::bundled(),
            seen_hashes: HashSet::new(),
            shelved: HashMap::new(),
            known_hashes: None,
        }
    }

    /// Record where a title landed, so its riders and saves can follow it.
    fn remember(&mut self, title: &str, dir: &Path, game_id: Option<i64>) {
        let key = title.to_ascii_lowercase();
        // A later id is better than none: the same title can be remembered
        // once as a bare directory and again once the row exists.
        let entry = self.shelved.entry(key).or_insert_with(|| Shelved {
            dir: dir.to_path_buf(),
            game_id: None,
        });
        entry.dir = dir.to_path_buf();
        if game_id.is_some() {
            entry.game_id = game_id;
        }
    }

    /// Shelve every leaf file, grouping multi-disc sets first. Every input
    /// file leaves this with exactly one word against it.
    pub fn shelve_all(&mut self, leaves: &[PathBuf]) -> Vec<ReportEntry> {
        let mut entries = Vec::new();
        let mut consumed_cues: HashSet<PathBuf> = HashSet::new();
        let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
        // Riders and saves are filed last, whatever order they arrived in, so
        // that the game they belong beside has already claimed its shelf.
        let mut others: Vec<PathBuf> = Vec::new();
        let mut riders: Vec<PathBuf> = Vec::new();

        for leaf in leaves {
            let ext = ext_of(leaf);
            // A BIOS is a BIOS even when it wears a disc extension: every
            // bundled name ends in `.bin`, which also belongs to Sega CD and
            // PlayStation.
            let is_bios = self.bios.matches_name(leaf).is_some();
            if is_disc_ext(&ext) && ext != "cue" && !is_bios {
                groups.entry(disc_key(leaf)).or_default().push(leaf.clone());
            } else if !is_bios && is_rider_first(&ext) {
                riders.push(leaf.clone());
            } else {
                others.push(leaf.clone());
            }
        }

        // Sets before singles, and both in a settled order, so a report reads
        // the same way twice over the same drop.
        let mut grouped: Vec<Vec<PathBuf>> = groups.into_values().collect();
        for files in grouped.iter_mut() {
            files.sort();
        }
        grouped.sort();

        for files in grouped {
            if files.len() > 1 {
                entries.extend(self.shelve_multi_disc(&files));
            } else {
                entries.push(self.shelve_disc(&files[0]));
            }
            for f in &files {
                for cue in sibling_cues(f) {
                    consumed_cues.insert(cue);
                }
            }
        }

        for leaf in others.into_iter().chain(riders) {
            if ext_of(&leaf) == "cue" {
                if consumed_cues.contains(&leaf) {
                    continue; // accounted for with its disc
                }
                entries.push(self.shelve_orphan_cue(&leaf));
            } else {
                entries.push(self.shelve_file(&leaf));
            }
        }
        entries
    }

    fn shelve_file(&mut self, src: &Path) -> ReportEntry {
        let ident = match identify(src) {
            Ok(i) => i,
            Err(e) => return self.quarantine(src, &e.to_string()),
        };
        match ident.kind {
            FileKind::Rom | FileKind::Executable => self.shelve_rom(src, &ident),
            FileKind::Disc => self.shelve_disc(src),
            FileKind::Bios => self.shelve_bios(src),
            FileKind::Rider => self.shelve_extra(src, "rider file"),
            FileKind::Save => self.shelve_extra(src, "imported save"),
            FileKind::Archive => self.quarantine(src, "archive could not be unpacked"),
            FileKind::Alien => self.unsupported(src, "unrecognised format"),
        }
    }

    fn shelve_rom(&mut self, src: &Path, ident: &Identification) -> ReportEntry {
        let system = ident.system.unwrap_or(System::Nes);
        let sha1 = hash::sha1_file(src).unwrap_or_default();
        let dat_entry = self.dat.lookup(&sha1);
        let title = dat_entry
            .map(|e| e.title.clone())
            .unwrap_or_else(|| clean_title(&stem_of(src)));
        let system = dat_entry.and_then(Index::system_of).unwrap_or(system);
        let probable = dat_entry.is_none();
        let file_name = file_name_of(src);

        let already =
            !sha1.is_empty() && (self.seen_hashes.contains(&sha1) || self.db_has_hash(&sha1));
        let game_dir = self.library.join(system.name()).join(sanitize(&title));
        fs::create_dir_all(&game_dir).ok();
        self.remember(&title, &game_dir, None);

        if already {
            self.file_variant(src, &game_dir, &file_name);
            if !sha1.is_empty() {
                self.seen_hashes.insert(sha1);
            }
            return ReportEntry {
                input: src.display().to_string(),
                outcome: Outcome::Duplicate { game: title },
            };
        }

        let dest = game_dir.join(&file_name);
        if let Err(e) = fs::copy(src, &dest) {
            return self.quarantine(src, &e.to_string());
        }
        let game_id = self.record_game(&title, system, &dest, Some(&sha1), size_of(src));
        self.remember(&title, &game_dir, game_id);
        if !sha1.is_empty() {
            self.seen_hashes.insert(sha1);
        }

        let outcome = if probable {
            Outcome::Probable {
                game: title.clone(),
            }
        } else {
            Outcome::Added {
                game: title.clone(),
            }
        };
        ReportEntry {
            input: src.display().to_string(),
            outcome,
        }
    }

    fn shelve_disc(&mut self, src: &Path) -> ReportEntry {
        let system = disc_system_for(src);
        let sha1 = hash::sha1_file(src).unwrap_or_default();
        let dat_entry = self.dat.lookup(&sha1);
        let title = dat_entry
            .map(|e| e.title.clone())
            .unwrap_or_else(|| disc_title(src));
        let system = dat_entry.and_then(Index::system_of).unwrap_or(system);
        let probable = dat_entry.is_none();
        let ext = ext_of(src);
        let file_name = file_name_of(src);

        let already =
            !sha1.is_empty() && (self.seen_hashes.contains(&sha1) || self.db_has_hash(&sha1));
        let game_dir = self.library.join(system.name()).join(sanitize(&title));
        fs::create_dir_all(&game_dir).ok();
        self.remember(&title, &game_dir, None);

        if already {
            self.file_variant(src, &game_dir, &file_name);
            if !sha1.is_empty() {
                self.seen_hashes.insert(sha1);
            }
            return ReportEntry {
                input: src.display().to_string(),
                outcome: Outcome::Duplicate { game: title },
            };
        }

        let dest = game_dir.join(&file_name);
        if let Err(e) = fs::copy(src, &dest) {
            return self.quarantine(src, &e.to_string());
        }

        let cues = sibling_cues(src);
        let mut repaired_note = String::new();
        if ext == "bin" && cues.is_empty() && is_disc_system(system) {
            write_cue(&game_dir, &file_name, system);
            repaired_note = "generated missing cue sheet".to_string();
        }
        for cue in &cues {
            if let Some(name) = cue.file_name() {
                fs::copy(cue, game_dir.join(name)).ok();
            }
        }

        let game_id = self.record_game(&title, system, &dest, Some(&sha1), size_of(src));
        self.remember(&title, &game_dir, game_id);
        if !sha1.is_empty() {
            self.seen_hashes.insert(sha1);
        }

        let outcome = if !repaired_note.is_empty() {
            Outcome::Repaired {
                game: title.clone(),
                note: repaired_note,
            }
        } else if probable {
            Outcome::Probable {
                game: title.clone(),
            }
        } else {
            Outcome::Added {
                game: title.clone(),
            }
        };
        ReportEntry {
            input: src.display().to_string(),
            outcome,
        }
    }

    /// Shelve a multi-disc set into one game directory and build the playlist
    /// RetroArch swaps discs from. Every disc in the set gets its own word,
    /// because every disc was its own input file.
    fn shelve_multi_disc(&mut self, files: &[PathBuf]) -> Vec<ReportEntry> {
        let system = disc_system_for(&files[0]);
        let title = disc_title(&files[0]);
        let game_dir = self.library.join(system.name()).join(sanitize(&title));
        fs::create_dir_all(&game_dir).ok();
        self.remember(&title, &game_dir, None);

        let mut notes: Vec<Option<String>> = Vec::with_capacity(files.len());
        for f in files {
            let ext = ext_of(f);
            let name = file_name_of(f);
            if let Err(e) = fs::copy(f, game_dir.join(&name)) {
                notes.push(Some(format!("!{e}")));
                continue;
            }
            let cues = sibling_cues(f);
            let mut note = None;
            if ext == "bin" && cues.is_empty() && is_disc_system(system) {
                write_cue(&game_dir, &name, system);
                note = Some("generated missing cue sheet".to_string());
            }
            for cue in &cues {
                if let Some(cn) = cue.file_name() {
                    fs::copy(cue, game_dir.join(cn)).ok();
                }
            }
            notes.push(note);
        }

        let mut cue_files: Vec<String> = fs::read_dir(&game_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| ext_of(&e.path()) == "cue")
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        cue_files.sort();
        let m3u_path = game_dir.join(format!("{}.m3u", sanitize(&title)));
        let playlist = if cue_files.is_empty() {
            None
        } else {
            fs::write(&m3u_path, cue_files.join("\n") + "\n")
                .ok()
                .map(|()| m3u_path)
        };

        // The game is the playlist when there is one; without cues there is
        // nothing to swap between, so point at the first disc rather than at
        // a path that was never written.
        let content = playlist
            .clone()
            .unwrap_or_else(|| game_dir.join(file_name_of(&files[0])));
        let game_id = self.record_game(&title, system, &content, None, None);
        self.remember(&title, &game_dir, game_id);

        let mut entries = Vec::with_capacity(files.len());
        for (i, f) in files.iter().enumerate() {
            let note = notes.get(i).cloned().flatten();
            if let Some(reason) = note.as_ref().and_then(|n| n.strip_prefix('!')) {
                entries.push(ReportEntry {
                    input: f.display().to_string(),
                    outcome: Outcome::Quarantined {
                        reason: reason.to_string(),
                    },
                });
                continue;
            }
            // The playlist is the set's own repair rather than any one disc's,
            // so it is said once, on the first line of the set.
            let mut notes_for_line: Vec<String> = note.into_iter().collect();
            if i == 0 && playlist.is_some() {
                notes_for_line.push("built multi-disc playlist".to_string());
            }
            let outcome = if notes_for_line.is_empty() {
                Outcome::Added {
                    game: title.clone(),
                }
            } else {
                Outcome::Repaired {
                    game: title.clone(),
                    note: notes_for_line.join(", "),
                }
            };
            entries.push(ReportEntry {
                input: f.display().to_string(),
                outcome,
            });
        }
        entries
    }

    fn shelve_bios(&mut self, src: &Path) -> ReportEntry {
        let sha1 = hash::sha1_file(src).unwrap_or_default();
        let name = self
            .bios
            .by_hash(&sha1)
            .map(str::to_string)
            .or_else(|| self.bios.matches_name(src).map(str::to_string))
            .unwrap_or_else(|| stem_of(src));
        let dir = self.library.join("bios");
        fs::create_dir_all(&dir).ok();
        let file_name = file_name_of(src);
        fs::copy(src, dir.join(&file_name)).ok();
        if let Some(db) = self.db {
            db.add_bios(&name, &dir.join(&file_name), &sha1).ok();
        }
        ReportEntry {
            input: src.display().to_string(),
            outcome: Outcome::Bios { name },
        }
    }

    fn shelve_extra(&mut self, src: &Path, note: &str) -> ReportEntry {
        let file_name = file_name_of(src);
        let stem_key = clean_title(&stem_of(src)).to_ascii_lowercase();
        let owner = self.shelved.get(&stem_key);
        let dest_dir = owner
            .map(|s| s.dir.join("_extras"))
            .unwrap_or_else(|| self.library.join("_extras"));
        let game_id = owner.and_then(|s| s.game_id);
        fs::create_dir_all(&dest_dir).ok();
        let dest = dest_dir.join(&file_name);
        if let Err(e) = fs::copy(src, &dest) {
            return self.quarantine(src, &e.to_string());
        }
        // A save that came in with a game is a save of that game: recording it
        // is what makes the library's Continue row point somewhere.
        let ext = ext_of(src);
        if is_save_ext(&ext) {
            if let (Some(db), Some(id)) = (self.db, game_id) {
                let kind = if ext.starts_with("state") {
                    "state"
                } else {
                    "battery"
                };
                db.add_save(id, kind, &dest).ok();
            }
        }
        ReportEntry {
            input: src.display().to_string(),
            outcome: Outcome::Extra {
                note: note.to_string(),
            },
        }
    }

    fn shelve_orphan_cue(&mut self, src: &Path) -> ReportEntry {
        self.shelve_extra(src, "orphan cue sheet")
    }

    fn file_variant(&self, src: &Path, game_dir: &Path, file_name: &str) {
        let variants = game_dir.join("_variants");
        fs::create_dir_all(&variants).ok();
        let n = count_files(&variants);
        fs::copy(src, variants.join(format!("{}.{}", n + 1, file_name))).ok();
    }

    /// Write the game row, returning its id. Without a database this is a
    /// no-op and intake still shelves the files.
    fn record_game(
        &self,
        title: &str,
        system: System,
        path: &Path,
        sha1: Option<&str>,
        size: Option<i64>,
    ) -> Option<i64> {
        let db = self.db?;
        let hash = sha1.filter(|h| !h.is_empty());
        db.add_game(title, system.name(), path, hash, size, "added")
            .ok()
    }

    fn db_has_hash(&mut self, sha1: &str) -> bool {
        if self.known_hashes.is_none() {
            self.known_hashes = Some(
                self.db
                    .and_then(|db| db.shelved_hashes().ok())
                    .unwrap_or_default(),
            );
        }
        self.known_hashes
            .as_ref()
            .map(|set| set.contains(sha1))
            .unwrap_or(false)
    }

    fn quarantine(&self, src: &Path, reason: &str) -> ReportEntry {
        ReportEntry {
            input: src.display().to_string(),
            outcome: Outcome::Quarantined {
                reason: reason.to_string(),
            },
        }
    }

    fn unsupported(&self, src: &Path, reason: &str) -> ReportEntry {
        ReportEntry {
            input: src.display().to_string(),
            outcome: Outcome::Unsupported {
                reason: reason.to_string(),
            },
        }
    }
}

/// The title for a disc, with any `(Disc N)` marker stripped.
fn disc_title(path: &Path) -> String {
    let stem = stem_of(path);
    let lower = stem.to_ascii_lowercase();
    for p in ["(disc ", "(disk ", "(cd "] {
        if let Some(idx) = lower.find(p) {
            return clean_title(&stem[..idx]);
        }
    }
    clean_title(&stem)
}

/// A grouping key: the lower-cased, disc-marker-free title.
fn disc_key(path: &Path) -> String {
    disc_title(path).to_ascii_lowercase()
}

fn file_name_of(src: &Path) -> String {
    src.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unnamed".to_string())
}

/// Whether an extension belongs to something that rides along with a game
/// rather than being one.
///
/// The same precedence `identify` uses, and for the same reason: `.md` is
/// markdown and it is also a Genesis cartridge, and a system claims an
/// extension before a rider does. This only decides the order files are
/// filed in -- riders last, so the game they name has already claimed its
/// shelf -- so disagreeing with `identify` here would misfile a manual, not a
/// game.
fn is_rider_first(ext: &str) -> bool {
    if System::from_extension(ext).is_some() {
        return false;
    }
    is_save_ext(ext) || is_rider_ext(ext)
}

/// The cue sheet sitting beside a disc image, if its author shipped one.
/// Matched case-insensitively on both the stem and the extension, because a
/// set that came out of a zip made on Windows often disagrees with itself.
fn sibling_cues(disc: &Path) -> Vec<PathBuf> {
    let stem = stem_of(disc).to_ascii_lowercase();
    let dir = disc.parent().unwrap_or_else(|| Path::new("."));
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| ext_of(p) == "cue" && stem_of(p).to_ascii_lowercase() == stem)
        .collect();
    out.sort();
    out
}

fn disc_system_for(src: &Path) -> System {
    let ext = ext_of(src);
    if matches!(ext.as_str(), "bin" | "chd") {
        System::Ps1
    } else {
        System::from_extension(&ext)
            .filter(|s| is_disc_system(*s))
            .unwrap_or(System::Ps1)
    }
}

fn write_cue(game_dir: &Path, bin_name: &str, system: System) {
    let mode = if system == System::Ps1 {
        "MODE2/2352"
    } else {
        "MODE1/2352"
    };
    let stem = Path::new(bin_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "disc".to_string());
    let cue = format!("FILE \"{bin_name}\" BINARY\n  TRACK 01 {mode}\n    INDEX 01 00:00:00\n");
    fs::write(game_dir.join(format!("{stem}.cue")), cue).ok();
}

fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0)
}

fn size_of(src: &Path) -> Option<i64> {
    fs::metadata(src).map(|m| m.len() as i64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_title_strips_marker() {
        assert_eq!(
            disc_title(Path::new("/x/Final Fantasy VII (Disc 1).bin")),
            "Final Fantasy VII"
        );
    }

    #[test]
    fn write_cue_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        write_cue(dir.path(), "Game.bin", System::Ps1);
        let cue = std::fs::read_to_string(dir.path().join("Game.cue")).unwrap();
        assert!(cue.contains("Game.bin"));
    }
}
