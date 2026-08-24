use std::fs;
use std::path::Path;

/// One player's controls for a session.
#[derive(Debug, Clone)]
pub struct PlayerBinding {
    /// Which player, from 1.
    pub player: usize,
    /// The joystick index RetroArch should use for them, if a pad is theirs.
    pub joypad_index: Option<usize>,
}

/// Den's keyboard scheme for player one.
///
/// RetroArch has defaults, but nobody can see them, and a launcher whose
/// answer to "how do I play this" is "read the emulator's settings" has not
/// answered. These are written into every session and shown on the
/// Controllers screen, so what the interface says and what the keys do are
/// the same thing by construction.
///
/// The layout is the one an emulator keyboard player expects: the arrow keys
/// for the pad, the right hand on Z/X/A/S for the face buttons in the shape
/// they sit on a controller, Enter and Right Shift for Start and Select.
pub const KEYBOARD_SCHEME: &[(&str, &str, &str)] = &[
    ("up", "up", "Up"),
    ("down", "down", "Down"),
    ("left", "left", "Left"),
    ("right", "right", "Right"),
    ("b", "z", "Z"),
    ("a", "x", "X"),
    ("y", "a", "A"),
    ("x", "s", "S"),
    ("l", "q", "Q"),
    ("r", "w", "W"),
    ("start", "enter", "Enter"),
    ("select", "rshift", "Right Shift"),
];

/// The keys Den binds that are not one player's buttons.
pub const KEYBOARD_CHROME: &[(&str, &str, &str)] = &[
    ("input_menu_toggle", "f1", "F1"),
    ("input_exit_emulator", "escape", "Escape"),
    ("input_save_state", "f2", "F2"),
    ("input_load_state", "f4", "F4"),
    ("input_toggle_fullscreen", "f11", "F11"),
];

/// The keys Den sets for a session. Everything else is the person's own.
const DEN_KEYS: &[&str] = &[
    "video_fullscreen",
    "input_autodetect_enable",
    "savestate_auto_save",
    "savestate_auto_load",
    "savestate_auto_save_interval",
    "savefile_directory",
    "savestate_directory",
    "rgui_show_start_screen",
    "libretro_directory",
];

/// How many players a session is configured for.
pub const MAX_PLAYERS: usize = 4;

/// Write the private config for one session.
///
/// `--config` replaces RetroArch's configuration rather than adding to it, so
/// this starts from the person's own file and overrides only the handful of
/// keys Den has an opinion about. Otherwise every launch through Den would
/// quietly discard their video driver, their pad bindings, their shaders --
/// everything they set up in RetroArch itself.
pub(crate) fn write_config(
    path: &Path,
    save_dir: &Path,
    state_dir: &Path,
    core_dir: Option<&Path>,
    inherit: Option<&Path>,
    players: &[PlayerBinding],
) -> std::io::Result<()> {
    let mut content = String::new();
    let overridden = den_keys();
    if let Some(theirs) = inherit.and_then(|p| fs::read_to_string(p).ok()) {
        content.push_str("# Den session config: their RetroArch settings, then ours.\n");
        for line in theirs.lines() {
            let key = line.split_once('=').map(|(k, _)| k.trim()).unwrap_or("");
            if overridden.iter().any(|k| k == key) {
                continue;
            }
            content.push_str(line);
            content.push('\n');
        }
        content.push('\n');
    }
    content.push_str("# Den, for this session\n");
    content.push_str(&format!(
        "video_fullscreen = \"true\"\n\
         input_autodetect_enable = \"true\"\n\
         savestate_auto_save = \"true\"\n\
         savestate_auto_load = \"false\"\n\
         savestate_auto_save_interval = \"60\"\n\
         savefile_directory = \"{}\"\n\
         savestate_directory = \"{}\"\n\
         rgui_show_start_screen = \"false\"\n",
        save_dir.display(),
        state_dir.display()
    ));
    if let Some(cores) = core_dir {
        content.push_str(&format!("libretro_directory = \"{}\"\n", cores.display()));
    }

    // Which pad answers for which player. RetroArch's udev joypad driver
    // fills its slots densely from 0 in attach order, so the index den-input
    // reports is the pad's rank among attached pads, not its js number.
    content.push_str("\n# Den, controllers\n");
    content.push_str(&format!("input_max_users = \"{}\"\n", MAX_PLAYERS));
    for binding in players {
        if let Some(index) = binding.joypad_index {
            content.push_str(&format!(
                "input_player{}_joypad_index = \"{index}\"\n",
                binding.player
            ));
        }
    }

    // And the keyboard, always, so there is a way to play whether or not a
    // pad turned up. These are player one's keys; a pad assigned to player
    // one works alongside them rather than instead of them.
    content.push_str("\n# Den, keyboard for player one\n");
    for (button, key, _shown) in KEYBOARD_SCHEME {
        content.push_str(&format!("input_player1_{button} = \"{key}\"\n"));
    }
    for (setting, key, _shown) in KEYBOARD_CHROME {
        content.push_str(&format!("{setting} = \"{key}\"\n"));
    }
    fs::write(path, content)
}

