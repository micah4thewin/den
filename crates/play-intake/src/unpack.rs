use crate::util::{ext_of, safe_join, stem_of};
use play_ident::dat::Index;
use play_ident::magic::{self, Archive, Kind};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// How deep archives may nest inside one another before Play stops opening
/// them. Real downloads go two or three deep; a hundred is a zip bomb.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub struct UnpackFailure {
    pub path: PathBuf,
    pub reason: String,
}

/// What a drop turned into once every archive in it was opened.
#[derive(Debug, Default)]
pub struct Unpacked {
    /// Files to shelve, wherever they now are: in the drop itself, or in
    /// staging if an archive held them.
    pub leaves: Vec<PathBuf>,
    /// Archives that could not be opened, and why.
    pub failures: Vec<UnpackFailure>,
    /// Second and later volumes of a multi-part archive: read along with the
    /// first, so they need no shelf of their own.
    pub volumes: Vec<(PathBuf, String)>,
    /// Folders that could not be read.
    pub unreadable: Vec<UnpackFailure>,
    /// Where each unpacked folder came from, for naming files in the report.
    origins: Vec<(PathBuf, PathBuf)>,
}

impl Unpacked {
    /// The path a person would recognise for `path`: an unpacked file is
    /// named as the archive it came out of, then its place inside.
    pub fn label(&self, path: &Path) -> PathBuf {
        let mut best: Option<(&PathBuf, &PathBuf)> = None;
        for (dir, archive) in &self.origins {
            if path.starts_with(dir)
                && best.is_none_or(|(b, _)| dir.components().count() > b.components().count())
            {
                best = Some((dir, archive));
            }
        }
        match best {
            Some((dir, archive)) => {
                let rest = path.strip_prefix(dir).unwrap_or(path);
                self.label(archive).join(rest)
            }
            None => path.to_path_buf(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("archive is password protected")]
    PasswordProtected,
    #[error("{0}")]
    Other(String),
}

impl UnpackError {
    pub fn other(e: impl std::fmt::Display) -> Self {
        UnpackError::Other(e.to_string())
    }
}

/// Open every archive under `roots`, and every archive inside those, putting
/// what comes out in `staging`. Nothing under `roots` is written to.
pub fn unpack_recursive(
    roots: &[PathBuf],
    staging: &Path,
    dat: &Index,
    password: Option<&str>,
) -> Unpacked {
    let mut out = Unpacked::default();
    let mut queue: Vec<(PathBuf, usize)> = Vec::new();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    let mut next_dir = 0usize;
    let mut fresh = |stem: &str| -> PathBuf {
        next_dir += 1;
        staging.join(format!("{next_dir:04}-{}", short(stem)))
    };

    let mut loose: Vec<PathBuf> = Vec::new();
    for root in roots {
        match fs::metadata(root) {
            Ok(m) if m.is_dir() => queue.push((root.clone(), 0)),
            Ok(_) => loose.push(root.clone()),
            Err(e) => out.unreadable.push(UnpackFailure {
                path: root.clone(),
                reason: format!("could not be read: {e}"),
            }),
        }
    }
    if !loose.is_empty() {
        process_files(loose, 0, dat, password, &mut fresh, &mut queue, &mut out);
    }

    while let Some((dir, depth)) = queue.pop() {
        let canonical = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen_dirs.insert(canonical) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                out.unreadable.push(UnpackFailure {
                    path: dir.clone(),
                    reason: format!("folder could not be read: {e}"),
                });
                continue;
            }
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if is_junk(&path) {
                continue;
            }
            match fs::metadata(&path) {
                Ok(m) if m.is_dir() => queue.push((path, depth)),
                Ok(m) if m.is_file() => files.push(path),
                Ok(_) => {}
                Err(e) => out.unreadable.push(UnpackFailure {
                    path,
                    reason: format!("could not be read: {e}"),
                }),
            }
        }
        process_files(
            files, depth, dat, password, &mut fresh, &mut queue, &mut out,
        );
    }

    out.leaves.sort();
    out.leaves.dedup();
    out
}

