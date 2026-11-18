//! The drops people actually make: every archive format, archives inside
//! archives, sets split into parts, a folder and some loose files at once,
//! and the litter an operating system leaves behind. Every file in gets
//! exactly one word out, and nothing in the drop is touched.

use play_db::Db;
use play_ident::dat::Index;
use play_intake::{IntakeOptions, Outcome, Report};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const NES: &[u8] = b"NES\x1a\x02\x01\x01\x00";

fn rom(tag: u8) -> Vec<u8> {
    let mut v = NES.to_vec();
    v.extend(std::iter::repeat_n(tag, 4096));
    v
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn zip_of(path: &Path, files: &[(&str, &[u8])], method: zip::CompressionMethod) {
    let f = fs::File::create(path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(method);
    for (name, bytes) in files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap();
}

fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *bytes).unwrap();
    }
    builder.into_inner().unwrap()
}

fn seven_z_of(path: &Path, files: &[(&str, &[u8])]) {
    let src = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        let p = src.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    sevenz_rust2::compress_to_path(src.path(), path).unwrap();
}

struct Run {
    report: Report,
    db: Db,
    _tmp: tempfile::TempDir,
    library: PathBuf,
}

fn intake(drops: &[PathBuf], tmp: tempfile::TempDir, password: Option<&str>) -> Run {
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let dat = Index::default();
    let opts = IntakeOptions {
        library: library.clone(),
        dat: &dat,
        db: Some(&db),
        password: password.map(str::to_string),
    };
    let report = play_intake::run_intake_many(drops, &opts).unwrap();
    Run {
        report,
        db,
        _tmp: tmp,
        library,
    }
}

fn line<'a>(report: &'a Report, ends_with: &str) -> &'a Outcome {
    &report
        .entries
        .iter()
        .find(|e| e.input.replace('\\', "/").ends_with(ends_with))
        .unwrap_or_else(|| panic!("no line for {ends_with}: {:#?}", report.entries))
        .outcome
}

fn tree(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            out.push((path.clone(), fs::metadata(&path).unwrap().len()));
        }
    }
    out.sort();
    out
}

#[test]
fn every_archive_format_opens_without_anything_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();

    seven_z_of(&drop.join("Seven.7z"), &[("Seven (USA).nes", &rom(1))]);
    zip_of(
        &drop.join("Bzipped.zip"),
        &[("Bzipped (USA).nes", &rom(2))],
        zip::CompressionMethod::Bzip2,
    );
    zip_of(
        &drop.join("Zstded.zip"),
        &[("Zstded (USA).nes", &rom(3))],
        zip::CompressionMethod::Zstd,
    );

    let tar = tar_of(&[("Tarred (USA).nes", &rom(4))]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    fs::write(drop.join("Gz.tar.gz"), gz.finish().unwrap()).unwrap();

    let tar = tar_of(&[("Tgz (USA).nes", &rom(5))]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    fs::write(drop.join("Short.tgz"), gz.finish().unwrap()).unwrap();

    let tar = tar_of(&[("Xzed (USA).nes", &rom(6))]);
    let mut xz = Vec::new();
    lzma_rs::xz_compress(&mut &tar[..], &mut xz).unwrap();
    fs::write(drop.join("Xz.tar.xz"), xz).unwrap();

    let tar = tar_of(&[("Bz (USA).nes", &rom(7))]);
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    bz.write_all(&tar).unwrap();
    fs::write(drop.join("Bz.tar.bz2"), bz.finish().unwrap()).unwrap();

    fs::write(
        drop.join("Zst (USA).nes.zst"),
        zstd::encode_all(&rom(8)[..], 3).unwrap(),
    )
    .unwrap();

    fs::copy(fixture("version.rar"), drop.join("version.rar")).unwrap();
    fs::copy(fixture("solid.rar"), drop.join("solid.rar")).unwrap();

    let before = tree(&drop);
    let run = intake(std::slice::from_ref(&drop), tmp, None);
    let r = &run.report;

    for name in [
        "Seven.7z/Seven (USA).nes",
        "Bzipped.zip/Bzipped (USA).nes",
        "Zstded.zip/Zstded (USA).nes",
        "Tarred (USA).nes",
        "Tgz (USA).nes",
        "Xzed (USA).nes",
        "Bz (USA).nes",
        "Zst (USA).nes",
    ] {
        assert_eq!(line(r, name).word(), "probable", "{name}: {:#?}", r.entries);
    }
    // The RAR fixtures hold text, not games: what matters is that they
    // opened, which an `unsupported` line for what was inside them shows.
    for e in &r.entries {
        assert!(
            !matches!(e.outcome, Outcome::Quarantined { .. }),
            "nothing here should be quarantined: {e:?}"
        );
        assert!(
            !e.input.contains("_staging") && !e.input.contains("intake-"),
            "a report line names the staging folder: {}",
            e.input
        );
    }
    assert!(r.entries.iter().any(|e| e.input.contains("version.rar")));
    assert!(r.entries.iter().any(|e| e.input.contains("solid.rar")));
    assert_eq!(run.db.game_count().unwrap(), 8);
    assert_eq!(before, tree(&drop), "the drop was modified");
    assert!(
        !run.library.join("_staging").exists(),
        "staging was left behind"
    );
}

#[test]
fn archives_inside_archives_inside_archives() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("nest");
    fs::create_dir_all(&drop).unwrap();
    let work = tempfile::tempdir().unwrap();
    let inner = work.path().join("inner.zip");
    zip_of(
        &inner,
        &[("Deep (USA).nes", &rom(9))],
        zip::CompressionMethod::Deflated,
    );
    let middle = work.path().join("middle.7z");
    seven_z_of(&middle, &[("inner.zip", &fs::read(&inner).unwrap())]);
    zip_of(
        &drop.join("outer.zip"),
        &[("middle.7z", &fs::read(&middle).unwrap())],
        zip::CompressionMethod::Stored,
    );

    let run = intake(std::slice::from_ref(&drop), tmp, None);
    assert_eq!(
        line(&run.report, "outer.zip/middle.7z/inner.zip/Deep (USA).nes").word(),
        "probable"
    );
}

