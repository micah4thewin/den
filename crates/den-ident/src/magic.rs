//! Magic bytes: what a file *is* before we trust its name.

use crate::System;
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