fn process_files(
    mut files: Vec<PathBuf>,
    depth: usize,
    dat: &Index,
    password: Option<&str>,
    fresh: &mut impl FnMut(&str) -> PathBuf,
    queue: &mut Vec<(PathBuf, usize)>,
    out: &mut Unpacked,
) {
    files.sort();
    let present: HashSet<String> = files.iter().map(|p| lower_name(p)).collect();

    // Split files (`game.7z.001`, `game.iso.002`, ...) are joined back into
    // the one file they were cut from before anything looks at them.
    let mut splits: BTreeMap<(PathBuf, String), Vec<(u32, PathBuf)>> = BTreeMap::new();
    let mut whole: Vec<PathBuf> = Vec::new();
    for f in files {
        match split_part(&f) {
            Some((base, n)) => splits
                .entry((f.parent().map(Path::to_path_buf).unwrap_or_default(), base))
                .or_default()
                .push((n, f)),
            None => whole.push(f),
        }
    }
    for ((_dir, base), mut parts) in splits {
        parts.sort();
        let first = parts[0].1.clone();
        let numbers: Vec<u32> = parts.iter().map(|(n, _)| *n).collect();
        let contiguous = numbers.iter().enumerate().all(|(i, n)| *n == i as u32 + 1);
        if !contiguous {
            let missing = (1..=*numbers.last().unwrap_or(&1))
                .find(|n| !numbers.contains(n))
                .unwrap_or(1);
            for (_, part) in &parts {
                out.failures.push(UnpackFailure {
                    path: part.clone(),
                    reason: format!("part {missing:03} of this split file is missing"),
                });
            }
            continue;
        }
        let dir = fresh(&base);
        let joined = dir.join(&base);
        match join_parts(&parts, &joined) {
            Ok(()) => {
                out.origins.push((dir, first.with_file_name(&base)));
                for (_, part) in &parts[1..] {
                    out.volumes
                        .push((part.clone(), format!("joined into {base}")));
                }
                // The first part stands for the joined file in the report.
                whole.push(joined);
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                for (_, part) in &parts {
                    out.failures.push(UnpackFailure {
                        path: part.clone(),
                        reason: format!("split file could not be joined: {e}"),
                    });
                }
            }
        }
    }

    for path in whole {
        if let Some(first) = later_rar_volume(&path, &present) {
            out.volumes.push((path, format!("read with {first}")));
            continue;
        }
        let kind = match magic::sniff(&path) {
            Ok(Kind::Archive(kind)) => kind,
            Ok(_) => {
                out.leaves.push(path);
                continue;
            }
            Err(e) => {
                out.failures.push(UnpackFailure {
                    path,
                    reason: format!("could not be read: {e}"),
                });
                continue;
            }
        };
        if kind == Archive::Zip && is_arcade_zip(&path, dat) {
            out.leaves.push(path);
            continue;
        }
        if depth >= MAX_DEPTH {
            out.failures.push(UnpackFailure {
                path,
                reason: format!("archives nested more than {MAX_DEPTH} deep were not opened"),
            });
            continue;
        }
        let dest = fresh(&stem_of(&path));
        match unpack_one(&path, &dest, kind, password) {
            Ok(()) => {
                out.origins.push((dest.clone(), path.clone()));
                queue.push((dest, depth + 1));
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&dest);
                // A stream format's magic is only a couple of bytes, and a
                // cartridge can start with them by chance. If it will not
                // open and its name says it is a game, it is a game.
                let rom_named = play_ident::System::from_extension(&ext_of(&path)).is_some();
                let weak_magic = matches!(kind, Archive::Gzip);
                if rom_named && weak_magic {
                    out.leaves.push(path);
                } else {
                    out.failures.push(UnpackFailure {
                        path,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }
}

fn short(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    if cleaned.trim().is_empty() {
        "archive".to_string()
    } else {
        cleaned
    }
}

/// Files an operating system leaves in folders that nobody dropped on
/// purpose. They are not games, manuals or saves, so they get no line.
fn is_junk(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name == ".ds_store"
        || name == "thumbs.db"
        || name == "desktop.ini"
        || name == "__macosx"
        || name.starts_with("._")
        || name == ".git"
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// `game.7z.001` is part 1 of `game.7z`.
fn split_part(path: &Path) -> Option<(String, u32)> {
    let name = path.file_name()?.to_str()?;
    let (base, number) = name.rsplit_once('.')?;
    if number.len() != 3 || !number.bytes().all(|b| b.is_ascii_digit()) || base.is_empty() {
        return None;
    }
    let n: u32 = number.parse().ok()?;
    if n == 0 {
        return None;
    }
    Some((base.to_string(), n))
}

fn join_parts(parts: &[(u32, PathBuf)], into: &Path) -> std::io::Result<()> {
    if let Some(parent) = into.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = File::create(into)?;
    for (_, part) in parts {
        let mut input = File::open(part)?;
        std::io::copy(&mut input, &mut out)?;
    }
    out.flush()
}

/// If `path` is the second or later volume of a RAR set whose first volume
/// is here too, the name of that first volume. Opening the first reads them
/// all; opening a later one on its own fails.
fn later_rar_volume(path: &Path, present: &HashSet<String>) -> Option<String> {
    let name = lower_name(path);
    // New style: game.part2.rar, game.part02.rar, ...
    if let Some(stem) = name.strip_suffix(".rar") {
        if let Some((base, part)) = stem.rsplit_once(".part") {
            if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
                let n: u32 = part.parse().ok()?;
                if n > 1 {
                    for width in 1..=part.len().max(1) {
                        let first = format!("{base}.part{:0width$}.rar", 1, width = width);
                        if present.contains(&first) {
                            return Some(first);
                        }
                    }
                }
            }
        }
        return None;
    }
    // Old style: game.rar, game.r00, game.r01, ...
    let (base, ext) = name.rsplit_once('.')?;
    let bytes = ext.as_bytes();
    if bytes.len() == 3
        && (bytes[0] == b'r' || bytes[0] == b's')
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
    {
        let first = format!("{base}.rar");
        if present.contains(&first) {
            return Some(first);
        }
    }
    None
}

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

fn unpack_one(
    src: &Path,
    dest: &Path,
    kind: Archive,
    password: Option<&str>,
) -> Result<(), UnpackError> {
    fs::create_dir_all(dest).map_err(UnpackError::other)?;
    match kind {
        Archive::Zip => unpack_zip(src, dest, password),
        Archive::SevenZ => unpack_7z(src, dest, password),
        Archive::Rar => unpack_rar(src, dest, password),
        Archive::Gzip => unpack_stream(src, dest, kind, gzip_name(src)),
        Archive::Bzip2 | Archive::Xz | Archive::Zstd => unpack_stream(src, dest, kind, None),
        Archive::Tar => unpack_tar(src, dest),
    }
}

fn unpack_zip(src: &Path, dest: &Path, password: Option<&str>) -> Result<(), UnpackError> {
    let file = File::open(src).map_err(UnpackError::other)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        let msg = e.to_string();
        if msg.to_ascii_lowercase().contains("multi-disk")
            || msg.to_ascii_lowercase().contains("spanned")
        {
            UnpackError::Other(
                "this zip is one part of a split set; join the parts into one zip first"
                    .to_string(),
            )
        } else {
            UnpackError::Other(msg)
        }
    })?;
    let mut extracted = 0usize;
    let mut first_err: Option<String> = None;
    let mut entries = 0usize;

    for i in 0..archive.len() {
        let raw_name = archive
            .name_for_index(i)
            .map(str::to_string)
            .unwrap_or_default();
        let entry = match password {
            Some(pw) => archive.by_index_decrypt(i, pw.as_bytes()),
            None => archive.by_index(i),
        };
        let mut entry = match entry {
            Ok(e) => e,
            Err(e) => {
                entries += 1;
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
        entries += 1;
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
                drop(out);
                let _ = fs::remove_file(&out_path);
                if first_err.is_none() {
                    first_err = Some(e.to_string());
                }
            }
        }
    }

    if extracted == 0 && entries > 0 {
        if let Some(e) = first_err {
            if looks_like_password(&e) {
                return Err(UnpackError::PasswordProtected);
            }
            return Err(UnpackError::Other(e));
        }
    }
    Ok(())
}

fn unpack_7z(src: &Path, dest: &Path, password: Option<&str>) -> Result<(), UnpackError> {
    let pw = password
        .map(sevenz_rust2::Password::from)
        .unwrap_or_else(sevenz_rust2::Password::empty);
    let file = File::open(src).map_err(UnpackError::other)?;
    let root = dest.to_path_buf();
    let result = sevenz_rust2::decompress_with_extract_fn_and_password(
        file,
        dest,
        pw,
        |entry, reader, _suggested| {
            let Some(target) = safe_join(&root, entry.name()) else {
                return Ok(true);
            };
            sevenz_rust2::default_entry_extract_fn(entry, reader, &target)
        },
    );
    match result {
        Ok(()) => Ok(()),
        Err(sevenz_rust2::Error::PasswordRequired) => Err(UnpackError::PasswordProtected),
        Err(sevenz_rust2::Error::MaybeBadPassword(_)) => Err(UnpackError::PasswordProtected),
        Err(e) => {
            let msg = e.to_string();
            if looks_like_password(&msg) {
                Err(UnpackError::PasswordProtected)
            } else {
                Err(UnpackError::Other(msg))
            }
        }
    }
}

fn unpack_rar(src: &Path, dest: &Path, password: Option<&str>) -> Result<(), UnpackError> {
    use unrar::error::Code;
    let describe = |e: unrar::error::UnrarError| -> UnpackError {
        match e.code {
            Code::MissingPassword | Code::BadPassword => UnpackError::PasswordProtected,
            Code::EOpen => UnpackError::Other(
                "a volume of this RAR set is missing; drop every part together".to_string(),
            ),
            Code::BadData => UnpackError::Other("the RAR is damaged (bad data)".to_string()),
            Code::BadArchive | Code::UnknownFormat => {
                UnpackError::Other("this is not a RAR Play can read".to_string())
            }
            _ => UnpackError::other(e),
        }
    };
    let archive = match password {
        Some(pw) => unrar::Archive::with_password(src, pw.as_bytes()),
        None => unrar::Archive::new(src),
    };
    let mut open = archive.open_for_processing().map_err(describe)?;
    loop {
        let header = match open.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(describe(e)),
        };
        let entry = header.entry();
        let unsafe_name = entry.filename.is_absolute()
            || entry
                .filename
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)));
        if unsafe_name || !entry.is_file() {
            open = header.skip().map_err(describe)?;
            continue;
        }
        open = header.extract_with_base(dest).map_err(describe)?;
    }
    Ok(())
}

