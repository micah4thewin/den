use crate::hash;
use crate::System;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub sha1: String,
    pub title: String,
    pub system: String,
}

#[derive(Debug, Clone, Default)]
pub struct Index {
    entries: HashMap<String, Entry>,
}

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

    pub fn lookup(&self, sha1: &str) -> Option<&Entry> {
        self.entries.get(sha1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn lookup_file(&self, path: &Path) -> std::io::Result<Option<&Entry>> {
        let sha1 = hash::sha1_file(path)?;
        Ok(self.lookup(&sha1))
    }

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
