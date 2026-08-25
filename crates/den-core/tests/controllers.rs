#![cfg(unix)]

use den_core::{Den, MAX_PLAYERS};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

fn exclusive() -> MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

fn plant_pad(root: &Path, input: &str, js: &str, name: &str, vendor: &str, product: &str) {
    let device = root.join(input);
    fs::create_dir_all(device.join("id")).unwrap();
    fs::create_dir_all(device.join("capabilities")).unwrap();
    fs::write(device.join("name"), format!("{name}\n")).unwrap();
    fs::write(device.join("id/vendor"), format!("{vendor}\n")).unwrap();
    fs::write(device.join("id/product"), format!("{product}\n")).unwrap();
    let width = usize::BITS as usize;
    let mut words = vec!["0".to_string(); 12];
    let index = words.len() - 1 - (0x130 / width);
    words[index] = format!("{:x}", 1u64 << (0x130 % width));
    fs::write(device.join("capabilities/key"), words.join(" ")).unwrap();

    let node = root.join(js);
    fs::create_dir_all(&node).unwrap();
    std::os::unix::fs::symlink(&device, node.join("device")).unwrap();
}

fn fake_retroarch(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("retroarch");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let cores = dir.join("cores");
    fs::create_dir_all(&cores).unwrap();
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    fs::write(
        cores.join(format!("mupen64plus_next_libretro.{ext}")),
        b"core",
    )
    .unwrap();
    path
}

#[test]
fn a_pad_plugged_in_is_player_one_without_being_asked() {
    let _turn = exclusive();
    let tmp = tempfile::tempdir().unwrap();
    let sysfs = tmp.path().join("sysfs");
    fs::create_dir_all(&sysfs).unwrap();
    plant_pad(
        &sysfs,
        "input5",
        "js0",
        "Microsoft X-Box 360 pad",
        "045e",
        "028e",
    );
    std::env::set_var("DEN_SYSFS_INPUT", &sysfs);
    std::env::set_var("RETROARCH", fake_retroarch(&tmp.path().join("bin")));

    let den = Den::open(&tmp.path().join("den")).unwrap();
    let pads = den.controllers();
    assert_eq!(pads.len(), 1, "{pads:#?}");
    assert_eq!(
        pads[0].player,
        Some(1),
        "a controller you plugged in should be Player 1 without a ceremony"
    );

    drop(den);
    let den = Den::open(&tmp.path().join("den")).unwrap();
    assert_eq!(den.controllers()[0].player, Some(1));

    plant_pad(&sysfs, "input6", "js1", "8BitDo SN30 Pro", "2dc8", "6001");
    let pads = den.controllers();
    assert_eq!(pads.len(), 2);
    assert_eq!(pads[0].player, Some(1));
    assert_eq!(pads[1].player, Some(2));

    std::env::remove_var("DEN_SYSFS_INPUT");
    std::env::remove_var("RETROARCH");
}

#[test]
fn assigning_a_player_somebody_else_holds_swaps_them() {
    let _turn = exclusive();
    let tmp = tempfile::tempdir().unwrap();
    let sysfs = tmp.path().join("sysfs");
    fs::create_dir_all(&sysfs).unwrap();
    plant_pad(&sysfs, "input5", "js0", "Pad One", "0001", "0001");
    plant_pad(&sysfs, "input6", "js1", "Pad Two", "0002", "0002");
    std::env::set_var("DEN_SYSFS_INPUT", &sysfs);

    let den = Den::open(&tmp.path().join("den")).unwrap();
    let pads = den.controllers();
    let (one, two) = (pads[0].identity.clone(), pads[1].identity.clone());
    assert_eq!(pads[0].player, Some(1));
    assert_eq!(pads[1].player, Some(2));

    den.assign_pad(&two, Some(1)).unwrap();
    let pads = den.controllers();
    let player = |identity: &str| {
        pads.iter()
            .find(|p| p.identity == identity)
            .and_then(|p| p.player)
    };
    assert_eq!(player(&two), Some(1));
    assert_eq!(
        player(&one),
        Some(2),
        "the pad that lost player 1 should have taken the other's number, \
         not been left claiming a player that is gone"
    );

    den.assign_pad(&one, None).unwrap();
    for _ in 0..3 {
        let pads = den.controllers();
        assert_eq!(
            pads.iter()
                .find(|p| p.identity == one)
                .and_then(|p| p.player),
            None,
            "\"Nobody\" has to stick, or it is a control that does nothing"
        );
    }

    assert!(den.assign_pad(&two, Some(MAX_PLAYERS + 1)).is_err());
    assert_eq!(den.controllers()[1].player, Some(1));

    std::env::remove_var("DEN_SYSFS_INPUT");
}

#[test]
fn retroarch_is_told_which_pad_is_which_player() {
    let _turn = exclusive();
    let tmp = tempfile::tempdir().unwrap();
    let sysfs = tmp.path().join("sysfs");
    fs::create_dir_all(&sysfs).unwrap();
    plant_pad(&sysfs, "input5", "js0", "Pad One", "0001", "0001");
    plant_pad(&sysfs, "input6", "js1", "Pad Two", "0002", "0002");
    std::env::set_var("DEN_SYSFS_INPUT", &sysfs);
    std::env::set_var("RETROARCH", fake_retroarch(&tmp.path().join("bin")));

    let library = tmp.path().join("den");
    let den = Den::open(&library).unwrap();

    let drop_dir = tmp.path().join("downloads");
    fs::create_dir_all(&drop_dir).unwrap();
    let mut rom = vec![0u8; 4096];
    rom[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
    fs::write(drop_dir.join("Mario Kart 64 (USA).z64"), &rom).unwrap();
    den.intake(&drop_dir, None).unwrap();
    let game = den.db().list_games("", None).unwrap().remove(0);

    let second = den.controllers()[1].identity.clone();
    den.assign_pad(&second, Some(1)).unwrap();

    den.launch(game.id).expect("the stand-in emulator launches");
    let config = fs::read_to_string(library.join("_config").join(format!("den-{}.cfg", game.id)))
        .expect("a session config was written");

    assert!(
        config.contains("input_player1_joypad_index = \"1\""),
        "player one should be joystick 1:\n{config}"
    );
    assert!(
        config.contains("input_player2_joypad_index = \"0\""),
        "player two should be joystick 0:\n{config}"
    );
    assert!(config.contains("input_player1_up = \"up\""));
    assert!(config.contains("input_exit_emulator = \"escape\""));

    std::env::remove_var("DEN_SYSFS_INPUT");
    std::env::remove_var("RETROARCH");
}

#[test]
fn the_keyboard_scheme_shown_is_the_one_written() {
    let _turn = exclusive();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("DEN_SYSFS_INPUT", tmp.path().join("no-pads"));
    let den = Den::open(&tmp.path().join("den")).unwrap();

    let scheme = den.keyboard_scheme();
    assert!(!scheme.is_empty());
    for binding in &scheme {
        assert!(!binding.action.is_empty(), "{binding:?}");
        assert!(!binding.key.is_empty(), "{binding:?}");
        assert!(
            !binding.action.starts_with("input_"),
            "a RetroArch setting name leaked into the interface: {binding:?}"
        );
    }
    assert!(scheme
        .iter()
        .any(|b| b.action == "Start" && b.key == "Enter"));
    assert!(scheme
        .iter()
        .any(|b| b.action == "Quit back to Den" && b.key == "Escape"));

    std::env::remove_var("DEN_SYSFS_INPUT");
}
