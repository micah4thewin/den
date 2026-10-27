#![cfg(unix)]

use play_core::Play;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// A RetroArch that keeps running until it is told to stop. `exec` so the
/// signal reaches the thing that is actually waiting, not a shell in front
/// of it.
fn patient_retroarch(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("retroarch");
    fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A library pointed at that RetroArch by hand, so nothing here touches the
/// process-wide `RETROARCH` and these tests can run beside each other.
fn library_with(tmp: &Path) -> Play {
    let play = Play::open(&tmp.join("play")).unwrap();
    play.set_retroarch_path(Some(patient_retroarch(tmp)))
        .unwrap();
    play
}

fn shelved_game(play: &Play, drop: &Path, name: &str) -> play_core::Game {
    fs::create_dir_all(drop).unwrap();
    fs::write(drop.join(name), b"NES\x1a\x02\x01\x01\x00rom").unwrap();
    play.intake(drop, None).unwrap();
    play.db()
        .list_games("", None)
        .unwrap()
        .into_iter()
        .find(|g| g.title.starts_with(name.split(" (").next().unwrap()))
        .expect("the game was shelved")
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_game_can_be_named_while_it_plays_and_stopped_from_away() {
    let tmp = tempfile::tempdir().unwrap();
    let play = library_with(tmp.path());
    let game = shelved_game(&play, &tmp.path().join("downloads"), "Zelda (USA).nes");

    assert!(play.running().is_empty());
    play.launch(game.id).unwrap();

    let playing = play.running();
    assert_eq!(playing.len(), 1);
    assert_eq!(playing[0].game_id, game.id);
    assert_eq!(playing[0].system, "NES");
    assert!(playing[0].pid > 0);
    assert!(playing[0].started > 0);

    assert_eq!(play.stop(game.id), 1);
    assert!(
        wait_until(|| play.running().is_empty()),
        "the game was still playing after it was stopped"
    );

    let sessions = play.db().recent_sessions(4).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(
        sessions[0].0.duration_seconds.is_some(),
        "stopping left the session open"
    );

    // Stopping what is not playing is the outcome that was asked for.
    assert_eq!(play.stop(game.id), 0);
}

#[test]
fn the_same_game_is_never_started_twice_over_its_own_save() {
    let tmp = tempfile::tempdir().unwrap();
    let play = library_with(tmp.path());
    let game = shelved_game(&play, &tmp.path().join("downloads"), "Metroid (USA).nes");

    play.launch(game.id).unwrap();
    let again = play.launch(game.id);
    let message = again.expect_err("a second copy was allowed").to_string();
    assert!(
        message.contains("already running"),
        "the refusal has to say why: {message}"
    );

    assert_eq!(play.running().len(), 1);
    assert_eq!(play.stop_all(), 1);
    assert!(wait_until(|| play.running().is_empty()));

    // With the first copy gone, the game starts again.
    play.launch(game.id).unwrap();
    assert_eq!(play.running().len(), 1);
    play.stop_all();
}