#[test]
fn a_set_split_into_parts_is_joined_and_each_part_is_accounted_for() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("split");
    fs::create_dir_all(&drop).unwrap();

    let work = tempfile::tempdir().unwrap();
    let whole = work.path().join("Split.7z");
    // Incompressible bytes, so the archive is big enough to cut in three.
    let mut noisy = NES.to_vec();
    let mut x: u32 = 12345;
    for _ in 0..30_000 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        noisy.push((x >> 16) as u8);
    }
    seven_z_of(&whole, &[("Split (USA).nes", &noisy)]);
    let bytes = fs::read(&whole).unwrap();
    let third = bytes.len() / 3 + 1;
    for (i, chunk) in bytes.chunks(third).enumerate() {
        fs::write(drop.join(format!("Split.7z.{:03}", i + 1)), chunk).unwrap();
    }

    let mut iso = vec![0u8; 0x9000];
    iso[0x8001..0x8006].copy_from_slice(b"CD001");
    fs::write(drop.join("Disc (USA).iso.001"), &iso[..0x5000]).unwrap();
    fs::write(drop.join("Disc (USA).iso.002"), &iso[0x5000..]).unwrap();

    let run = intake(std::slice::from_ref(&drop), tmp, None);
    let r = &run.report;
    assert_eq!(line(r, "Split (USA).nes").word(), "probable");
    assert_eq!(line(r, "Split.7z.002").word(), "extra");
    assert_eq!(line(r, "Split.7z.003").word(), "extra");
    assert_eq!(line(r, "Disc (USA).iso.002").word(), "extra");
    assert!(
        r.entries
            .iter()
            .any(|e| e.input.ends_with("Disc (USA).iso") && e.outcome.word() == "probable"),
        "{:#?}",
        r.entries
    );
}

