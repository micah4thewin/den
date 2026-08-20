//! The glue object, from the shell's point of view.

use den_core::Den;
use std::fs;

#[test]
fn opening_a_library_creates_it() {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("den");
    let den = Den::open(&library).unwrap();
    assert!(library.join("library.db").is_file());
    assert_eq!(den.db().game_count().unwrap(), 0);
    // Opening the same library twice is the normal case, not an error.
    drop(den);
    Den::open(&library).unwrap();
}

#[test]
fn intake_shelves_into_the_library_and_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Zelda (USA).nes"), b"NES\x1a\x02\x01\x01\x00rom").unwrap();

    let den = Den::open(&tmp.path().join("den")).unwrap();
    let report = den.intake(&drop, None).unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(den.db().game_count().unwrap(), 1);

    let game = den.db().list_games("", None).unwrap().remove(0);
    assert_eq!(game.title, "Zelda");
    assert_eq!(game.system, "NES");
    assert_eq!(game.playtime, 0);
    assert!(game.last_played.is_none());
}

#[test]
fn launching_a_missing_game_is_an_error_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let den = Den::open(&tmp.path().join("den")).unwrap();
    assert!(den.launch(4242).is_err());
}

#[test]
fn an_external_only_system_is_refused_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let den = Den::open(&tmp.path().join("den")).unwrap();
    let id = den
        .db()
        .add_game(
            "Some Game",
            "PlayStation 2",
            std::path::Path::new("/nowhere/game.iso"),
            None,
            None,
            "added",
        )
        .unwrap();
    let err = den.launch(id).unwrap_err().to_string();
    assert!(err.contains("PlayStation 2"), "unhelpful message: {err}");
}
