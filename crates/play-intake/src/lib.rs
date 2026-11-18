pub mod bios;
mod identify;
mod shelve;
mod unpack;
mod util;

pub use bios::BiosIndex;
pub use identify::{identify, FileKind, Identification};
pub use shelve::Shelf;
pub use unpack::{unpack_recursive, UnpackError, UnpackFailure, Unpacked};

use play_db::now;
use play_ident::dat::Index;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "word", content = "detail", rename_all = "lowercase")]
pub enum Outcome {
    #[serde(alias = "Added")]
    Added { game: String },
    #[serde(alias = "Duplicate")]
    Duplicate { game: String },
    #[serde(alias = "Repaired")]
    Repaired { game: String, note: String },
    #[serde(alias = "Probable")]
    Probable { game: String },
    #[serde(alias = "Bios")]
    Bios { name: String },
    #[serde(alias = "Extra")]
    Extra { note: String },
    #[serde(alias = "Quarantined")]
    Quarantined { reason: String },
    #[serde(alias = "Unsupported")]
    Unsupported { reason: String },
}

impl Outcome {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEntry {
    pub input: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub started: i64,
    pub finished: i64,
    pub entries: Vec<ReportEntry>,
}

impl Report {
    pub fn tally(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let mut map = std::collections::BTreeMap::new();
        for e in &self.entries {
            *map.entry(e.outcome.word()).or_insert(0) += 1;
        }
        map
    }
}

pub struct IntakeOptions<'a> {
    pub library: PathBuf,
    pub dat: &'a Index,
    pub db: Option<&'a play_db::Db>,
    pub password: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn run_intake(drop: &Path, opts: &IntakeOptions) -> Result<Report, IntakeError> {
    run_intake_many(&[drop.to_path_buf()], opts)
}

/// Take in everything that was dropped at once: files, folders and archives,
/// in any mix. Nothing in the drop is written to; what archives hold is
/// unpacked into a staging folder inside the library, which goes away after.
pub fn run_intake_many(drops: &[PathBuf], opts: &IntakeOptions) -> Result<Report, IntakeError> {
    let started = now();
    // Staging lives beside the library, not in the system temp folder: a
    // disc image can be bigger than a RAM-backed /tmp, and the copy onto the
    // shelf is then a move within one disk.
    let staging_root = opts.library.join("_staging");
    fs::create_dir_all(&staging_root)?;
    let staging = tempfile::Builder::new()
        .prefix("intake-")
        .tempdir_in(&staging_root)?;

    let unpacked = unpack_recursive(drops, staging.path(), opts.dat, opts.password.as_deref());

    let mut shelf = Shelf::new(&opts.library, opts.dat, opts.db);
    let mut entries = Vec::new();
    for failure in unpacked.unreadable.iter().chain(unpacked.failures.iter()) {
        entries.push(ReportEntry {
            input: failure.path.display().to_string(),
            outcome: Outcome::Quarantined {
                reason: failure.reason.clone(),
            },
        });
    }
    for (volume, note) in &unpacked.volumes {
        entries.push(ReportEntry {
            input: volume.display().to_string(),
            outcome: Outcome::Extra {
                note: format!("archive volume, {note}"),
            },
        });
    }
    entries.extend(shelf.shelve_all(&unpacked.leaves));

    let shown = |raw: &str| -> String {
        let real = unpacked.label(Path::new(raw));
        for drop in drops {
            let base = drop.parent().unwrap_or(drop);
            if let Ok(rest) = real.strip_prefix(base) {
                if !rest.as_os_str().is_empty() {
                    return slashed(rest);
                }
            }
        }
        real.display().to_string()
    };
    for entry in entries.iter_mut() {
        entry.input = shown(&entry.input);
    }

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
    drop(staging);
    let _ = fs::remove_dir(&staging_root);
    Ok(report)
}

/// A path inside a drop, written the same way on every system.
fn slashed(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
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