#[test]
fn a_split_set_with_a_part_missing_says_which() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("gappy");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Gap.7z.001"), b"7z\xbc\xaf\x27\x1c").unwrap();
    fs::write(drop.join("Gap.7z.003"), b"tail").unwrap();
    let run = intake(std::slice::from_ref(&drop), tmp, None);
    match line(&run.report, "Gap.7z.001") {
        Outcome::Quarantined { reason } => assert!(reason.contains("002"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rar_volume_without_its_first_part_is_quarantined_with_a_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("rar");
    fs::create_dir_all(&drop).unwrap();
    fs::copy(fixture("archive.part1.rar"), drop.join("archive.part1.rar")).unwrap();
    let run = intake(std::slice::from_ref(&drop), tmp, None);
    match line(&run.report, "archive.part1.rar") {
        Outcome::Quarantined { reason } => {
            assert!(reason.contains("volume"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn later_rar_volumes_are_read_with_the_first() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("rar");
    fs::create_dir_all(&drop).unwrap();
    fs::copy(fixture("archive.part1.rar"), drop.join("archive.part1.rar")).unwrap();
    fs::write(drop.join("archive.part2.rar"), b"Rar!\x1a\x07\x00").unwrap();
    let run = intake(std::slice::from_ref(&drop), tmp, None);
    match line(&run.report, "archive.part2.rar") {
        Outcome::Extra { note } => assert!(note.contains("archive.part1.rar"), "{note}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_encrypted_rar_asks_for_its_password_and_opens_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("locked");
    fs::create_dir_all(&drop).unwrap();
    fs::copy(fixture("crypted.rar"), drop.join("crypted.rar")).unwrap();

    let run = intake(std::slice::from_ref(&drop), tmp, None);
    match line(&run.report, "crypted.rar") {
        Outcome::Quarantined { reason } => assert!(reason.contains("password"), "{reason}"),
        other => panic!("{other:?}"),
    }

    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("locked");
    fs::create_dir_all(&drop).unwrap();
    fs::copy(fixture("crypted.rar"), drop.join("crypted.rar")).unwrap();
    let run = intake(std::slice::from_ref(&drop), tmp, Some("unrar"));
    assert!(
        run.report
            .entries
            .iter()
            .all(|e| !matches!(e.outcome, Outcome::Quarantined { .. })),
        "{:#?}",
        run.report.entries
    );
}

#[test]
fn folders_loose_files_and_archives_dropped_together() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("Folder");
    fs::create_dir_all(folder.join("sub")).unwrap();
    fs::write(folder.join("sub/In Folder (USA).nes"), rom(10)).unwrap();
    fs::write(folder.join(".DS_Store"), b"junk").unwrap();
    fs::write(folder.join("._In Folder (USA).nes"), b"junk").unwrap();
    fs::create_dir_all(folder.join("__MACOSX")).unwrap();
    fs::write(folder.join("__MACOSX/whatever"), b"junk").unwrap();

    let loose = tmp.path().join("Loose (USA).nes");
    fs::write(&loose, rom(11)).unwrap();
    let zipped = tmp.path().join("Zipped.zip");
    zip_of(
        &zipped,
        &[("Zipped (USA).nes", &rom(12))],
        zip::CompressionMethod::Deflated,
    );

    let drops = vec![folder, loose, zipped];
    let run = intake(&drops, tmp, None);
    let r = &run.report;
    assert_eq!(r.entries.len(), 3, "junk got a line: {:#?}", r.entries);
    assert_eq!(line(r, "Folder/sub/In Folder (USA).nes").word(), "probable");
    assert_eq!(line(r, "Loose (USA).nes").word(), "probable");
    assert_eq!(line(r, "Zipped.zip/Zipped (USA).nes").word(), "probable");
}

#[test]
fn a_dropped_file_that_vanished_is_a_line_not_a_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let gone = tmp.path().join("Gone (USA).nes");
    let here = tmp.path().join("Here (USA).nes");
    fs::write(&here, rom(13)).unwrap();
    let run = intake(&[gone, here], tmp, None);
    assert_eq!(line(&run.report, "Gone (USA).nes").word(), "quarantined");
    assert_eq!(line(&run.report, "Here (USA).nes").word(), "probable");
}

#[test]
fn a_damaged_archive_is_quarantined_and_the_rest_still_shelves() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("mixed");
    fs::create_dir_all(&drop).unwrap();
    let mut broken = b"PK\x03\x04".to_vec();
    broken.extend([0u8; 64]);
    fs::write(drop.join("broken.zip"), broken).unwrap();
    fs::write(drop.join("Fine (USA).nes"), rom(14)).unwrap();
    let run = intake(std::slice::from_ref(&drop), tmp, None);
    assert_eq!(line(&run.report, "broken.zip").word(), "quarantined");
    assert_eq!(line(&run.report, "Fine (USA).nes").word(), "probable");
}

#[test]
fn dropping_the_same_thing_twice_costs_no_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("again");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Again (USA).nes"), rom(15)).unwrap();
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let dat = Index::default();
    let opts = IntakeOptions {
        library: library.clone(),
        dat: &dat,
        db: Some(&db),
        password: None,
    };
    play_intake::run_intake(&drop, &opts).unwrap();
    let before = tree(&library);
    let second = play_intake::run_intake(&drop, &opts).unwrap();
    assert_eq!(line(&second, "Again (USA).nes").word(), "duplicate");
    let after: Vec<_> = tree(&library)
        .into_iter()
        .filter(|(p, _)| p.extension().is_none_or(|e| e != "db-wal" && e != "db-shm"))
        .collect();
    let before: Vec<_> = before
        .into_iter()
        .filter(|(p, _)| p.extension().is_none_or(|e| e != "db-wal" && e != "db-shm"))
        .collect();
    assert_eq!(before, after);
}
