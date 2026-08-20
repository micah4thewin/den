//! Archive unpacking: zip, 7z, rar, tar, gz, nested to any depth.
//!
//! zip, 7z, tar, and gz are pure Rust. RAR shells out to the system `unrar`
//! binary (the freeware extraction-only tool already shipped on Debian), which
//! keeps Den free of a native build dependency. A corrupt archive is salvaged
//! entry-by-entry: what extracts cleanly continues, what does not is reported.
//!
//! Note: AES-encrypted 7z is reported as password-protected rather than
//! decrypted, because sevenz-rust's `aes256` feature is broken against its
//! current lzma-rust dependency. Standard (unencrypted) 7z decompresses fine.

use crate::util::{safe_join, stem_of};
use den_ident::dat::Index;
use den_ident::magic::{self, Archive, Kind};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One archive that could not be unpacked, with a human reason.
#[derive(Debug, Clone)]
pub struct UnpackFailure {
    pub path: PathBuf,
    pub reason: String,
}

/// An error surfaced by a single unpack step.
#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("archive is password protected")]
    PasswordProtected,
    #[error("{0}")]
    Other(String),
}

impl UnpackError {
    /// Build a plain error from anything displayable.
    pub fn other(e: impl std::fmt::Display) -> Self {
        UnpackError::Other(e.to_string())
    }
}

/// Recursively unpack every archive under `root`. Returns the leaf files (non
/// archives, plus arcade sets kept zipped) and the archives that failed.
pub fn unpack_recursive(
    root: &Path,
    dat: &Index,
    password: Option<&str>,
) -> (Vec<PathBuf>, Vec<UnpackFailure>) {
    let mut leaves = Vec::new();
    let mut failures = Vec::new();
    let mut queue = vec![root.to_path_buf()];

    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
                continue;
            }
            let kind = match magic::sniff(&path) {
                Ok(Kind::Archive(kind)) => kind,
                _ => {
                    leaves.push(path);
                    continue;
                }
            };
            // An arcade set is one zip per game; never unpack it.
            if kind == Archive::Zip && is_arcade_zip(&path, dat) {
                leaves.push(path);
                continue;
            }
            let dest = fresh_sibling_dir(&path);
            match unpack_one(&path, &dest, kind, password) {
                Ok(()) => queue.push(dest),
                Err(e) => failures.push(UnpackFailure {
                    path,
                    reason: e.to_string(),
                }),
            }
        }
    }

    leaves.sort();
    (leaves, failures)
}

/// Whether a zip is an arcade set (a game in its own right, kept zipped).
/// True when its name matches an arcade entry in the DAT index.
fn is_arcade_zip(path: &Path, dat: &Index) -> bool {
    let stem_norm = normalize(&stem_of(path));
    dat.entries()
        .filter(|e| e.system.eq_ignore_ascii_case("arcade"))
        .any(|e| normalize(&e.title) == stem_norm)
}

fn normalize(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// A sibling directory for an archive's contents, made unique.
fn fresh_sibling_dir(path: &Path) -> PathBuf {
    let base = path.with_extension("");
    let mut candidate = base.clone();
    let mut i = 2u32;
    while candidate.exists() {
        candidate = PathBuf::from(format!("{}.{}", base.display(), i));
        i += 1;
    }
    candidate
}

/// Unpack a single archive into `dest`, which is created if needed.
fn unpack_one(
    src: &Path,
    dest: &Path,
    kind: Archive,
    password: Option<&str>,
) -> Result<(), UnpackError> {
    fs::create_dir_all(dest).map_err(UnpackError::other)?;
    match kind {
        Archive::Zip => unpack_zip(src, dest, password),
        Archive::SevenZ => unpack_7z(src, dest),
        Archive::Rar => unpack_rar(src, dest, password),
        Archive::Gzip => unpack_gzip(src, dest),
        Archive::Tar => unpack_tar(src, dest),
    }
}

fn unpack_zip(src: &Path, dest: &Path, password: Option<&str>) -> Result<(), UnpackError> {
    let file = File::open(src).map_err(UnpackError::other)?;
    let mut archive = zip::ZipArchive::new(file).map_err(UnpackError::other)?;
    let mut extracted = 0usize;
    let mut first_err: Option<String> = None;

    for i in 0..archive.len() {
        let raw_name = archive.name_for_index(i).unwrap_or("").to_string();
        let entry = match password {
            Some(pw) => archive.by_index_decrypt(i, pw.as_bytes()),
            None => archive.by_index(i),
        };
        let mut entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e.to_string());
                }
                continue;
            }
        };
        let Some(out_path) = safe_join(dest, &raw_name) else {
            continue;
        };
        if entry.is_dir() {
            let _ = fs::create_dir_all(&out_path);
            continue;
        }
        if let Some(parent) = out_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut out = match File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e.to_string());
                }
                continue;
            }
        };
        match std::io::copy(&mut entry, &mut out) {
            Ok(_) => extracted += 1,
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e.to_string());
                }
            }
        }
    }

    if extracted == 0 {
        if let Some(e) = first_err {
            if looks_like_password(&e) {
                return Err(UnpackError::PasswordProtected);
            }
            return Err(UnpackError::Other(e));
        }
    }
    Ok(())
}

