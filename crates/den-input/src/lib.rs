//! Controller detection.
//!
//! Reads sysfs directly, so Den needs no native build dependency. The build
//! plan calls for gilrs plus the community SDL gamecontrollerdb; that is the
//! intended upgrade. This detector does the same job with the kernel's own
//! answer rather than a guess.
//!
//! It asks what a device *is*, not what it is called. The previous version
//! matched names against a keyword list, which misses the one every Linux
//! machine has: the kernel calls an Xbox pad `Microsoft X-Box 360 pad`, and
//! `x-box` is not `xbox`. Two better questions are available:
//!
//! 1. **Does it report `BTN_GAMEPAD`?** The capability bitmask in sysfs says
//!    so outright, for `js` and `event` nodes alike, and it is what udev
//!    itself looks at. A joydev node alone is not enough: accelerometers,
//!    wheels and UPSes get one too, and a non-pad that slips through here
//!    steals player one.
//! 2. **Is there a `js` node?** The kernel creates one only for a joystick,
//!    which makes it the natural identity for a pad, and its number gives
//!    the stable order pads are ranked in.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the kernel publishes input devices.
const SYSFS_INPUT: &str = "/sys/class/input";

/// `BTN_GAMEPAD` (`BTN_SOUTH`), the bit that makes a device a gamepad.
const BTN_GAMEPAD: usize = 0x130;

/// One detected controller.
#[derive(Debug, Clone, Serialize)]
pub struct ControllerInfo {
    /// The kernel's node for the device (`js0`), stable while it is plugged
    /// in.
    pub id: String,
    /// A name for this pad that survives unplugging it: vendor, product, and
    /// what it calls itself. Assignments are stored against this, so a pad
    /// that goes away and comes back is still the same player.
    pub identity: String,
    /// The name the pad reports, which is what the interface shows.
    pub name: String,
    /// Which player this pad is assigned to, from 1.
    pub player: Option<usize>,
    /// The index RetroArch is told to use: the pad's rank among attached
    /// pads, dense from 0. RetroArch's udev joypad driver fills slots
    /// 0..n-1 in its own enumeration order and never consults `jsN`
    /// numbers, so a lone pad on `js1` is still its slot 0.
    pub index: Option<usize>,
}

/// The input manager: stateless, hot-plug is re-enumerated on demand.
#[derive(Debug, Clone)]
pub struct Input {
    root: PathBuf,
}

impl Default for Input {
    fn default() -> Self {
        Input::new()
    }
}

impl Input {
    /// A new input manager, reading the machine's own input devices.
    ///
    /// `DEN_SYSFS_INPUT` points it somewhere else, which is how a test walks
    /// a fixture and how somebody debugging can replay a tree they captured
    /// from a machine they do not have.
    pub fn new() -> Self {
        let root = std::env::var_os("DEN_SYSFS_INPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(SYSFS_INPUT));
        Input { root }
    }

    /// An input manager reading a given sysfs root.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Input { root: root.into() }
    }

    /// Controllers currently attached.
    pub fn controllers(&self) -> Vec<ControllerInfo> {
        detect_in(&self.root)
    }
}

/// Enumerate attached gamepads.
pub fn detect() -> Vec<ControllerInfo> {
    Input::new().controllers()
}