fn den_keys() -> Vec<String> {
    let mut keys: Vec<String> = DEN_KEYS.iter().map(|k| k.to_string()).collect();
    keys.push("input_max_users".to_string());
    for player in 1..=MAX_PLAYERS {
        keys.push(format!("input_player{player}_joypad_index"));
    }
    for (button, _, _) in KEYBOARD_SCHEME {
        keys.push(format!("input_player1_{button}"));
    }
    for (setting, _, _) in KEYBOARD_CHROME {
        keys.push(setting.to_string());
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn config_includes_save_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            None,
            &[],
        )
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("/lib/saves"));
        assert!(text.contains("savestate_auto_save_interval"));
        assert!(!text.contains("libretro_directory"));
    }

    #[test]
    fn config_names_the_core_directory_when_one_was_found() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            Some(Path::new("/usr/lib/libretro")),
            None,
            &[],
        )
        .unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("libretro_directory = \"/usr/lib/libretro\""));
    }

    #[test]
    fn a_session_keeps_the_settings_the_person_chose() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("retroarch.cfg");
        fs::write(
            &theirs,
            "# their config\n\
             video_driver = \"vulkan\"\n\
             input_player1_a = \"x\"\n\
             video_fullscreen = \"false\"\n\
             libretro_directory = \"/somewhere/else\"\n\
             video_shader_enable = \"true\"\n",
        )
        .unwrap();

        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            Some(Path::new("/usr/lib/libretro")),
            Some(&theirs),
            &[],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();

        // Theirs, kept.
        assert!(text.contains("video_driver = \"vulkan\""));
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("video_shader_enable = \"true\""));
        // Ours, and only once, so RetroArch cannot read the wrong one.
        assert_eq!(text.matches("video_fullscreen =").count(), 1);
        assert!(text.contains("video_fullscreen = \"true\""));
        assert_eq!(text.matches("libretro_directory =").count(), 1);
        assert!(text.contains("libretro_directory = \"/usr/lib/libretro\""));
    }

    #[test]
    fn a_session_always_has_keys_to_play_with() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            None,
            &[],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        // With no pad at all there is still a way to play, and a way out.
        assert!(text.contains("input_player1_up = \"up\""));
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("input_player1_start = \"enter\""));
        assert!(text.contains("input_exit_emulator = \"escape\""));
        assert!(!text.contains("input_player1_joypad_index"));
    }

    #[test]
    fn a_pad_is_told_to_retroarch_as_the_player_it_answers_for() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            None,
            &[
                PlayerBinding {
                    player: 1,
                    joypad_index: Some(2),
                },
                PlayerBinding {
                    player: 2,
                    joypad_index: Some(0),
                },
                // A pad with no joystick node has no index to give.
                PlayerBinding {
                    player: 3,
                    joypad_index: None,
                },
            ],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(
            text.contains("input_player1_joypad_index = \"2\""),
            "{text}"
        );
        assert!(
            text.contains("input_player2_joypad_index = \"0\""),
            "{text}"
        );
        assert!(!text.contains("input_player3_joypad_index"), "{text}");
        assert!(text.contains("input_max_users = \"4\""));
        // The keyboard is still there beside the pad.
        assert!(text.contains("input_player1_up = \"up\""));
    }

    #[test]
    fn ours_replace_theirs_rather_than_being_set_twice() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("retroarch.cfg");
        fs::write(
            &theirs,
            "input_player1_a = \"k\"\n\
             input_player1_joypad_index = \"7\"\n\
             input_exit_emulator = \"nul\"\n\
             input_max_users = \"1\"\n\
             video_driver = \"gl\"\n",
        )
        .unwrap();
        let cfg = dir.path().join("den-1.cfg");
        write_config(
            &cfg,
            Path::new("/lib/saves"),
            Path::new("/lib/savestates"),
            None,
            Some(&theirs),
            &[PlayerBinding {
                player: 1,
                joypad_index: Some(0),
            }],
        )
        .unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        for key in [
            "input_player1_a",
            "input_player1_joypad_index",
            "input_exit_emulator",
            "input_max_users",
        ] {
            assert_eq!(
                text.matches(&format!("{key} =")).count(),
                1,
                "{key} is set twice, and RetroArch would take the wrong one"
            );
        }
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("input_player1_joypad_index = \"0\""));
        // And what Den has no opinion about is left alone.
        assert!(text.contains("video_driver = \"gl\""));
    }
}
