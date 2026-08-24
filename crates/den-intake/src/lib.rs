//! Den intake: the pipeline that turns a pile of downloads into a clean,
//! named, playable library. Originals are read-only until a copy is safely
//! shelved, and every file is accounted for with a word.

pub mod bios;
mod identify;
mod shelve;
mod unpack;
mod util;

pub use bios::BiosIndex;
pub use identify::{identify, FileKind, Identification};
pub use shelve::Shelf;
pub use unpack::{unpack_recursive, UnpackError, UnpackFailure};

use den_db::now;
use den_ident::dat::Index;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The word-per-file outcome vocabulary from the build plan: a word, never a
/// colour, never silence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "word", content = "detail", rename_all = "lowercase")]
pub enum Outcome {
    /// Shelved as a new game.
    #[serde(alias = "Added")]
    Added {
        /// The title it was shelved under.
        game: String,
    },
    /// A byte-for-byte copy of a game already on the shelf.
    #[serde(alias = "Duplicate")]
    Duplicate {
        /// The title it duplicates.
        game: String,
    },
    /// Shelved after a repair (a missing .cue was written, for example).
    #[serde(alias = "Repaired")]
    Repaired {
        /// The title it was shelved under.
        game: String,
        /// What the repair was, in words.
        note: String,
    },
    /// Identified only by header or extension, not a hash match.
    #[serde(alias = "Probable")]
    Probable {
        /// The title Den guessed.
        game: String,
    },
    /// A BIOS file, recognised and filed automatically.
    #[serde(alias = "Bios")]
    Bios {
        /// The canonical name for this BIOS.
        name: String,
    },
    /// A rider file (manual, art, readme, save) that rides along.
    #[serde(alias = "Extra")]
    Extra {
        /// What kind of rider it was.
        note: String,
    },
    /// Could not be used, kept with a reason and a retry path.
    #[serde(alias = "Quarantined")]
    Quarantined {
        /// Why it could not be used.
        reason: String,
    },
    /// A format Den does not support.
    #[serde(alias = "Unsupported")]
    Unsupported {
        /// Why the format is out of scope.
        reason: String,
    },
}

impl Outcome {
    /// The single word for this outcome, for the report card tally.
    pub fn word(&self) -> &'static str {
        match self {
            Outcome::Added { .. } => "added",
            Outcome::Duplicate { .. } => "duplicate",
            Outcome::Repaired { .. } => "repaired",
            Outcome::Probable { .. } => "probable",
            Outcome::Bios { .. } => "bios",
            Outcome::Extra { .. } => "extra",
            Outcome::Quarantined { .. } => "quarantined",
            Outcome::Unsupported { .. } => "unsupported",
        }
    }
}

/// One input file and its word.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEntry {
    /// The input file this line accounts for.
    pub input: String,
    /// The word it earned.
    pub outcome: Outcome,
}

/// The intake report card: every input file accounted for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Unix time the run began.
    pub started: i64,
    /// Unix time the run ended.
    pub finished: i64,
    /// One line per input file, in the order they were filed.
    pub entries: Vec<ReportEntry>,
}

impl Report {
    /// Count entries by outcome word.
    pub fn tally(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let mut map = std::collections::BTreeMap::new();
        for e in &self.entries {
            *map.entry(e.outcome.word()).or_insert(0) += 1;
        }
        map
    }
}

/// Everything one intake run needs.
pub struct IntakeOptions<'a> {
    /// The library root that receives shelved games.
    pub library: PathBuf,
    /// The hash database used for exact naming.
    pub dat: &'a Index,
    /// The optional library database for persistence and dedupe.
    pub db: Option<&'a den_db::Db>,
    /// An optional password for encrypted archives.
    pub password: Option<String>,
}

/// Why a whole intake run could not start or finish.
#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    /// Staging the drop, or reading it, failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Run the full intake pipeline over a drop: stage, unpack, identify, repair,
/// shelve, and report. The drop is never modified.
pub fn run_intake(drop: &Path, opts: &IntakeOptions) -> Result<Report, IntakeError> {
    let started = now();
    let staging = tempfile::tempdir()?;
    let root = staging.path().join("drop");
    fs::create_dir_all(&root)?;
    copy_tree(drop, &root)?;

    let (leaves, failures) = unpack_recursive(&root, opts.dat, opts.password.as_deref());

    let mut shelf = Shelf::new(&opts.library, opts.dat, opts.db);
    let mut entries = Vec::new();
    for failure in failures {
        entries.push(ReportEntry {
            input: failure.path.display().to_string(),
            outcome: Outcome::Quarantined {
                reason: failure.reason,
            },
        });
    }
    entries.extend(shelf.shelve_all(&leaves));

    let report = Report {
        started,
        finished: now(),
        entries,
    };
    if let Some(db) = opts.db {
        if let Ok(json) = serde_json::to_string(&report) {
            db.add_report(&json).ok();
        }
    }
    Ok(report)
}

/// Copy a file or directory tree into `dest`, preserving structure.
fn copy_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_words_serialize_lowercase() {
        let entry = ReportEntry {
            input: "/drop/sonic.md".to_string(),
            outcome: Outcome::Added {
                game: "Sonic".to_string(),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"word\":\"added\""), "{json}");
        let back: ReportEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, entry.outcome);
    }

    #[test]
    fn old_capitalized_records_still_deserialize() {
        let old = r#"{
            "started": 1,
            "finished": 2,
            "entries": [
                {"input": "/drop/sonic.md",
                 "outcome": {"word": "Added", "detail": {"game": "Sonic"}}},
                {"input": "/drop/junk.xyz",
                 "outcome": {"word": "Unsupported",
                             "detail": {"reason": "unrecognised format"}}}
            ]
        }"#;
        let report: Report = serde_json::from_str(old).unwrap();
        assert_eq!(
            report.entries[0].outcome,
            Outcome::Added {
                game: "Sonic".to_string()
            }
        );
        assert_eq!(report.entries[1].outcome.word(), "unsupported");
        let rewritten = serde_json::to_string(&report).unwrap();
        assert!(rewritten.contains("\"word\":\"added\""), "{rewritten}");
    }
}
