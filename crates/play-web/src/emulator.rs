//! The emulator the phone runs, served by the machine the shelf lives on.
//!
//! "Play on this phone" runs the game in the phone's own browser: EmulatorJS,
//! which is RetroArch's libretro cores built for WebAssembly. Nothing has to
//! be installed on the phone or set up on the machine. The first time a phone
//! asks for a file, Play downloads the package it lives in from the npm
//! registry — the loader once, each core the first time a system is played —
//! checks it against the SHA-512 written below, and keeps it beside the
//! library. Every later request is a file off the disk, so a phone on the
//! tailnet never needs to reach anything but the machine.
//!
//! The version and every package's hash are pinned. A tarball that does not
//! match is thrown away and never served.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha512};

pub const VERSION: &str = "4.2.3";

const REGISTRY: &str = "https://registry.npmjs.org/@emulatorjs";

/// The biggest package Play fetches (fbneo) is about 33 MB unpacked; nothing
/// it should be downloading is anywhere near this.
const MOST_BYTES: u64 = 128 * 1024 * 1024;

/// What `npm view @emulatorjs/<package>@4.2.3 dist.integrity` says, in hex.
const PACKAGES: &[(&str, &str)] = &[
    (
        "emulatorjs",
        "ef3dea680e0bc32bab86e1af74c50317dc49a446f10b748dcbdf84f6d49a3ac4\
         68f05092d905da9bfd24fe573d92a1d64452252ca59a84d8cf012b722f4ac59f",
    ),
    (
        "core-fceumm",
        "5d7f55bf637f873a744ecb4d3024d2a6912cfac83ed65a49a4f7520ee46a20ef\
         dcc1db7b754705f968cd5f5c9024b45b3f9f97c1c34767a2b804acbc089a6775",
    ),
    (
        "core-nestopia",
        "165cf4f9279ba67e6c82326f821850c9d67938cf897f88f06be13ab46d53b991\
         90e3ee6c61e245077140e731c021766e1414bdd5c5a721745ba4e51e9e77fc17",
    ),
    (
        "core-snes9x",
        "2537bd46ff5e3ae0a48241b820bc2e09952a35c548de6f63bb53474d38eb89d5\
         25160252a8be0352a17fbcb28df68942a6caa52457dc09ac803a88a427179fc0",
    ),
    (
        "core-genesis_plus_gx",
        "0f57b65c0d823d10237ebd09baba5726047d75e3f197fc47b5a13cbf54fd06a0\
         fd0370cb7e86c8156b1653d8c2152e0d7d4c42bdc2dd4d8758fd67b7433bdc4f",
    ),
    (
        "core-picodrive",
        "154c5e1de24820a8b85c6f9474ea020110e31fb855d755cb6fa2c550eeb8e05e\
         d18b4f2f786a8c42438902808ff2e5c6c5afc97604bb497a9cd1ae34faf2efdb",
    ),
    (
        "core-smsplus",
        "f771b6a58f47255979a489a82106c386db305645fd68882bae285d193147aaea\
         e18ac60797be170a9ce60a8333741e54331d3d9c55b5d92fa7f9b2d322e88bac",
    ),
    (
        "core-mupen64plus_next",
        "1e79c45db384a58c43edfdb050940bc56529101d1b4f6902e7abb9b3b3cbca3f\
         f1348e5c9102a46b9f9acb54215367cebf606e3d1c238e19739952669397b787",
    ),
    (
        "core-parallel_n64",
        "95e443333277e8774bc89ee0a6fa1c693da003c48788f00b84456547cb473a98\
         95dafa61f613e93421a68892f609b3f6788e435ba0353d8d7316e2176c99c427",
    ),
    (
        "core-pcsx_rearmed",
        "98c6e5fcdb330abc85235e57e509e23aa2cd2fb60667ab4b3f6e20af9cb5ab67\
         a414e21bb5cdfd05c55330313046774d1a8be1da73e809c32307e5b77e0402a2",
    ),
    (
        "core-mednafen_psx_hw",
        "4b336b99aa8ca819277ffd214ef3e0de920009078b6fa3d549f6892a3608e22e\
         aed26a40860b085b83c988b3879f945f2ed4e9cbe78859026f0b50d0180a5f79",
    ),
    (
        "core-gambatte",
        "3aadc024bf522a732c3409ad00fd9e37ddf84d0b6cdcd1de6807b4cf504ef4bc\
         7f2f7c4364a2e9c4bf97c207125c8ae5e12c75a17c58114c03ec98cf110d031a",
    ),
    (
        "core-mgba",
        "75a887cd940a12bf8ff5fadaee3e58a040178b261412d061150f04015fd2782b\
         6b92faad6b253b190f4b620a1283390a4b0b1b352b24029a86b80f441804583c",
    ),
    (
        "core-fbneo",
        "e78631f0a72af031f8d9c6936a4c2cde931465ce9490136ab59820ed0d3894d9\
         9659cfd6d303c6374807c46f74c6694e1461c7b71a7d42789c05bd4d5cc8c65a",
    ),
    (
        "core-fbalpha2012_cps1",
        "19e0045ff989716b2470c9c46753a38de7dfb4bdafce7984f8b9acd7d5f24514\
         b07c9db7ff073ca759adecff3640247f681d3ddb9824b2326a397d9606f295e8",
    ),
    (
        "core-fbalpha2012_cps2",
        "46a98b77427f1a1f1d9328e7a4637b6062114b57c98ed08aa81b3caf9e14cf7e\
         4a987c70e10b3f82109cf37e834573ff22b2185fcef58ccc55bec38839d081fe",
    ),
];

