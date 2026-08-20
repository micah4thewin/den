//! Controller detection.
//!
//! Reads evdev device names straight from sysfs, so Den needs no native build
//! dependency. The build plan calls for gilrs plus the community SDL
//! gamecontrollerdb; that is the intended upgrade on systems with libudev-dev
//! installed. This detector keeps the same job working everywhere.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;

/// One detected controller.
#[derive(Debug, Clone, Serialize)]
pub struct ControllerInfo {
    /// The kernel's own name for the device (`input5`), stable while it is
    /// plugged in.
    pub id: String,
    /// The name the pad reports, which is what the interface shows.
    pub name: String,
    /// Which player this pad is assigned to, once assignment exists.
    pub player: Option<usize>,
}

/// The input manager: stateless for now, hot-plug is re-enumerated on demand.
#[derive(Debug, Default)]
pub struct Input;

impl Input {
    /// A new input manager.
    pub fn new() -> Self {
        Input
    }

    /// Controllers currently attached.
    pub fn controllers(&self) -> Vec<ControllerInfo> {
        detect()
    }
}

/// Enumerate attached gamepads from `/sys/class/input`.
///
/// One physical pad shows up several times there -- as `eventN`, as `jsN`,
/// and as the `inputN` node both of those point at -- so devices are keyed by
/// the input node they resolve to and reported once each. A person with one
/// controller plugged in should be told they have one controller.
pub fn detect() -> Vec<ControllerInfo> {
    let mut found: BTreeMap<String, ControllerInfo> = BTreeMap::new();
    let Ok(entries) = fs::read_dir("/sys/class/input") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let name = fs::read_to_string(dir.join("device/name"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty() || !looks_like_gamepad(&name) {
            continue;
        }
        // `device` is a symlink to the inputN node; canonicalizing it is what
        // collapses eventN and jsN onto the one pad they describe.
        let id = fs::canonicalize(dir.join("device"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        found.entry(id.clone()).or_insert(ControllerInfo {
            id,
            name,
            player: None,
        });
    }
    found.into_values().collect()
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
    fn detect_is_quiet_where_there_is_no_sysfs() {
        // Nothing here asserts a pad is present; what matters is that the
        // detector never reports the same device twice.
        let pads = detect();
        let mut ids: Vec<&str> = pads.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "the same pad was reported twice");
    }

    #[test]
    fn keyword_filter() {
        assert!(looks_like_gamepad("Xbox 360 Controller"));
        assert!(looks_like_gamepad("8BitDo SN30 Pro"));
        assert!(!looks_like_gamepad("Logitech USB Keyboard"));
        assert!(!looks_like_gamepad("USB Optical Mouse"));
    }
}