/// Enumerate attached gamepads under a given sysfs root.
///
/// Split out so the shape of a real `/sys/class/input` can be built as a
/// fixture and walked in a test. A detector nobody can run without the
/// hardware is a detector nobody checks.
pub fn detect_in(root: &Path) -> Vec<ControllerInfo> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    // Joystick nodes first, so that when one pad is visible as both `js0` and
    // `event7` it is the `js0` that is kept -- that is the node carrying the
    // pad's identity and its rank, and `event7` sorts first alphabetically.
    nodes.sort_by_key(|p| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (!name.starts_with("js"), name)
    });

    // Keyed by the input device each node points at, so the same pad seen
    // through `js0` and `event7` is one pad.
    let mut found: BTreeMap<String, ControllerInfo> = BTreeMap::new();
    let mut seen_identities: BTreeMap<String, usize> = BTreeMap::new();

    for node in &nodes {
        let file_name = node
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_js = file_name.starts_with("js");
        if !reports_gamepad_buttons(node) {
            continue;
        }
        let name = read_trimmed(&node.join("device/name"));
        if name.is_empty() {
            continue;
        }
        let key = device_key(node).unwrap_or_else(|| file_name.clone());
        if found.contains_key(&key) {
            continue;
        }
        let identity = identity_of(node, &name, &mut seen_identities);
        found.insert(
            key,
            ControllerInfo {
                id: file_name.clone(),
                identity,
                name,
                player: None,
                index: is_js.then(|| joystick_number(&file_name)).flatten(),
            },
        );
    }

    // A `js` node is preferred over an `event` node for the same device, and
    // pads come back in joystick order. Their RetroArch index is their rank
    // in that order, dense from 0: the udev joypad driver fills slots
    // 0..n-1 by its own count and never reads `jsN` numbers, so a gap in
    // the js numbering (a Bluetooth pad that came back as `js1`) must not
    // become a gap in the slots.
    let mut pads: Vec<ControllerInfo> = found.into_values().collect();
    pads.sort_by(|a, b| match (a.index, b.index) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.id.cmp(&b.id),
    });
    for (rank, pad) in pads.iter_mut().enumerate() {
        pad.index = Some(rank);
    }
    pads
}

/// The input device a node belongs to, so two views of one pad collapse.
fn device_key(node: &Path) -> Option<String> {
    let device = node.join("device");
    let resolved = fs::canonicalize(&device).unwrap_or(device);
    resolved
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// `js3` -> 3.
fn joystick_number(file_name: &str) -> Option<usize> {
    file_name.strip_prefix("js")?.parse().ok()
}

/// A name for a pad that survives it being unplugged.
///
/// Vendor and product identify the model; the reported name separates two
/// models that share an id. Two of the same pad get a number, in joystick
/// order, so a pair of identical controllers keep their own players.
fn identity_of(node: &Path, name: &str, seen: &mut BTreeMap<String, usize>) -> String {
    let vendor = read_trimmed(&node.join("device/id/vendor"));
    let product = read_trimmed(&node.join("device/id/product"));
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let base = if vendor.is_empty() && product.is_empty() {
        slug
    } else {
        format!("{vendor}:{product}:{slug}")
    };
    let count = seen.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}#{count}")
    }
}