/// One download at a time: two phones asking for the same core at once
/// should fetch it once, not race to write the same directory.
static FETCHING: Mutex<()> = Mutex::new(());

/// Where under the emulator's data folder a request lands: which package
/// holds it, and its path inside that package's tarball.
#[derive(Debug, PartialEq, Eq)]
pub struct Located {
    pub package: &'static str,
    pub inner: String,
}

fn pinned(package: &str) -> Option<(&'static str, &'static str)> {
    PACKAGES.iter().copied().find(|(name, _)| *name == package)
}

/// `cores/mupen64plus_next-wasm.data` → the core package, and everything else
/// under the data folder → the loader's package. Anything that tries to step
/// out of the folder, or names a core Play does not know, is nothing.
pub fn locate(path: &str) -> Option<Located> {
    let clean = !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !clean {
        return None;
    }
    if let Some(rest) = path.strip_prefix("cores/") {
        let file = rest.strip_prefix("reports/").unwrap_or(rest);
        let core = file.split(['-', '.']).next().unwrap_or_default();
        let (package, _) = pinned(&format!("core-{core}"))?;
        return Some(Located {
            package,
            inner: format!("package/{rest}"),
        });
    }
    Some(Located {
        package: "emulatorjs",
        inner: format!("package/data/{path}"),
    })
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The scripts the loader asks for one by one when the bundled
/// `emulator.min.js` is missing, in the order it asks. The npm package ships
/// them unbundled, so Play joins them the way the minified build would.
const SCRIPTS: &[&str] = &[
    "emulator.js",
    "nipplejs.js",
    "shaders.js",
    "storage.js",
    "gamepad.js",
    "GameManager.js",
    "socket.io.min.js",
    "compression.js",
];

/// The file for `path`, fetching its package first if this machine has never
/// needed it. Blocking: call it off the async runtime.
pub fn file(cache: &Path, path: &str) -> Result<PathBuf, String> {
    let found = locate(path).ok_or("There is no such file in the emulator.")?;
    let dir = unpacked(cache, found.package)?;
    let file = dir.join(&found.inner);
    if !file.is_file() && found.package == "emulatorjs" {
        bundle(&dir.join("package/data"), path)?;
    }
    if file.is_file() {
        Ok(file)
    } else {
        Err("There is no such file in the emulator.".to_string())
    }
}

/// Writes `emulator.min.js` or `emulator.min.css` from the sources beside
/// it. Without them the loader still works, but only by switching itself
/// into a debug mode that also turns off the phone's copy of each core.
fn bundle(data: &Path, path: &str) -> Result<(), String> {
    let joined = match path {
        "emulator.min.js" => {
            let mut all = String::new();
            for script in SCRIPTS {
                let text = fs::read_to_string(data.join("src").join(script))
                    .map_err(|e| format!("the emulator is missing {script}: {e}"))?;
                all.push_str(&text);
                all.push_str("\n;\n");
            }
            all
        }
        "emulator.min.css" => fs::read_to_string(data.join("emulator.css"))
            .map_err(|e| format!("the emulator is missing its stylesheet: {e}"))?,
        _ => return Ok(()),
    };
    let target = data.join(path);
    let staging = data.join(format!("{path}.part"));
    fs::write(&staging, joined)
        .and_then(|()| fs::rename(&staging, &target))
        .map_err(|e| e.to_string())
}

/// The package's directory, downloading and unpacking it if it is not here.
fn unpacked(cache: &Path, package: &'static str) -> Result<PathBuf, String> {
    let dir = cache.join(VERSION).join(package);
    let done = dir.join(".complete");
    if done.is_file() {
        return Ok(dir);
    }
    let _one_at_a_time = FETCHING.lock().map_err(|_| "the emulator is busy")?;
    if done.is_file() {
        return Ok(dir);
    }
    let (_, sha512) = pinned(package).ok_or("Play does not know that emulator package.")?;
    let url = format!("{REGISTRY}/{package}/-/{package}-{VERSION}.tgz");
    log::info!("fetching the phone's emulator: {url}");
    let tarball = download(&url).map_err(|e| {
        format!("Play could not download the emulator for the phone ({e}). The machine needs the internet once for each system; after that it keeps a copy.")
    })?;
    let got = hex(&Sha512::digest(&tarball));
    if got != sha512 {
        return Err(format!(
            "The emulator Play downloaded ({package} {VERSION}) is not the one it expected, so it was not used."
        ));
    }
    unpack(&tarball, &dir).map_err(|e| format!("Play could not unpack the emulator: {e}"))?;
    fs::write(&done, VERSION).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    // The operating system's own trust store, so a machine behind a proxy
    // that its owner already trusts can still fetch.
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls).build().into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MOST_BYTES {
        return Err("the download was far larger than any emulator".to_string());
    }
    Ok(bytes)
}

