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

    /// The core Play reaches for first.
    pub fn default_core(self) -> &'static str {
        self.cores()[0]
    }

    /// Every core that runs this system, best first. Play launches with the
    /// first one present and downloads the first one it can get, so the later
    /// entries are the ones a distribution packages or a smaller machine
    /// (a Raspberry Pi) has builds of when the first does not.
    ///
    /// PlayStation leads with `pcsx_rearmed` because it boots without a BIOS
    /// file; a fresh machine has none, and Play does not ship any.
    pub fn cores(self) -> &'static [&'static str] {
        match self {
            System::Nes => &["mesen", "nestopia", "fceumm"],
            System::Snes => &["snes9x", "snes9x2010"],
            System::Genesis | System::SegaCd => &["genesis_plus_gx", "picodrive"],
            System::Sega32x => &["picodrive"],
            System::N64 => &["mupen64plus_next", "parallel_n64"],
            System::Ps1 => &["pcsx_rearmed", "swanstation", "mednafen_psx"],
            System::Gb | System::Gbc => &["gambatte", "mgba", "sameboy"],
            System::Gba => &["mgba", "vba_next", "gpsp"],
            System::Arcade => &["fbneo", "mame2003_plus"],
            System::Dos => &["dosbox_pure"],
            System::Ps2 => &["pcsx2"],
            System::Gamecube | System::Wii => &["dolphin"],
        }
    }

    /// The systems a fresh install gets ready before anything is shelved:
    /// the ones most downloads turn out to be.
    pub const COMMON: [System; 8] = [
        System::Nes,
        System::Snes,
        System::Genesis,
        System::Gb,
        System::Gba,
        System::N64,
        System::Ps1,
        System::Arcade,
    ];
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

    #[test]
    fn every_system_has_a_core_and_none_repeats() {
        for system in System::ALL {
            let cores = system.cores();
            assert!(!cores.is_empty(), "{system:?}");
            assert_eq!(system.default_core(), cores[0]);
            let mut seen = cores.to_vec();
            seen.dedup();
            assert_eq!(seen.len(), cores.len(), "{system:?}");
        }
    }
}
