use crate::sources::{Packing, Target};
use crate::SetupError;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Nothing Play downloads is anywhere near this; a response that is has gone
/// wrong somewhere, and the disk should not find out how wrong.
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

/// Fetch `url` into `dest`, through a `.part` file so a cut-off download is
/// never mistaken for a finished one.
pub(crate) fn download(url: &str, dest: &Path) -> Result<(), SetupError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("Play/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => SetupError::NotPublished(url.to_string(), code),
        other => SetupError::Network(url.to_string(), other.to_string()),
    })?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let part = with_suffix(dest, ".part");
    let mut file = fs::File::create(&part)?;
    let mut body = response.into_reader().take(MAX_DOWNLOAD + 1);
    let written = io::copy(&mut body, &mut file)
        .map_err(|e| SetupError::Network(url.to_string(), e.to_string()))?;
    drop(file);
    if written > MAX_DOWNLOAD {
        let _ = fs::remove_file(&part);
        return Err(SetupError::Unusable(format!(
            "{url} is larger than any emulator"
        )));
    }
    fs::rename(&part, dest)?;
    Ok(())
}

/// Download one core and put its library in `cores_dir`.
pub(crate) fn install_core(
    target: Target,
    core: &str,
    cores_dir: &Path,
) -> Result<PathBuf, SetupError> {
    let file_name = format!("{core}_libretro.{}", target.core_extension());
    let scratch = cores_dir.join(".download");
    fs::create_dir_all(&scratch)?;
    let zip_path = scratch.join(format!("{file_name}.zip"));

    let mut last = None;
    for url in target.core_urls(core) {
        match download(&url, &zip_path) {
            Ok(()) => {
                last = None;
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    if let Some(e) = last {
        let _ = fs::remove_dir_all(&scratch);
        return Err(e);
    }

    let result = unzip_one(&zip_path, &file_name, cores_dir);
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn unzip_one(zip_path: &Path, file_name: &str, into: &Path) -> Result<PathBuf, SetupError> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?)
        .map_err(|e| SetupError::Unusable(format!("{} is not a zip: {e}", zip_path.display())))?;
    let index = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|entry| entry.name().rsplit('/').next() == Some(file_name))
                .unwrap_or(false)
        })
        .ok_or_else(|| SetupError::Unusable(format!("the download did not hold {file_name}")))?;
    let mut entry = archive
        .by_index(index)
        .map_err(|e| SetupError::Unusable(e.to_string()))?;
    let dest = into.join(file_name);
    let part = with_suffix(&dest, ".part");
    let mut out = fs::File::create(&part)?;
    io::copy(&mut entry, &mut out)?;
    drop(out);
    fs::rename(&part, &dest)?;
    Ok(dest)
}

/// Download a portable RetroArch into `managed`, laid out where the runner's
/// search already looks: `managed/retroarch/retroarch[.exe]`, or
/// `managed/RetroArch.app` on a Mac.
pub(crate) fn install_retroarch(target: Target, managed: &Path) -> Result<PathBuf, SetupError> {
    let (url, packing) = target
        .retroarch()
        .ok_or(SetupError::NoRetroArchForThisMachine)?;
    let scratch = managed.join(".download");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch)?;
    let archive = scratch.join(url.rsplit('/').next().unwrap_or("RetroArch"));
    download(&url, &archive)?;

    let result = match packing {
        Packing::SevenZipAppImage => from_appimage_7z(&archive, &scratch, managed),
        Packing::SevenZipDirectory => from_directory_7z(&archive, &scratch, managed),
        Packing::DiskImage => from_disk_image(&archive, &scratch, managed),
    };
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn unpack_7z(archive: &Path, into: &Path) -> Result<(), SetupError> {
    sevenz_rust::decompress_file(archive, into)
        .map_err(|e| SetupError::Unusable(format!("could not unpack {}: {e}", archive.display())))
}

