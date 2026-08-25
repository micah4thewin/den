use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PlayerBinding {
    pub player: usize,
    pub joypad_index: Option<usize>,
}

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

pub const KEYBOARD_CHROME: &[(&str, &str, &str)] = &[
    ("input_menu_toggle", "f1", "F1"),
    ("input_exit_emulator", "escape", "Escape"),
    ("input_save_state", "f2", "F2"),
    ("input_load_state", "f4", "F4"),
    ("input_toggle_fullscreen", "f11", "F11"),
];

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

pub const MAX_PLAYERS: usize = 4;

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

    content.push('\n');
    content.push_str(&format!("input_max_users = \"{}\"\n", MAX_PLAYERS));
    for binding in players {
        if let Some(index) = binding.joypad_index {
            content.push_str(&format!(
                "input_player{}_joypad_index = \"{index}\"\n",
                binding.player
            ));
        }
    }

    content.push('\n');
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

        assert!(text.contains("video_driver = \"vulkan\""));
        assert!(text.contains("input_player1_a = \"x\""));
        assert!(text.contains("video_shader_enable = \"true\""));
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
        assert!(text.contains("video_driver = \"gl\""));
    }
}
