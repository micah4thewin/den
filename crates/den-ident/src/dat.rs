//! The DAT index: SHA-1 hashes to exact game names, the No-Intro/Redump
//! posture. Den ships a tiny bundled sample and loads real databases as a
//! plain TSV (`sha1`, `title`, `system`), one entry per line, `#` comments.

use crate::hash;
use crate::System;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// One hash-to-name mapping from a database.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    /// The SHA-1 of the file this entry names, as lowercase hex.
    pub sha1: String,
    /// The exact title the database gives it.
    pub title: String,
    /// The system label as the database spells it.
    pub system: String,
}

/// An immutable hash lookup table.
#[derive(Debug, Clone, Default)]
pub struct Index {
    entries: HashMap<String, Entry>,
}

/// The bundled sample: a few entries that match the corpus fixtures, so
/// the hash path of intake is exercised without any download.
pub fn bundled() -> Index {
    let mut index = Index::default();
    for (sha1, title, system) in [
        (
            "0000000000000000000000000000000000000000",
            "Corpus Fixture (USA)",
            "NES",
        ),
        (
            "1111111111111111111111111111111111111111",
            "Corpus Fixture II (Europe)",
            "Genesis",
        ),
        (
            "2222222222222222222222222222222222222222",
            "Corpus Fixture III (Japan)",
            "GBA",
        ),
    ] {
        index.entries.insert(
            sha1.to_string(),
            Entry {
                sha1: sha1.to_string(),
                title: title.to_string(),
                system: system.to_string(),
            },
        );
    }
    index
}

impl Index {
    /// Load a TSV database: `sha1<TAB>title<TAB>system`, `#` comments.
    pub fn load_tsv(path: &Path) -> std::io::Result<Index> {
        let text = fs::read_to_string(path)?;
        let mut index = Index::default();
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let sha1 = parts.next().unwrap_or("").trim();
            let title = parts.next().unwrap_or("").trim();
            let system = parts.next().unwrap_or("").trim();
            if sha1.len() == 40 && !title.is_empty() {
                index.entries.insert(
                    sha1.to_string(),
                    Entry {
                        sha1: sha1.to_string(),
                        title: title.to_string(),
                        system: system.to_string(),
                    },
                );
            } else {
                log::warn!("dat: skipping malformed line {}", line_no + 1);
            }
        }
        Ok(index)
    }

    /// Look a SHA-1 up; returns the exact name if the database has it.
    pub fn lookup(&self, sha1: &str) -> Option<&Entry> {
        self.entries.get(sha1)
    }

    /// How many entries are loaded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over every entry (for arcade-set detection and reporting).
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// Look a file up by its hash, returning the entry if found.
    pub fn lookup_file(&self, path: &Path) -> std::io::Result<Option<&Entry>> {
        let sha1 = hash::sha1_file(path)?;
        Ok(self.lookup(&sha1))
    }

    /// Resolve a system name from the database to our enum. Exact match
    /// on the simple labels Den writes, plus the common aliases.
    pub fn system_of(entry: &Entry) -> Option<System> {
        let name = entry.system.trim().to_ascii_lowercase();
        Some(match name.as_str() {
            "nes" | "nintendo entertainment system" => System::Nes,
            "snes" | "super nintendo" | "super nintendo entertainment system" => System::Snes,
            "genesis" | "mega drive" | "sega genesis" => System::Genesis,
            "sega cd" | "mega cd" => System::SegaCd,
            "32x" | "sega 32x" => System::Sega32x,
            "n64" | "nintendo 64" => System::N64,
            "playstation" | "psx" | "ps1" => System::Ps1,
            "gb" | "game boy" => System::Gb,
            "gbc" | "game boy color" => System::Gbc,
            "gba" | "game boy advance" => System::Gba,
            "arcade" | "fbneo" => System::Arcade,
            "dos" | "ms-dos" => System::Dos,
            "ps2" | "playstation 2" => System::Ps2,
            "gamecube" | "game cube" | "ngc" => System::Gamecube,
            "wii" => System::Wii,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_lookup() {
        let index = bundled();
        assert_eq!(index.len(), 3);
        let entry = index
            .lookup("0000000000000000000000000000000000000000")
            .unwrap();
        assert_eq!(entry.title, "Corpus Fixture (USA)");
        assert_eq!(System::from_extension("nes"), Some(System::Nes));
    }

    #[test]
    fn tsv_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.tsv");
        std::fs::write(
            &path,
            "# comment\n\
             aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tSonic the Hedgehog (USA, Europe)\tGenesis\n\
             bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\tTwisted Metal (USA)\tPlayStation\n",
        )
        .unwrap();
        let index = Index::load_tsv(&path).unwrap();
        assert_eq!(index.len(), 2);
        let entry = index
            .lookup("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap();
        assert_eq!(entry.title, "Sonic the Hedgehog (USA, Europe)");
        assert_eq!(Index::system_of(entry), Some(System::Genesis));
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.tsv");
        std::fs::write(&path, "not-a-hash\tTitle\tNES\n").unwrap();
        let index = Index::load_tsv(&path).unwrap();
        assert_eq!(index.len(), 0);
    }
}