/// An AppImage runs through FUSE, which a fresh Ubuntu does not have. It also
/// unpacks itself without FUSE, so Play unpacks it once and runs what is
/// inside through a small launcher.
fn from_appimage_7z(archive: &Path, scratch: &Path, managed: &Path) -> Result<PathBuf, SetupError> {
    let unpacked = scratch.join("unpacked");
    unpack_7z(archive, &unpacked)?;
    let appimage = find(&unpacked, &|p| {
        p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("AppImage")
    })
    .ok_or_else(|| SetupError::Unusable("the RetroArch download held no AppImage".into()))?;
    make_executable(&appimage)?;

    let staged = scratch.join("retroarch");
    fs::create_dir_all(&staged)?;
    let status = Command::new(&appimage)
        .arg("--appimage-extract")
        .current_dir(&staged)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() || !staged.join("squashfs-root").join("AppRun").exists() {
        return Err(SetupError::Unusable(
            "the RetroArch AppImage would not unpack".into(),
        ));
    }

    // libretro ships the AppImage's assets and controller profiles beside it,
    // in the home directory the AppImage would give itself when run in place.
    let mut home = appimage.clone().into_os_string();
    home.push(".home");
    let home = PathBuf::from(home);
    if home.is_dir() {
        fs::rename(&home, staged.join("home"))?;
    }

    let launcher = staged.join("retroarch");
    fs::write(
        &launcher,
        "#!/bin/sh\n\
         # Written by Play: runs the RetroArch it downloaded, unpacked so it needs no FUSE.\n\
         here=\"$(dirname \"$(readlink -f \"$0\")\")\"\n\
         if [ -d \"$here/home/.config\" ]; then\n\
         \texport XDG_CONFIG_HOME=\"$here/home/.config\"\n\
         fi\n\
         exec \"$here/squashfs-root/AppRun\" \"$@\"\n",
    )?;
    make_executable(&launcher)?;
    put_in_place(&staged, &managed.join("retroarch"))?;
    Ok(managed.join("retroarch").join("retroarch"))
}

fn from_directory_7z(
    archive: &Path,
    scratch: &Path,
    managed: &Path,
) -> Result<PathBuf, SetupError> {
    let unpacked = scratch.join("unpacked");
    unpack_7z(archive, &unpacked)?;
    let exe = find(&unpacked, &|p| {
        p.is_file()
            && p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("retroarch.exe"))
                .unwrap_or(false)
    })
    .ok_or_else(|| SetupError::Unusable("the RetroArch download held no retroarch.exe".into()))?;
    let dir = exe.parent().unwrap_or(&unpacked).to_path_buf();
    put_in_place(&dir, &managed.join("retroarch"))?;
    Ok(managed.join("retroarch").join("retroarch.exe"))
}

/// macOS mounts a disk image with the `hdiutil` every Mac has, and copies the
/// app out with `ditto`, which keeps its signature intact.
fn from_disk_image(image: &Path, scratch: &Path, managed: &Path) -> Result<PathBuf, SetupError> {
    let mount = scratch.join("mount");
    fs::create_dir_all(&mount)?;
    let attached = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(image)
        .stdout(std::process::Stdio::null())
        .status()?;
    if !attached.success() {
        return Err(SetupError::Unusable(
            "the RetroArch disk image would not open".into(),
        ));
    }
    let result = (|| {
        let app = fs::read_dir(&mount)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
            .ok_or_else(|| SetupError::Unusable("the disk image held no app".into()))?;
        let staged = scratch.join("RetroArch.app");
        let copied = Command::new("ditto").arg(&app).arg(&staged).status()?;
        if !copied.success() {
            return Err(SetupError::Unusable(
                "could not copy RetroArch out of its disk image".into(),
            ));
        }
        put_in_place(&staged, &managed.join("RetroArch.app"))?;
        Ok(managed.join("RetroArch.app"))
    })();
    let _ = Command::new("hdiutil")
        .arg("detach")
        .arg(&mount)
        .stdout(std::process::Stdio::null())
        .status();
    result
}

/// Swap a finished directory in for whatever was there.
fn put_in_place(staged: &Path, dest: &Path) -> Result<(), SetupError> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(staged, dest)?;
    Ok(())
}

fn find(root: &Path, wanted: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if wanted(&path) {
                return Some(path);
            }
            if path.is_dir() {
                queue.push(path);
            }
        }
    }
    None
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_core_comes_out_of_its_zip_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("mgba_libretro.so.zip");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("readme.txt", options).unwrap();
            zip.write_all(b"not this").unwrap();
            zip.start_file("mgba_libretro.so", options).unwrap();
            zip.write_all(b"the core").unwrap();
            zip.finish().unwrap();
        }
        let cores = dir.path().join("cores");
        fs::create_dir_all(&cores).unwrap();
        let got = unzip_one(&zip_path, "mgba_libretro.so", &cores).unwrap();
        assert_eq!(got, cores.join("mgba_libretro.so"));
        assert_eq!(fs::read(&got).unwrap(), b"the core");
        assert!(!cores.join("mgba_libretro.so.part").exists());

        let err = unzip_one(&zip_path, "snes9x_libretro.so", &cores).unwrap_err();
        assert!(err.to_string().contains("snes9x_libretro.so"), "{err}");
    }

    #[test]
    fn a_finished_directory_replaces_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("retroarch");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("stale"), b"x").unwrap();
        let staged = dir.path().join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("retroarch"), b"y").unwrap();
        put_in_place(&staged, &old).unwrap();
        assert!(old.join("retroarch").is_file());
        assert!(!old.join("stale").exists());
        assert!(!staged.exists());
    }
}