fn unpack_tar(src: &Path, dest: &Path) -> Result<(), UnpackError> {
    let file = File::open(src).map_err(UnpackError::other)?;
    let mut archive = tar::Archive::new(file);
    let entries = archive.entries().map_err(UnpackError::other)?;
    for entry in entries {
        let mut entry = entry.map_err(UnpackError::other)?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        // `unpack_in` refuses anything that would land outside `dest`.
        entry.unpack_in(dest).map_err(UnpackError::other)?;
    }
    Ok(())
}

/// The name gzip recorded for what it compressed, when it recorded one.
fn gzip_name(src: &Path) -> Option<String> {
    let file = File::open(src).ok()?;
    let decoder = flate2::read::GzDecoder::new(file);
    let raw = decoder.header()?.filename()?;
    let name = String::from_utf8_lossy(raw).to_string();
    let base = Path::new(&name).file_name()?.to_string_lossy().to_string();
    (!base.is_empty()).then_some(base)
}

fn unpack_stream(
    src: &Path,
    dest: &Path,
    kind: Archive,
    recorded: Option<String>,
) -> Result<(), UnpackError> {
    let name = recorded.unwrap_or_else(|| {
        let stem = match ext_of(src).as_str() {
            "tgz" | "tbz" | "tbz2" | "txz" | "tzst" => format!("{}.tar", stem_of(src)),
            _ => stem_of(src),
        };
        if stem.is_empty() || stem == "unnamed" {
            "file".to_string()
        } else {
            stem
        }
    });
    let Some(out_path) = safe_join(dest, &name) else {
        return Err(UnpackError::Other("unsafe entry name".to_string()));
    };
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(UnpackError::other)?;
    }
    let file = std::io::BufReader::new(File::open(src).map_err(UnpackError::other)?);
    let mut out = File::create(&out_path).map_err(UnpackError::other)?;
    let copied = match kind {
        Archive::Gzip => std::io::copy(&mut flate2::read::MultiGzDecoder::new(file), &mut out)
            .map(|_| ())
            .map_err(UnpackError::other),
        Archive::Bzip2 => std::io::copy(&mut bzip2::read::MultiBzDecoder::new(file), &mut out)
            .map(|_| ())
            .map_err(UnpackError::other),
        Archive::Zstd => zstd::stream::read::Decoder::new(file)
            .and_then(|mut d| std::io::copy(&mut d, &mut out))
            .map(|_| ())
            .map_err(UnpackError::other),
        Archive::Xz => {
            let mut reader = file;
            let mut writer = std::io::BufWriter::new(&mut out);
            lzma_rs::xz_decompress(&mut reader, &mut writer)
                .map_err(|e| UnpackError::Other(format!("xz: {e}")))
                .and_then(|()| writer.flush().map_err(UnpackError::other))
        }
        _ => Err(UnpackError::Other("not a stream format".to_string())),
    };
    if copied.is_err() {
        drop(out);
        let _ = fs::remove_file(&out_path);
    }
    copied
}

fn looks_like_password(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("password") || lower.contains("encrypted") || lower.contains("invalidpassword")
}

pub fn is_disc_ext(ext: &str) -> bool {
    matches!(
        ext,
        "bin" | "cue" | "chd" | "iso" | "pbp" | "gcm" | "wbfs" | "rvz" | "nkit" | "img" | "mdf"
    )
}

pub fn is_rider_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "pdf"
            | "doc"
            | "docx"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "nfo"
            | "md"
            | "html"
            | "htm"
            | "rtf"
            | "epub"
            | "srt"
            | "sub"
            | "chm"
    )
}

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
        unpack_stream(&gz_path, &out, Archive::Gzip, gzip_name(&gz_path)).unwrap();
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
