//! Identify a leaf file: extension, then magic bytes.

use crate::unpack::{is_disc_ext, is_rider_ext, is_save_ext};
use crate::util::ext_of;
use den_ident::magic::{self, Kind};
use den_ident::System;
use std::path::Path;

/// What kind of thing a leaf file is, before it is shelved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A cartridge ROM.
    Rom,
    /// A disc image.
    Disc,
    /// A BIOS file, recognised by name or hash.
    Bios,
    /// A DOS/Windows executable.
    Executable,
    /// A rider file (manual, art, readme, save).
    Rider,
    /// Someone's save file left in the pile.
    Save,
    /// An archive that could not be unpacked.
    Archive,
    /// A format Den does not support.
    Alien,
}

/// The result of identification: a kind and a best-guess system.
#[derive(Debug, Clone, Copy)]
pub struct Identification {
    pub kind: FileKind,
    pub system: Option<System>,
    pub probable: bool,
}

/// Whether a system's games are disc images rather than cartridges.
pub fn is_disc_system(sys: System) -> bool {
    matches!(
        sys,
        System::SegaCd | System::Ps1 | System::Ps2 | System::Gamecube | System::Wii
    )
}

/// Identify a file by extension, then magic bytes.
pub fn identify(path: &Path) -> std::io::Result<Identification> {
    let ext = ext_of(path);
    let sniffed = magic::sniff(path).unwrap_or(Kind::Unknown);
    Ok(match sniffed {
        // A zip that survived unpacking is an arcade set, kept whole.
        Kind::Archive(_) if ext == "zip" => Identification {
            kind: FileKind::Rom,
            system: Some(System::Arcade),
            probable: true,
        },
        Kind::Archive(_) => Identification {
            kind: FileKind::Archive,
            system: None,
            probable: false,
        },
        Kind::Rom(sys) => Identification {
            kind: FileKind::Rom,
            system: Some(sys),
            probable: false,
        },
        Kind::Iso9660 => {
            let sys = System::from_extension(&ext)
                .filter(|s| is_disc_system(*s))
                .unwrap_or(System::Ps1);
            Identification {
                kind: FileKind::Disc,
                system: Some(sys),
                probable: true,
            }
        }
        Kind::Mz => Identification {
            kind: FileKind::Executable,
            system: Some(System::Dos),
            probable: true,
        },
        Kind::Unknown => identify_by_extension(path, &ext),
    })
}

fn identify_by_extension(path: &Path, ext: &str) -> Identification {
    if ext == "cue" {
        return Identification {
            kind: FileKind::Rider,
            system: None,
            probable: false,
        };
    }
    if is_save_ext(ext) {
        return Identification {
            kind: FileKind::Save,
            system: None,
            probable: false,
        };
    }
    if matches!(ext, "exe" | "com" | "bat" | "conf") {
        return Identification {
            kind: FileKind::Executable,
            system: Some(System::Dos),
            probable: true,
        };
    }
    if let Some(sys) = System::from_extension(ext) {
        let kind = if is_disc_ext(ext) {
            FileKind::Disc
        } else {
            FileKind::Rom
        };
        return Identification {
            kind,
            system: Some(sys),
            probable: true,
        };
    }
    if is_rider_ext(ext) {
        return Identification {
            kind: FileKind::Rider,
            system: None,
            probable: false,
        };
    }
    if crate::bios::BiosIndex::bundled().matches_name(path).is_some() {
        return Identification {
            kind: FileKind::Bios,
            system: None,
            probable: true,
        };
    }
    Identification {
        kind: FileKind::Alien,
        system: None,
        probable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nes_header_is_rom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.nes");
        std::fs::write(&path, b"NES\x1a\x02\x01\x01\x00rest").unwrap();
        let id = identify(&path).unwrap();
        assert_eq!(id.kind, FileKind::Rom);
        assert_eq!(id.system, Some(System::Nes));
    }

    #[test]
    fn unknown_text_is_rider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manual.txt");
        std::fs::write(&path, "read me").unwrap();
        let id = identify(&path).unwrap();
        assert_eq!(id.kind, FileKind::Rider);
    }

    #[test]
    fn extension_only_snes_is_probable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Mario.sfc");
        std::fs::write(&path, [0u8; 64]).unwrap();
        let id = identify(&path).unwrap();
        assert_eq!(id.kind, FileKind::Rom);
        assert_eq!(id.system, Some(System::Snes));
        assert!(id.probable);
    }

    #[test]
    fn save_file_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.srm");
        std::fs::write(&path, [0u8; 8]).unwrap();
        let id = identify(&path).unwrap();
        assert_eq!(id.kind, FileKind::Save);
    }
}
