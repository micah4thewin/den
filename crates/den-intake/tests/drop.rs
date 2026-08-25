use den_db::Db;
use den_ident::dat::Index;
use den_intake::{IntakeOptions, Outcome, Report};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn drop_folder(root: &Path) -> PathBuf {
    let drop = root.join("downloads");
    fs::create_dir_all(&drop).unwrap();

    let f = fs::File::create(drop.join("Pack.zip")).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    zw.start_file("Super Mario Bros (USA).nes", opts).unwrap();
    zw.write_all(b"NES\x1a\x02\x01\x01\x00rom bytes").unwrap();
    zw.start_file("readme.txt", opts).unwrap();
    zw.write_all(b"scanned manual").unwrap();
    zw.finish().unwrap();

    fs::write(drop.join("Twisted Metal (USA).bin"), vec![7u8; 4096]).unwrap();

    fs::write(drop.join("scph1001.bin"), vec![9u8; 2048]).unwrap();

    fs::write(
        drop.join("Final Fantasy VII (USA) (Disc 1).bin"),
        vec![1u8; 2048],
    )
    .unwrap();
    fs::write(
        drop.join("Final Fantasy VII (USA) (Disc 2).bin"),
        vec![2u8; 2048],
    )
    .unwrap();

    fs::write(drop.join("Super Mario Bros (USA).srm"), vec![3u8; 32]).unwrap();

    fs::write(
        drop.join("Sonic the Hedgehog (USA, Europe).md"),
        vec![5u8; 512],
    )
    .unwrap();

    drop
}

fn run(library: &Path, drop: &Path, db: &Db) -> Report {
    let dat = Index::default();
    let opts = IntakeOptions {
        library: library.to_path_buf(),
        dat: &dat,
        db: Some(db),
        password: None,
    };
    den_intake::run_intake(drop, &opts).unwrap()
}

fn word_for(report: &Report, file: &str) -> String {
    report
        .entries
        .iter()
        .find(|e| e.input.ends_with(file))
        .unwrap_or_else(|| panic!("{file} is missing from the report"))
        .outcome
        .word()
        .to_string()
}

#[test]
fn every_input_file_gets_exactly_one_word() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let report = run(&library, &drop, &db);

    let inputs: Vec<&str> = report.entries.iter().map(|e| e.input.as_str()).collect();
    assert_eq!(inputs.len(), 8, "{inputs:#?}");

    for file in [
        "Super Mario Bros (USA).nes",
        "readme.txt",
        "Twisted Metal (USA).bin",
        "scph1001.bin",
        "Final Fantasy VII (USA) (Disc 1).bin",
        "Final Fantasy VII (USA) (Disc 2).bin",
        "Super Mario Bros (USA).srm",
        "Sonic the Hedgehog (USA, Europe).md",
    ] {
        assert_eq!(
            inputs.iter().filter(|i| i.ends_with(file)).count(),
            1,
            "{file} should appear exactly once in {inputs:#?}"
        );
    }
}

#[test]
fn a_bios_is_filed_as_a_bios_and_not_as_a_playstation_game() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let report = run(&library, &drop, &db);

    assert_eq!(word_for(&report, "scph1001.bin"), "bios");
    assert!(library.join("bios").join("scph1001.bin").is_file());

    let titles: Vec<String> = db
        .list_games("", None)
        .unwrap()
        .into_iter()
        .map(|g| g.title)
        .collect();
    assert!(
        !titles.iter().any(|t| t.contains("scph")),
        "the BIOS was shelved as a game: {titles:?}"
    );
    assert!(!library.join("PlayStation").join("scph1001").exists());
}

#[test]
fn a_markdown_extension_that_is_also_a_cartridge_is_shelved_as_a_game() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let report = run(&library, &drop, &db);

    assert_eq!(
        word_for(&report, "Sonic the Hedgehog (USA, Europe).md"),
        "probable"
    );
    let sonic = db
        .list_games("Sonic", None)
        .unwrap()
        .into_iter()
        .next()
        .expect("the Genesis cartridge was shelved as a game, not as a manual");
    assert_eq!(sonic.system, "Genesis");
    assert!(library
        .join("Genesis")
        .join("Sonic the Hedgehog")
        .join("Sonic the Hedgehog (USA, Europe).md")
        .is_file());
}

#[test]
fn a_disc_without_a_cue_gets_one_and_a_set_gets_a_playlist() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let report = run(&library, &drop, &db);

    assert_eq!(word_for(&report, "Twisted Metal (USA).bin"), "repaired");
    let single = library.join("PlayStation").join("Twisted Metal");
    assert!(single.join("Twisted Metal (USA).cue").is_file());

    let first = report
        .entries
        .iter()
        .find(|e| e.input.ends_with("(Disc 1).bin"))
        .unwrap();
    match &first.outcome {
        den_intake::Outcome::Repaired { note, .. } => {
            assert!(note.contains("built multi-disc playlist"), "{note}");
        }
        other => panic!("expected a repair on the first disc, got {other:?}"),
    }

    let set = library.join("PlayStation").join("Final Fantasy VII");
    let m3u = fs::read_to_string(set.join("Final Fantasy VII.m3u")).unwrap();
    assert_eq!(
        m3u.lines().collect::<Vec<_>>(),
        vec![
            "Final Fantasy VII (USA) (Disc 1).cue",
            "Final Fantasy VII (USA) (Disc 2).cue"
        ]
    );

    for game in db.list_games("", None).unwrap() {
        assert!(
            Path::new(&game.path).is_file(),
            "{} points at a file that was never written: {}",
            game.title,
            game.path
        );
    }
}

#[test]
fn an_imported_save_is_attached_to_its_game() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    let report = run(&library, &drop, &db);

    assert_eq!(word_for(&report, "Super Mario Bros (USA).srm"), "extra");

    let mario = db
        .list_games("Super Mario", None)
        .unwrap()
        .into_iter()
        .next()
        .expect("the cartridge was shelved");
    let saves = db.list_saves(mario.id).unwrap();
    assert_eq!(saves.len(), 1, "the save was filed but never recorded");
    assert_eq!(saves[0].kind, "battery");
    assert!(Path::new(&saves[0].path).is_file());

    let cont = db.continue_game().unwrap().expect("a continue row");
    assert_eq!(cont.id, mario.id);
}

#[test]
fn running_the_same_drop_twice_adds_nothing_new() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();

    run(&library, &drop, &db);
    let first = db.game_count().unwrap();
    assert!(first > 0);

    let second = run(&library, &drop, &db);
    assert_eq!(db.game_count().unwrap(), first, "intake is not idempotent");
    assert!(
        second
            .entries
            .iter()
            .any(|e| matches!(e.outcome, Outcome::Duplicate { .. })),
        "the second run should have said `duplicate` somewhere: {:#?}",
        second.tally()
    );
}

#[test]
fn the_drop_is_never_modified() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = drop_folder(tmp.path());
    let before = tree(&drop);
    let library = tmp.path().join("library");
    let db = Db::open(&tmp.path().join("library.db")).unwrap();
    run(&library, &drop, &db);
    assert_eq!(before, tree(&drop), "intake touched the originals");
}

fn tree(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            out.push((path, size));
        }
    }
    out.sort();
    out
}
