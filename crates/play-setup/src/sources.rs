//! Where RetroArch and its cores come from, per machine.
//!
//! Everything comes from libretro's own buildbot — the same place RetroArch's
//! Online Updater fetches from — so what Play downloads is what RetroArch
//! would have downloaded for itself. `PLAY_BUILDBOT` points the whole thing at
//! a mirror.

/// The RetroArch release Play fetches when the machine has none.
pub const RETROARCH_VERSION: &str = "1.20.0";

const BUILDBOT: &str = "https://buildbot.libretro.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    LinuxX64,
    LinuxArm64,
    LinuxArmhf,
    WindowsX64,
    MacX64,
    MacArm64,
}

/// How a downloaded RetroArch is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// A `.7z` holding an AppImage, which is unpacked so it runs without FUSE.
    SevenZipAppImage,
    /// A `.7z` holding a portable directory with the program in it.
    SevenZipDirectory,
    /// A disk image holding `RetroArch.app`.
    DiskImage,
}

impl Target {
    pub fn current() -> Option<Target> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(Target::LinuxX64),
            ("linux", "aarch64") => Some(Target::LinuxArm64),
            ("linux", "arm") => Some(Target::LinuxArmhf),
            ("windows", "x86_64") => Some(Target::WindowsX64),
            ("macos", "x86_64") => Some(Target::MacX64),
            ("macos", "aarch64") => Some(Target::MacArm64),
            _ => None,
        }
    }

    pub fn core_extension(self) -> &'static str {
        match self {
            Target::WindowsX64 => "dll",
            Target::MacX64 | Target::MacArm64 => "dylib",
            _ => "so",
        }
    }

    /// The buildbot directories that carry this machine's cores, tried in
    /// order. ARM Linux has more than one name on the buildbot over the years,
    /// so both are asked before giving up.
    fn core_dirs(self) -> &'static [&'static str] {
        match self {
            Target::LinuxX64 => &["nightly/linux/x86_64/latest"],
            Target::LinuxArm64 => &["nightly/linux/aarch64/latest", "nightly/linux/arm64/latest"],
            Target::LinuxArmhf => &[
                "nightly/linux/armhf/latest",
                "nightly/linux/armv7-neon-hf/latest",
            ],
            Target::WindowsX64 => &["nightly/windows/x86_64/latest"],
            Target::MacX64 => &["nightly/apple/osx/x86_64/latest"],
            Target::MacArm64 => &["nightly/apple/osx/arm64/latest"],
        }
    }

    /// Where a core's zip may be, best guess first.
    pub fn core_urls(self, core: &str) -> Vec<String> {
        let base = base();
        let ext = self.core_extension();
        self.core_dirs()
            .iter()
            .map(|dir| format!("{base}/{dir}/{core}_libretro.{ext}.zip"))
            .collect()
    }

    /// Where a portable RetroArch for this machine is, if libretro publishes
    /// one. ARM Linux has none; there the installer brings the distribution's
    /// RetroArch in instead (see the `.deb`'s recommends).
    pub fn retroarch(self) -> Option<(String, Packing)> {
        let base = base();
        let v = RETROARCH_VERSION;
        match self {
            Target::LinuxX64 => Some((
                format!("{base}/stable/{v}/linux/x86_64/RetroArch.7z"),
                Packing::SevenZipAppImage,
            )),
            Target::WindowsX64 => Some((
                format!("{base}/stable/{v}/windows/x86_64/RetroArch.7z"),
                Packing::SevenZipDirectory,
            )),
            Target::MacX64 | Target::MacArm64 => Some((
                format!("{base}/stable/{v}/apple/osx/universal/RetroArch_Metal.dmg"),
                Packing::DiskImage,
            )),
            Target::LinuxArm64 | Target::LinuxArmhf => None,
        }
    }
}

fn base() -> String {
    std::env::var("PLAY_BUILDBOT")
        .ok()
        .filter(|b| !b.trim().is_empty())
        .map(|b| b.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| BUILDBOT.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_core_url_names_the_file_the_buildbot_publishes() {
        let urls = Target::LinuxX64.core_urls("mupen64plus_next");
        assert_eq!(
            urls,
            vec!["https://buildbot.libretro.com/nightly/linux/x86_64/latest/mupen64plus_next_libretro.so.zip"]
        );
        let urls = Target::WindowsX64.core_urls("snes9x");
        assert!(urls[0].ends_with("/windows/x86_64/latest/snes9x_libretro.dll.zip"));
        let urls = Target::MacArm64.core_urls("mgba");
        assert!(urls[0].ends_with("/apple/osx/arm64/latest/mgba_libretro.dylib.zip"));
    }

    #[test]
    fn a_raspberry_pi_asks_more_than_one_place_for_its_cores() {
        assert!(Target::LinuxArm64.core_urls("mgba").len() > 1);
        assert!(Target::LinuxArm64.retroarch().is_none());
    }

    #[test]
    fn the_machines_with_a_published_retroarch_get_one() {
        for target in [Target::LinuxX64, Target::WindowsX64, Target::MacArm64] {
            let (url, _) = target.retroarch().expect("a download");
            assert!(url.contains(RETROARCH_VERSION), "{url}");
        }
    }
}
