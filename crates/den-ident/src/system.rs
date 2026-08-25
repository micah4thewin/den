#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum System {
    Nes,
    Snes,
    Genesis,
    SegaCd,
    Sega32x,
    N64,
    Ps1,
    Gb,
    Gbc,
    Gba,
    Arcade,
    Dos,
    Ps2,
    Gamecube,
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

    pub fn from_extension(ext: &str) -> Option<System> {
        let ext = ext.to_ascii_lowercase();
        System::ALL
            .into_iter()
            .find(|system| system.extensions().contains(&ext.as_str()))
    }

    pub fn from_name(name: &str) -> Option<System> {
        System::ALL.into_iter().find(|system| system.name() == name)
    }

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