fn unpack_7z(src: &Path, dest: &Path) -> Result<(), UnpackError> {
    match sevenz_rust::decompress_file(src, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            let lower = msg.to_ascii_lowercase();
            if looks_like_password(&msg) || lower.contains("aes") || lower.contains("encrypted") {
                Err(UnpackError::PasswordProtected)
            } else {
                Err(UnpackError::Other(msg))
            }
        }
    }
}

fn unpack_rar(src: &Path, dest: &Path, password: Option<&str>) -> Result<(), UnpackError> {
    let mut cmd = Command::new("unrar");
    cmd.arg("x").arg("-o+").arg("-y");
    match password {
        Some(pw) => {
            cmd.arg(format!("-p{pw}"));
        }
        None => {
            cmd.arg("-p-"); // empty password, never prompt
        }
    }
    cmd.arg(src).arg(dest);
    let output = cmd.output().map_err(UnpackError::other)?;
    if output.status.success() {
        return Ok(());
    }
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if looks_like_password(&all) {
        Err(UnpackError::PasswordProtected)
    } else {
        Err(UnpackError::Other(all.trim().to_string()))
    }
}

fn unpack_tar(src: &Path, dest: &Path) -> Result<(), UnpackError> {
    let file = File::open(src).map_err(UnpackError::other)?;
    let mut archive = tar::Archive::new(file);
    archive.unpack(dest).map_err(UnpackError::other)?;
    Ok(())
}

fn unpack_gzip(src: &Path, dest: &Path) -> Result<(), UnpackError> {
    let file = File::open(src).map_err(UnpackError::other)?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut name = stem_of(src);
    if name.is_empty() || name == "unnamed" {
        name = "file".to_string();
    }
    let out_path = dest.join(&name);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(UnpackError::other)?;
    }
    let mut out = File::create(&out_path).map_err(UnpackError::other)?;
    std::io::copy(&mut decoder, &mut out).map_err(UnpackError::other)?;
    Ok(())
}

fn looks_like_password(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("encrypted")
        || lower.contains("invalidpassword")
}

/// Decide whether an extension denotes a disc image (as opposed to a cart).
pub fn is_disc_ext(ext: &str) -> bool {
    matches!(
        ext,
        "bin" | "cue" | "chd" | "iso" | "pbp" | "gcm" | "wbfs" | "rvz" | "nkit" | "img" | "mdf"
    )
}

/// Whether an extension is a rider file (manual, art, readme, save).
pub fn is_rider_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "pdf" | "doc" | "docx" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
            | "nfo" | "md" | "html" | "htm" | "rtf" | "epub" | "srt" | "sub" | "chm"
    )
}

/// Whether an extension is a save file someone left in the pile.
pub fn is_save_ext(ext: &str) -> bool {
    ext == "srm" || ext == "dsv" || ext == "state" || ext.starts_with("state")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ext_of;
    use std::io::Write;

    #[test]
    fn zip_roundtrip_extracts() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        let out = dir.path().join("out");
        {
            let f = File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("games/sonic.md", opts).unwrap();
            zw.write_all(b"fake genesis rom").unwrap();
            zw.start_file("manual.txt", opts).unwrap();
            zw.write_all(b"read me").unwrap();
            zw.finish().unwrap();
        }
        unpack_zip(&zip_path, &out, None).unwrap();
        assert!(out.join("games/sonic.md").exists());
        assert!(out.join("manual.txt").exists());
    }

    #[test]
    fn zip_traversal_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        let out = dir.path().join("out");
        {
            let f = File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("../evil.txt", opts).unwrap();
            zw.write_all(b"boo").unwrap();
            zw.finish().unwrap();
        }
        unpack_zip(&zip_path, &out, None).unwrap();
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn gzip_roundtrip_extracts() {
        let dir = tempfile::tempdir().unwrap();
        let gz_path = dir.path().join("note.txt.gz");
        let out = dir.path().join("out");
        {
            let f = File::create(&gz_path).unwrap();
            let mut enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            enc.write_all(b"hello").unwrap();
            enc.finish().unwrap();
        }
        unpack_gzip(&gz_path, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("note.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn ext_classifiers() {
        assert!(is_disc_ext("bin"));
        assert!(!is_disc_ext("nes"));
        assert!(is_rider_ext("txt"));
        assert!(is_save_ext("srm"));
        assert_eq!(ext_of(Path::new("a.BIN")), "bin");
    }
}