fn read_trimmed(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Whether a device's capability bitmask claims `BTN_GAMEPAD`.
fn reports_gamepad_buttons(node: &Path) -> bool {
    let caps = read_trimmed(&node.join("device/capabilities/key"));
    has_key_bit(&caps, BTN_GAMEPAD)
}

/// Whether `bit` is set in a sysfs capability bitmask.
///
/// The kernel writes these as space-separated hexadecimal words, most
/// significant first, each holding one `long`. The words are not zero-padded,
/// so their width cannot be read off the text -- it is the machine's, which
/// for any userspace matching its kernel is `usize`.
fn has_key_bit(bitmask: &str, bit: usize) -> bool {
    has_key_bit_wide(bitmask, bit, usize::BITS as usize)
}

/// The same, with the word width named, which is what makes it testable on a
/// machine of either size.
fn has_key_bit_wide(bitmask: &str, bit: usize, width: usize) -> bool {
    let words: Vec<&str> = bitmask.split_whitespace().collect();
    let word_from_end = bit / width;
    if words.is_empty() || word_from_end >= words.len() {
        return false;
    }
    let word = words[words.len() - 1 - word_from_end];
    let Ok(value) = u64::from_str_radix(word, 16) else {
        return false;
    };
    value >> (bit % width) & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the shape of a real `/sys/class/input` for one pad.
    #[cfg(unix)]
    fn plant_pad(
        root: &Path,
        input: &str,
        js: Option<&str>,
        name: &str,
        id: (&str, &str),
        key_caps: &str,
    ) {
        let device = root.join(input);
        fs::create_dir_all(device.join("id")).unwrap();
        fs::create_dir_all(device.join("capabilities")).unwrap();
        fs::write(device.join("name"), format!("{name}\n")).unwrap();
        fs::write(device.join("id/vendor"), format!("{}\n", id.0)).unwrap();
        fs::write(device.join("id/product"), format!("{}\n", id.1)).unwrap();
        fs::write(device.join("capabilities/key"), format!("{key_caps}\n")).unwrap();
        if let Some(js) = js {
            let node = root.join(js);
            fs::create_dir_all(&node).unwrap();
            std::os::unix::fs::symlink(&device, node.join("device")).unwrap();
        }
    }

    /// A capability line with BTN_GAMEPAD set, in this machine's word size,
    /// as a real pad on this machine reports it.
    fn gamepad_caps() -> String {
        let width = usize::BITS as usize;
        let mut words = vec!["0".to_string(); 12];
        let index = words.len() - 1 - (BTN_GAMEPAD / width);
        words[index] = format!("{:x}", 1u64 << (BTN_GAMEPAD % width));
        words.join(" ")
    }

    #[test]
    fn a_capability_bit_is_read_from_the_right_end() {
        // Bit 0 is the low bit of the *last* word, whatever the width.
        for width in [32, 64] {
            assert!(has_key_bit_wide("0 0 1", 0, width), "{width}");
            assert!(!has_key_bit_wide("1 0 0", 0, width), "{width}");
            // The first bit of the next word up is the low bit of the word
            // before the last one.
            assert!(has_key_bit_wide("1 0", width, width), "{width}");
            assert!(!has_key_bit_wide("1 0", width - 1, width), "{width}");
            // Off the end of the list is not set, rather than a panic.
            assert!(!has_key_bit_wide("0", 1000, width), "{width}");
        }

        // A real pad's line, in this machine's word size.
        assert!(has_key_bit(&gamepad_caps(), BTN_GAMEPAD));
        assert!(!has_key_bit(&gamepad_caps(), 0x11));

        // The line a keyboard reports has plenty of bits set, none of them
        // this one.
        let keyboard = "10000 0 0 0 0 100 0 0 0 0 0 0";
        assert!(!has_key_bit(keyboard, BTN_GAMEPAD));

        // Nothing, and nonsense, are not gamepads.
        assert!(!has_key_bit("", BTN_GAMEPAD));
        assert!(!has_key_bit("zzzz", 0));
    }

    #[cfg(unix)]
    #[test]
    fn a_pad_the_old_keyword_list_missed_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // The kernel's own name for an Xbox pad. `x-box` is not `xbox`, which
        // is exactly what a keyword list gets wrong.
        plant_pad(
            root,
            "input5",
            Some("js0"),
            "Microsoft X-Box 360 pad",
            ("045e", "028e"),
            &gamepad_caps(),
        );

        let pads = detect_in(root);
        assert_eq!(pads.len(), 1, "{pads:#?}");
        assert_eq!(pads[0].name, "Microsoft X-Box 360 pad");
        assert_eq!(pads[0].id, "js0");
        assert_eq!(pads[0].index, Some(0));
        assert_eq!(pads[0].identity, "045e:028e:microsoft-x-box-360-pad");
    }

    #[cfg(unix)]
    #[test]
    fn a_keyboard_and_a_mouse_are_not_gamepads() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input1",
            None,
            "AT Translated Set 2 keyboard",
            ("0001", "0001"),
            "1 0 0 0 0 0",
        );
        plant_pad(
            root,
            "input2",
            None,
            "Logitech USB Optical Mouse",
            ("046d", "c05a"),
            "0 0 0 0 0 0",
        );
        assert!(detect_in(root).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn one_pad_seen_twice_is_one_pad() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input5",
            Some("js0"),
            "8BitDo SN30 Pro",
            ("2dc8", "6001"),
            &gamepad_caps(),
        );
        // The same device also appears as an event node.
        let event = root.join("event7");
        fs::create_dir_all(&event).unwrap();
        std::os::unix::fs::symlink(root.join("input5"), event.join("device")).unwrap();

        let pads = detect_in(root);
        assert_eq!(pads.len(), 1, "the same pad was reported twice: {pads:#?}");
        assert_eq!(pads[0].id, "js0", "the joystick node is the one to keep");
    }

    #[cfg(unix)]
    #[test]
    fn a_pad_with_no_joystick_node_is_still_found() {
        // Some systems do not load `joydev`; the capability bits still say
        // what the device is.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input9",
            None,
            "Sony Interactive Entertainment Wireless Controller",
            ("054c", "09cc"),
            &gamepad_caps(),
        );
        let event = root.join("event9");
        fs::create_dir_all(&event).unwrap();
        std::os::unix::fs::symlink(root.join("input9"), event.join("device")).unwrap();

        let pads = detect_in(root);
        assert_eq!(pads.len(), 1, "{pads:#?}");
        assert_eq!(
            pads[0].index,
            Some(0),
            "a pad with no joystick node still holds a udev slot"
        );
    }

    #[cfg(unix)]
    #[test]
    fn two_of_the_same_pad_keep_separate_identities() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input5",
            Some("js0"),
            "Microsoft X-Box 360 pad",
            ("045e", "028e"),
            &gamepad_caps(),
        );
        plant_pad(
            root,
            "input6",
            Some("js1"),
            "Microsoft X-Box 360 pad",
            ("045e", "028e"),
            &gamepad_caps(),
        );

        let pads = detect_in(root);
        assert_eq!(pads.len(), 2);
        assert_eq!(pads[0].index, Some(0));
        assert_eq!(pads[1].index, Some(1));
        assert_ne!(
            pads[0].identity, pads[1].identity,
            "two of the same pad would share a player assignment"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pads_come_back_in_joystick_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Planted out of order; js10 must not sort before js2.
        plant_pad(
            root,
            "input7",
            Some("js10"),
            "Pad Ten",
            ("0001", "0010"),
            &gamepad_caps(),
        );
        plant_pad(
            root,
            "input6",
            Some("js2"),
            "Pad Two",
            ("0001", "0002"),
            &gamepad_caps(),
        );
        let pads = detect_in(root);
        assert_eq!(
            pads.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["Pad Two", "Pad Ten"],
            "pads rank in joystick order"
        );
        assert_eq!(
            pads.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![Some(0), Some(1)],
            "RetroArch's udev driver fills slots densely from 0, never by js number"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_lone_pad_on_a_high_joystick_number_is_still_slot_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input5",
            Some("js1"),
            "8BitDo SN30 Pro",
            ("2dc8", "6001"),
            &gamepad_caps(),
        );
        let pads = detect_in(root);
        assert_eq!(pads.len(), 1, "{pads:#?}");
        assert_eq!(
            pads[0].index,
            Some(0),
            "the only attached pad is RetroArch's slot 0 whatever its js number"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_joystick_node_without_gamepad_buttons_is_not_a_pad() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plant_pad(
            root,
            "input3",
            Some("js0"),
            "ST LIS3LV02DL Accelerometer",
            ("0000", "0000"),
            "0 0 0 0 0 0",
        );
        plant_pad(
            root,
            "input5",
            Some("js1"),
            "Microsoft X-Box 360 pad",
            ("045e", "028e"),
            &gamepad_caps(),
        );
        let pads = detect_in(root);
        assert_eq!(pads.len(), 1, "{pads:#?}");
        assert_eq!(pads[0].name, "Microsoft X-Box 360 pad");
        assert_eq!(
            pads[0].index,
            Some(0),
            "a non-pad joydev device must not steal player one or shift the slots"
        );
    }

    #[test]
    fn detect_is_quiet_where_there_is_no_sysfs() {
        assert!(detect_in(Path::new("/definitely/not/a/sysfs")).is_empty());
    }
}
