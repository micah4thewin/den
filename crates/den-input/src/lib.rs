//! Controller detection.
//!
//! Reads evdev device names straight from sysfs, so Den needs no native build
//! dependency. The build plan calls for gilrs plus the community SDL
//! gamecontrollerdb; that is the intended upgrade on systems with libudev-dev
//! installed. This detector keeps the same job working everywhere.

use serde::Serialize;
use std::fs;

/// One detected controller.
#[derive(Debug, Clone, Serialize)]
pub struct ControllerInfo {
    pub id: String,
    pub name: String,
    pub player: Option<usize>,
}

/// The input manager: stateless for now, hot-plug is re-enumerated on demand.
#[derive(Debug, Default)]
pub struct Input;

impl Input {
    pub fn new() -> Self {
        Input
    }

    /// Controllers currently attached.
    pub fn controllers(&self) -> Vec<ControllerInfo> {
        detect()
    }
}

/// Enumerate attached gamepads from `/sys/class/input`.
pub fn detect() -> Vec<ControllerInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/input") else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let name = fs::read_to_string(dir.join("device/name"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if !name.is_empty() && looks_like_gamepad(&name) {
            let id = entry.file_name().to_string_lossy().into_owned();
            out.push(ControllerInfo {
                id,
                name,
                player: None,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn looks_like_gamepad(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "gamepad",
        "joypad",
        "joystick",
        "controller",
        "xbox",
        "dualshock",
        "dualsense",
        "8bitdo",
        "game controller",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_filter() {
        assert!(looks_like_gamepad("Xbox 360 Controller"));
        assert!(looks_like_gamepad("8BitDo SN30 Pro"));
        assert!(!looks_like_gamepad("Logitech USB Keyboard"));
        assert!(!looks_like_gamepad("USB Optical Mouse"));
    }
}
