//! Identification for Den: hashing, magic-byte sniffing, and the DAT index.
//!
//! This crate is deliberately pure: it reads files and returns answers, and
//! nothing else. No platform code, no state, so it builds headless and can
//! be fuzzed against the corpus.

/// The systems Den shelves games under, with the core the runner will pick
/// by default. Names are the words the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum System {
    /// Nintendo Entertainment System / Famicom.
    Nes,
    /// Super Nintendo Entertainment System / Super Famicom.
    Snes,
    /// Sega Genesis / Mega Drive.
    Genesis,
    /// Sega CD / Mega CD.
    SegaCd,
    /// Sega 32X.
    Sega32x,
    /// Nintendo 64.
    N64,
    /// Sony PlayStation.
    Ps1,
    /// Game Boy.
    Gb,
    /// Game Boy Color.
    Gbc,
    /// Game Boy Advance.
    Gba,
    /// Arcade sets (FinalBurn Neo and friends), kept zipped.
    Arcade,
    /// MS-DOS, run through DOSBox.
    Dos,
    /// Sony PlayStation 2. Needs an external emulator profile.
    Ps2,
    /// Nintendo GameCube. Needs an external emulator profile.
    Gamecube,
    /// Nintendo Wii. Needs an external emulator profile.
    Wii,
}

impl System {
    /// The human word for this system.
    pub fn name(self) -> &'static str {
        match self {
            System::Nes => "NES",
            System::Snes => "SNES",
            System::Genesis => "Genesis",
            System::SegaCd => "Sega CD",
            System::Sega32x => "Sega 32X",
            System::N64 => "N64",
            System::Ps1 => "PlayStation",
            System::Gb => "GB",
            System::Gbc => "GBC",
            System::Gba => "GBA",
            System::Arcade => "Arcade",
            System::Dos => "DOS",
            System::Ps2 => "PlayStation 2",
            System::Gamecube => "GameCube",
            System::Wii => "Wii",
        }
    }

    /// File extensions that most commonly carry this system's games.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            System::Nes => &["nes", "fds"],
            System::Snes => &["sfc", "smc", "fig"],
            System::Genesis => &["md", "gen", "smd"],
            System::SegaCd => &["bin", "cue", "chd"],
            System::Sega32x => &["32x", "bin", "md"],
            System::N64 => &["z64", "n64", "v64"],
            System::Ps1 => &["bin", "cue", "chd", "iso", "pbp"],
            System::Gb => &["gb"],
            System::Gbc => &["gbc"],
            System::Gba => &["gba"],
            System::Arcade => &["zip"],
            System::Dos => &["exe", "com", "bat", "conf"],
            System::Ps2 => &["iso", "chd", "bin", "cue"],
            System::Gamecube => &["iso", "gcm", "rvz", "nkit"],
            System::Wii => &["iso", "wbfs", "rvz", "nkit"],
        }
    }

    /// Resolve an extension (without the dot, lower-cased) to a system.
    pub fn from_extension(ext: &str) -> Option<System> {
        let ext = ext.to_ascii_lowercase();
        [
            System::Nes,
            System::Snes,
            System::Genesis,
            System::SegaCd,
            System::Sega32x,
            System::N64,
            System::Ps1,
            System::Gb,
            System::Gbc,
            System::Gba,
            System::Arcade,
            System::Dos,
            System::Ps2,
            System::Gamecube,
            System::Wii,
        ]
        .into_iter()
        .find(|system| system.extensions().contains(&ext.as_str()))
    }

    /// The libretro core Den prefers for this system: the accuracy-per-watt
    /// picks from the build plan so one library behaves on a Pi and a desktop.
    pub fn default_core(self) -> &'static str {
        match self {
            System::Nes => "mesen",
            System::Snes => "snes9x",
            System::Genesis | System::SegaCd => "genesis_plus_gx",
            System::Sega32x => "picodrive",
            System::N64 => "mupen64plus_next",
            System::Ps1 => "swanstation",
            System::Gb | System::Gbc => "gambatte",
            System::Gba => "mgba",
            System::Arcade => "fbneo",
            System::Dos => "dosbox_pure",
            System::Ps2 => "pcsx2",
            System::Gamecube | System::Wii => "dolphin",
        }
    }
}

