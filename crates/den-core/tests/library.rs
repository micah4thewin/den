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
fn choosing_a_retroarch_that_does_not_work_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let den = Den::open(&tmp.path().join("den")).unwrap();

    // Nothing chosen to begin with.
    assert!(!den.retroarch_status().chosen);

    let nowhere = tmp.path().join("not-a-retroarch");
    let err = den
        .set_retroarch_path(Some(nowhere.clone()))
        .expect_err("a path that cannot run should be refused");
    assert!(err.to_string().contains("not-a-retroarch"), "{err}");

    // And the refusal left nothing behind: the search is still in charge,
    // rather than the library now pointing at something that cannot run.
    let status = den.retroarch_status();
    assert!(!status.chosen, "a refused choice was kept anyway");

    // It also survives a reopen, which is where a half-written setting shows.
    drop(den);
    let den = Den::open(&tmp.path().join("den")).unwrap();
    assert!(!den.retroarch_status().chosen);
}

/// A Flatpak or Snap RetroArch is a symlink to a multiplexer that behaves
/// like RetroArch only because it looks at the name it was invoked under.
/// Storing where it points, rather than what was picked, hands Den a
/// `flatpak` that exits with a usage message while reporting success.
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

    let den = Den::open(&tmp.path().join("den")).unwrap();
    let status = den.set_retroarch_path(Some(wrapper.clone())).unwrap();
    let expected = wrapper.to_str().unwrap();
    assert_eq!(status.path.as_deref(), Some(expected));
    assert_eq!(
        den.db().setting("retroarch_path").unwrap().as_deref(),
        Some(expected),
        "the multiplexer was written down instead of the wrapper"
    );

    // And it is still the wrapper after a restart.
    drop(den);
    let den = Den::open(&tmp.path().join("den")).unwrap();
    assert_eq!(den.retroarch_status().path.as_deref(), Some(expected));
}

#[test]
fn a_system_den_cannot_launch_says_so_rather_than_blaming_a_core() {
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
    let game = den.db().get_game(id).unwrap().unwrap();
    let core = den.core_status(&game);
    let reason = core
        .unsupported
        .expect("PS2 cannot be launched, whatever is installed");
    assert!(reason.contains("PlayStation 2"), "{reason}");
    assert_eq!(
        core.installed, None,
        "there is no core to be missing when the system cannot be launched"
    );

    // And an ordinary system is not marked unsupported.
    let id = den
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
    let game = den.db().get_game(id).unwrap().unwrap();
    let core = den.core_status(&game);
    assert_eq!(core.unsupported, None);
    assert_eq!(core.name, "mupen64plus_next");
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
