#![cfg(unix)]

use den_core::Den;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn fake_retroarch(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("retroarch");
    let log = dir.join("argv");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_launch_is_recorded_and_closed_when_the_emulator_exits() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("RETROARCH", fake_retroarch(tmp.path()));

    let drop = tmp.path().join("downloads");
    fs::create_dir_all(&drop).unwrap();
    fs::write(drop.join("Zelda (USA).nes"), b"NES\x1a\x02\x01\x01\x00rom").unwrap();

    let den = Den::open(&tmp.path().join("den")).unwrap();
    den.intake(&drop, None).unwrap();
    let game = den.db().list_games("", None).unwrap().remove(0);

    assert!(den.db().recent_games(4).unwrap().is_empty());

    let info = den.launch(game.id).unwrap();
    assert!(info.pid > 0);
    assert_eq!(info.core, "mesen");

    let recent = den.db().recent_games(4).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, game.id);

    for _ in 0..200 {
        den.reap();
        if den.running_count() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(den.running_count(), 0, "the process was never reaped");

    let sessions = den.db().recent_sessions(4).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(
        sessions[0].0.duration_seconds.is_some(),
        "the session was left open"
    );

    let argv = fs::read_to_string(tmp.path().join("argv")).unwrap();
    let args: Vec<&str> = argv.lines().collect();
    let core_arg = args
        .iter()
        .position(|a| *a == "-L")
        .map(|i| args[i + 1])
        .expect("a core was passed");
    assert!(
        core_arg.contains("mesen_libretro."),
        "the core has to be a file name: {core_arg}"
    );
    assert!(args.contains(&"--fullscreen"));
    assert!(args.iter().any(|a| a.ends_with("Zelda (USA).nes")));

    std::env::remove_var("RETROARCH");
}