/// Unpacked beside the final directory and moved into place whole, so a
/// half-written package is never mistaken for a finished one.
pub fn unpack(tarball: &[u8], dir: &Path) -> std::io::Result<()> {
    let parent = dir.parent().unwrap_or(dir);
    fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    // `unpack_in` refuses any entry that would land outside the directory.
    for entry in archive.entries()? {
        entry?.unpack_in(staging.path())?;
    }
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::rename(staging.keep(), dir)?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_is_a_whole_sha512() {
        for (name, sha) in PACKAGES {
            assert_eq!(sha.len(), 128, "{name}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
        }
    }

    #[test]
    fn a_core_file_comes_from_its_core_package() {
        assert_eq!(
            locate("cores/mupen64plus_next-wasm.data"),
            Some(Located {
                package: "core-mupen64plus_next",
                inner: "package/mupen64plus_next-wasm.data".into(),
            })
        );
        assert_eq!(
            locate("cores/reports/mupen64plus_next.json"),
            Some(Located {
                package: "core-mupen64plus_next",
                inner: "package/reports/mupen64plus_next.json".into(),
            })
        );
        assert_eq!(
            locate("cores/genesis_plus_gx-legacy-wasm.data").map(|l| l.package),
            Some("core-genesis_plus_gx")
        );
    }

    #[test]
    fn everything_else_comes_from_the_loader_package() {
        assert_eq!(
            locate("src/emulator.js"),
            Some(Located {
                package: "emulatorjs",
                inner: "package/data/src/emulator.js".into(),
            })
        );
        assert_eq!(locate("loader.js").map(|l| l.package), Some("emulatorjs"));
    }

    #[test]
    fn nothing_outside_the_folder_and_no_unknown_core() {
        assert_eq!(locate("../library.db"), None);
        assert_eq!(locate("src/../../x"), None);
        assert_eq!(locate("/etc/passwd"), None);
        assert_eq!(locate("cores/ppsspp-wasm.data"), None);
        assert_eq!(locate(""), None);
        assert_eq!(locate("src/emu%2e.js"), None);
    }

    #[test]
    fn a_package_already_here_is_served_without_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(VERSION).join("emulatorjs");
        fs::create_dir_all(dir.join("package/data")).unwrap();
        fs::write(dir.join("package/data/loader.js"), "// loader").unwrap();
        fs::write(dir.join(".complete"), VERSION).unwrap();
        let found = file(tmp.path(), "loader.js").unwrap();
        assert_eq!(fs::read_to_string(found).unwrap(), "// loader");
        assert!(file(tmp.path(), "missing.js").is_err());
    }

    #[test]
    fn the_loader_gets_one_script_made_from_the_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(VERSION).join("emulatorjs");
        let data = dir.join("package/data");
        fs::create_dir_all(data.join("src")).unwrap();
        for script in SCRIPTS {
            fs::write(data.join("src").join(script), format!("// {script}")).unwrap();
        }
        fs::write(data.join("emulator.css"), "body{}").unwrap();
        fs::write(dir.join(".complete"), VERSION).unwrap();

        let js = fs::read_to_string(file(tmp.path(), "emulator.min.js").unwrap()).unwrap();
        assert!(js.starts_with("// emulator.js"), "{js}");
        assert!(js.contains("// compression.js"), "{js}");
        let css = fs::read_to_string(file(tmp.path(), "emulator.min.css").unwrap()).unwrap();
        assert_eq!(css, "body{}");
    }

    #[test]
    fn a_tarball_unpacks_whole_into_place() {
        let mut tarball = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::fast());
            let mut builder = tar::Builder::new(gz);
            let body = b"hello";
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "package/data/hello.txt", &body[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("pkg");
        unpack(&tarball, &dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("package/data/hello.txt")).unwrap(),
            "hello"
        );
    }
}
