use play_core::Play;
use std::fs;

#[test]
fn opening_a_library_creates_it() {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("play");
    let play = Play::open(&library).unwrap();
    assert!(library.join("library.db").is_file());
    assert_eq!(play.db().game_count().unwrap(), 0);
    drop(play);
    Play::open(&library).unwrap();
}

#[test]
fn intake_shelves_into_the_library_and_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Zelda (USA).nes"), b"NES\x1a\x02\x01\x01\x00rom").unwrap();

    let play = Play::open(&tmp.path().join("play")).unwrap();
    let report = play.intake(&drop, None).unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(play.db().game_count().unwrap(), 1);

    let game = play.db().list_games("", None).unwrap().remove(0);
    assert_eq!(game.title, "Zelda");
    assert_eq!(game.system, "NES");
    assert_eq!(game.playtime, 0);
    assert!(game.last_played.is_none());
}

#[test]
fn launching_a_missing_game_is_an_error_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let play = Play::open(&tmp.path().join("play")).unwrap();
    assert!(play.launch(4242).is_err());
}

#[test]
fn choosing_a_retroarch_that_does_not_work_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let play = Play::open(&tmp.path().join("play")).unwrap();

    assert!(!play.retroarch_status().chosen);

    let nowhere = tmp.path().join("not-a-retroarch");
    let err = play
        .set_retroarch_path(Some(nowhere.clone()))
        .expect_err("a path that cannot run should be refused");
    assert!(err.to_string().contains("not-a-retroarch"), "{err}");

    let status = play.retroarch_status();
    assert!(!status.chosen, "a refused choice was kept anyway");

    drop(play);
    let play = Play::open(&tmp.path().join("play")).unwrap();
    assert!(!play.retroarch_status().chosen);
}

#[cfg(unix)]
#[test]
fn a_chosen_wrapper_is_stored_as_it_was_picked() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let multiplexer = tmp.path().join("flatpak");
    fs::write(&multiplexer, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&multiplexer, fs::Permissions::from_mode(0o755)).unwrap();

    let wrapper = tmp.path().join("org.libretro.RetroArch");
    std::os::unix::fs::symlink(&multiplexer, &wrapper).unwrap();

    let play = Play::open(&tmp.path().join("play")).unwrap();
    let status = play.set_retroarch_path(Some(wrapper.clone())).unwrap();
    let expected = wrapper.to_str().unwrap();
    assert_eq!(status.path.as_deref(), Some(expected));
    assert_eq!(
        play.db().setting("retroarch_path").unwrap().as_deref(),
        Some(expected),
        "the multiplexer was written down instead of the wrapper"
    );

    drop(play);
    let play = Play::open(&tmp.path().join("play")).unwrap();
    assert_eq!(play.retroarch_status().path.as_deref(), Some(expected));
}

#[test]
fn a_system_play_cannot_launch_says_so_rather_than_blaming_a_core() {
    let tmp = tempfile::tempdir().unwrap();
    let play = Play::open(&tmp.path().join("play")).unwrap();
    let id = play
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
    let game = play.db().get_game(id).unwrap().unwrap();
    let core = play.core_status(&game);
    let reason = core
        .unsupported
        .expect("PS2 cannot be launched, whatever is installed");
    assert!(reason.contains("PlayStation 2"), "{reason}");
    assert_eq!(
        core.installed, None,
        "there is no core to be missing when the system cannot be launched"
    );

    let id = play
        .db()
        .add_game(
            "Zelda",
            "N64",
            std::path::Path::new("/nowhere/zelda.z64"),
            None,
            None,
            "added",
        )
        .unwrap();
    let game = play.db().get_game(id).unwrap().unwrap();
    let core = play.core_status(&game);
    assert_eq!(core.unsupported, None);
    assert_eq!(core.name, "mupen64plus_next");
}

#[test]
fn an_external_only_system_is_refused_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let play = Play::open(&tmp.path().join("play")).unwrap();
    let id = play
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
    let err = play.launch(id).unwrap_err().to_string();
    assert!(err.contains("PlayStation 2"), "unhelpful message: {err}");
}

#[test]
fn a_moved_library_keeps_its_games() {
    let tmp = tempfile::tempdir().unwrap();
    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Zelda (USA).nes"), b"NES\x1a\x02\x01\x01\x00rom").unwrap();
    let old = tmp.path().join("old");
    let new = tmp.path().join("share").join("play");
    Play::open(&old).unwrap().intake(&drop, None).unwrap();

    assert!(play_core::move_library(&old, &new).unwrap());
    assert!(!old.exists());
    let play = Play::open(&new).unwrap();
    let game = play.db().list_games("", None).unwrap().remove(0);
    assert!(
        std::path::Path::new(&game.path).starts_with(&new),
        "{}",
        game.path
    );
    assert!(std::path::Path::new(&game.path).is_file());

    // Once moved, a second call leaves both places alone.
    assert!(!play_core::move_library(&old, &new).unwrap());
}