/// Hashing: SHA-1 for the DAT index, CRC32 for entry-level corruption checks.
pub mod hash {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    use std::path::Path;

    /// SHA-1 of a byte slice, as lowercase hex.
    pub fn sha1_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha1::digest(bytes))
    }

    /// SHA-1 of a file, as lowercase hex.
    pub fn sha1_file(path: &Path) -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha1::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// CRC32 of a byte slice.
    pub fn crc32(bytes: &[u8]) -> u32 {
        crc32fast::hash(bytes)
    }

    /// CRC32 of a file.
    pub fn crc32_file(path: &Path) -> std::io::Result<u32> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = crc32fast::Hasher::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hasher.finalize())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sha1_of_empty_string_is_known() {
            assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        }

        #[test]
        fn crc32_of_known_bytes() {
            assert_eq!(crc32(b"123456789"), 0xcbf43926);
        }
    }
}

/// Magic bytes: what a file *is* before we trust its name.
pub mod magic {
    use super::System;
    use std::fs;
    use std::path::Path;

    /// What the first bytes of a file say it is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        /// A container we can unpack: zip, 7z, rar, gzip, tar.
        Archive(Archive),
        /// A ROM with a recognisable header.
        Rom(System),
        /// An ISO9660 disc image (PS1, PS2, GameCube...).
        Iso9660,
        /// A DOS/Windows executable (MZ header).
        Mz,
        /// Nothing we can name.
        Unknown,
    }

    /// The archive formats Den unpacks.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Archive {
        /// A PKZIP container.
        Zip,
        /// A 7-Zip container.
        SevenZ,
        /// A RAR container (RAR4 or RAR5).
        Rar,
        /// A gzip stream, usually wrapping a tar.
        Gzip,
        /// A POSIX tar container.
        Tar,
    }

    /// The ISO9660 primary volume descriptor sits at 0x8000 on a 2048-byte
    /// sector disc, so a head that stops short of it can never see one. Every
    /// other signature lives in the first few bytes; this length is set by the
    /// furthest one.
    const ISO_MARKER: usize = 0x8001;
    const HEAD: usize = ISO_MARKER + 5;

    /// Sniff the first bytes of a file.
    pub fn sniff(path: &Path) -> std::io::Result<Kind> {
        let mut file = fs::File::open(path)?;
        let mut head = vec![0u8; HEAD];
        // One `read` returns what one syscall gave us, which for a large file
        // is usually less than HEAD; read to the end of the buffer or the end
        // of the file, whichever comes first.
        let mut filled = 0;
        loop {
            match std::io::Read::read(&mut file, &mut head[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
            if filled == head.len() {
                break;
            }
        }
        head.truncate(filled);
        Ok(kinds(&head))
    }

    /// Classify a byte slice (the file's head) without touching the disk.
    pub fn kinds(head: &[u8]) -> Kind {
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            return Kind::Archive(Archive::Zip);
        }
        if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
            return Kind::Archive(Archive::SevenZ);
        }
        if head.starts_with(b"Rar!\x1a\x07") {
            return Kind::Archive(Archive::Rar);
        }
        if head.starts_with(&[0x1f, 0x8b]) {
            return Kind::Archive(Archive::Gzip);
        }
        if head.len() >= 262 && &head[257..262] == b"ustar" {
            return Kind::Archive(Archive::Tar);
        }
        if head.starts_with(b"NES\x1a") {
            return Kind::Rom(System::Nes);
        }
        // N64 cartridges: three byte orders, same bytes in a different order.
        if head.starts_with(&[0x80, 0x37, 0x12, 0x40])
            || head.starts_with(&[0x37, 0x80, 0x40, 0x12])
            || head.starts_with(&[0x40, 0x12, 0x37, 0x80])
        {
            return Kind::Rom(System::N64);
        }
        // ISO9660: the volume descriptor sits at 0x8001 on a 2048-sector disc.
        // The bound has to cover the whole marker, not its first byte: a head
        // that stops inside it used to slice past the end and panic.
        if head.len() >= ISO_MARKER + 5 && &head[ISO_MARKER..ISO_MARKER + 5] == b"CD001" {
            return Kind::Iso9660;
        }
        if head.starts_with(b"MZ") {
            return Kind::Mz;
        }
        Kind::Unknown
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn zip() {
            assert_eq!(kinds(b"PK\x03\x04rest"), Kind::Archive(Archive::Zip));
        }

        #[test]
        fn seven_z() {
            assert_eq!(kinds(b"7z\xbc\xaf\x27\x1c"), Kind::Archive(Archive::SevenZ));
        }

        #[test]
        fn rar4_and_rar5() {
            assert_eq!(kinds(b"Rar!\x1a\x07\x00rest"), Kind::Archive(Archive::Rar));
            assert_eq!(
                kinds(b"Rar!\x1a\x07\x01\x00rest"),
                Kind::Archive(Archive::Rar)
            );
        }

        #[test]
        fn gzip() {
            assert_eq!(
                kinds(&[0x1f, 0x8b, 0x08, 0x00]),
                Kind::Archive(Archive::Gzip)
            );
        }

        #[test]
        fn ines() {
            assert_eq!(kinds(b"NES\x1a\x02\x01\x01\x00"), Kind::Rom(System::Nes));
        }

        #[test]
        fn n64_three_byte_orders() {
            for head in [
                &[0x80, 0x37, 0x12, 0x40][..],
                &[0x37, 0x80, 0x40, 0x12][..],
                &[0x40, 0x12, 0x37, 0x80][..],
            ] {
                assert_eq!(kinds(head), Kind::Rom(System::N64));
            }
        }

        #[test]
        fn iso9660() {
            let mut head = vec![0u8; 0x8006];
            head[0x8001..0x8006].copy_from_slice(b"CD001");
            assert_eq!(kinds(&head), Kind::Iso9660);
        }

        #[test]
        fn head_that_stops_inside_the_iso_marker_does_not_panic() {
            // Anything from 0x8003 to 0x8005 used to slice past the end.
            for len in 0x8000..=0x8005 {
                let mut head = vec![0u8; len];
                if len > 0x8001 {
                    head[0x8001] = b'C';
                }
                assert_eq!(kinds(&head), Kind::Unknown, "len {len:#x}");
            }
        }

        #[test]
        fn sniff_reads_far_enough_to_see_a_disc() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("disc.img");
            let mut iso = vec![0u8; 0x9000];
            iso[0x8001..0x8006].copy_from_slice(b"CD001");
            std::fs::write(&path, &iso).unwrap();
            assert_eq!(sniff(&path).unwrap(), Kind::Iso9660);
        }

        #[test]
        fn sniff_of_a_short_file_is_still_classified() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("game.nes");
            std::fs::write(&path, b"NES\x1a\x02\x01").unwrap();
            assert_eq!(sniff(&path).unwrap(), Kind::Rom(System::Nes));
        }

        #[test]
        fn unknown() {
            assert_eq!(kinds(b"hello world"), Kind::Unknown);
        }
    }
}

/// The DAT index: SHA-1 hashes to exact game names, the No-Intro/Redump
/// posture. Den ships a tiny bundled sample and loads real databases as a
/// plain TSV (`sha1`, `title`, `system`), one entry per line, `#` comments.
pub mod dat {
    use super::hash;
    use super::System;
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
}
