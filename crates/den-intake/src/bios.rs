//! BIOS recognition: by hash (a user-supplied TSV) or by well-known filename.

use std::collections::HashMap;
use std::path::Path;

/// A lookup of BIOS files, name- and hash-keyed. Real hash databases load as
/// `sha1<TAB>name` TSVs; a small name list ships so common BIOSes file
/// themselves even without a hash database present.
#[derive(Debug, Clone, Default)]
pub struct BiosIndex {
    by_hash: HashMap<String, String>,
    by_name: HashMap<String, String>,
}

impl BiosIndex {
    /// The bundled name list (common BIOS filenames to canonical names).
    pub fn bundled() -> Self {
        let names = [
            ("scph1000.bin", "PlayStation SCPH-1000 BIOS"),
            ("scph1001.bin", "PlayStation SCPH-1001 BIOS"),
            ("scph5000.bin", "PlayStation SCPH-5000 BIOS"),
            ("scph5500.bin", "PlayStation SCPH-5500 BIOS"),
            ("scph5501.bin", "PlayStation SCPH-5501 BIOS"),
            ("scph5502.bin", "PlayStation SCPH-5502 BIOS"),
            ("scph7001.bin", "PlayStation SCPH-7001 BIOS"),
            ("scph7502.bin", "PlayStation SCPH-7502 BIOS"),
            ("psxonpsp660.bin", "PlayStation PSP 6.60 BIOS"),
            ("gba_bios.bin", "Game Boy Advance BIOS"),
            ("bios7.bin", "Nintendo DS ARM7 BIOS"),
            ("bios9.bin", "Nintendo DS ARM9 BIOS"),
            ("dc_boot.bin", "Dreamcast boot ROM"),
            ("neogeo.zip", "Neo Geo BIOS"),
        ];
        let mut by_name = HashMap::new();
        for (name, canonical) in names {
            by_name.insert(name.to_ascii_lowercase(), canonical.to_string());
        }
        BiosIndex {
            by_hash: HashMap::new(),
            by_name,
        }
    }

    /// Load a hash TSV (`sha1<TAB>name`) on top of the bundled name list.
    pub fn load_tsv(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut index = BiosIndex::bundled();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let sha1 = parts.next().unwrap_or("").trim();
            let name = parts.next().unwrap_or("").trim();
            if sha1.len() == 40 && !name.is_empty() {
                index
                    .by_hash
                    .insert(sha1.to_ascii_lowercase(), name.to_string());
            }
        }
        Ok(index)
    }

    /// The canonical name for a SHA-1, if the database knows it.
    pub fn by_hash(&self, sha1: &str) -> Option<&str> {
        self.by_hash
            .get(&sha1.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    /// The canonical name for a filename, if it is a known BIOS name.
    pub fn matches_name(&self, path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        self.by_name.get(&name).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_name_matches() {
        let index = BiosIndex::bundled();
        assert_eq!(
            index.matches_name(Path::new("/x/scph1001.bin")),
            Some("PlayStation SCPH-1001 BIOS")
        );
        assert_eq!(index.matches_name(Path::new("/x/random.bin")), None);
    }

    #[test]
    fn hash_tsv_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bios.tsv");
        std::fs::write(
            &path,
            "0123456789abcdef0123456789abcdef01234567\tCustom BIOS\n",
        )
        .unwrap();
        let index = BiosIndex::load_tsv(&path).unwrap();
        assert_eq!(
            index.by_hash("0123456789ABCDEF0123456789ABCDEF01234567"),
            Some("Custom BIOS")
        );
    }
}
