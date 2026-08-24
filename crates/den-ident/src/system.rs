/// The systems Den shelves games under, with the core the runner will pick
/// by default. Names are the words the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum System {
    /// Nintendo Entertainment System / Famicom.
    Nes,
    /// Super Nintendo Entertainment System / Super Famicom.
    Snes,
    /// Sega Genesis / Mega Drive.
    Genesis,
    /// Sega CD / Mega CD.
    SegaCd,
    /// Sega 32X.
    Sega32x,
    /// Nintendo 64.
    N64,
    /// Sony PlayStation.
    Ps1,
    /// Game Boy.
    Gb,
    /// Game Boy Color.
    Gbc,
    /// Game Boy Advance.
    Gba,
    /// Arcade sets (FinalBurn Neo and friends), kept zipped.
    Arcade,
    /// MS-DOS, run through DOSBox.
    Dos,
    /// Sony PlayStation 2. Needs an external emulator profile.
    Ps2,
    /// Nintendo GameCube. Needs an external emulator profile.
    Gamecube,
    /// Nintendo Wii. Needs an external emulator profile.
    Wii,
}

impl System {
    const ALL: [System; 15] = [
        System::Nes,
        System::Snes,
        System::Genesis,
        System::SegaCd,
        System::Sega32x,
        System::N64,
        System::Ps1,
        System::Gb,
        System::Gbc,
        System::Gba,
        System::Arcade,
        System::Dos,
        System::Ps2,
        System::Gamecube,
        System::Wii,
    ];

    /// The human word for this system.
    pub fn name(self) -> &'static str {
        match self {
            System::Nes => "NES",
            System::Snes => "SNES",
            System::Genesis => "Genesis",
            System::SegaCd => "Sega CD",
            System::Sega32x => "Sega 32X",
            System::N64 => "N64",
            System::Ps1 => "PlayStation",
            System::Gb => "GB",
            System::Gbc => "GBC",
            System::Gba => "GBA",
            System::Arcade => "Arcade",
            System::Dos => "DOS",
            System::Ps2 => "PlayStation 2",
            System::Gamecube => "GameCube",
            System::Wii => "Wii",
        }
    }

    /// File extensions that most commonly carry this system's games.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            System::Nes => &["nes", "fds"],
            System::Snes => &["sfc", "smc", "fig"],
            System::Genesis => &["md", "gen", "smd"],
            System::SegaCd => &["bin", "cue", "chd"],
            System::Sega32x => &["32x", "bin", "md"],
            System::N64 => &["z64", "n64", "v64"],
            System::Ps1 => &["bin", "cue", "chd", "iso", "pbp"],
            System::Gb => &["gb"],
            System::Gbc => &["gbc"],
            System::Gba => &["gba"],
            System::Arcade => &["zip"],
            System::Dos => &["exe", "com", "bat", "conf"],
            System::Ps2 => &["iso", "chd", "bin", "cue"],
            System::Gamecube => &["iso", "gcm", "rvz", "nkit"],
            System::Wii => &["iso", "wbfs", "rvz", "nkit"],
        }
    }

    /// Resolve an extension (without the dot, lower-cased) to a system.
    pub fn from_extension(ext: &str) -> Option<System> {
        let ext = ext.to_ascii_lowercase();
        System::ALL
            .into_iter()
            .find(|system| system.extensions().contains(&ext.as_str()))
    }

    /// Resolve the exact interface name (`name()`'s output) back to a system.
    pub fn from_name(name: &str) -> Option<System> {
        System::ALL.into_iter().find(|system| system.name() == name)
    }

    /// The libretro core Den prefers for this system: the accuracy-per-watt
    /// picks from the build plan so one library behaves on a Pi and a desktop.
    pub fn default_core(self) -> &'static str {
        match self {
            System::Nes => "mesen",
            System::Snes => "snes9x",
            System::Genesis | System::SegaCd => "genesis_plus_gx",
            System::Sega32x => "picodrive",
            System::N64 => "mupen64plus_next",
            System::Ps1 => "swanstation",
            System::Gb | System::Gbc => "gambatte",
            System::Gba => "mgba",
            System::Arcade => "fbneo",
            System::Dos => "dosbox_pure",
            System::Ps2 => "pcsx2",
            System::Gamecube | System::Wii => "dolphin",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_inverts_name() {
        for system in System::ALL {
            assert_eq!(System::from_name(system.name()), Some(system));
        }
        assert_eq!(System::from_name("Atari"), None);
    }
}
