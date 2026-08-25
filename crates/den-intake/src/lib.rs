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
    pub db: Option<&'a den_db::Db>,
    pub password: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

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
