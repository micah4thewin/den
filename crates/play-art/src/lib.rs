//! Box art for the shelf.
//!
//! libretro keeps box art for every system it knows, one image per game,
//! named after the game as the No-Intro and Redump sets name it. Play ships
//! the list of those names ([`data/boxarts.txt.gz`], rebuilt by
//! `tools/cover_index.py`), so choosing a cover is a lookup on this machine.
//! Only the one image a game needs is fetched, once, and then it lives in the
//! library with everything else.

use play_ident::System;
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

mod matching;

pub use matching::{base_key, choose};

const INDEX_GZ: &[u8] = include_bytes!("../data/boxarts.txt.gz");

/// The repository libretro keeps a system's thumbnails in.
pub fn repo_for(system: System) -> &'static str {
    match system {
        System::Nes => "Nintendo_-_Nintendo_Entertainment_System",
        System::Snes => "Nintendo_-_Super_Nintendo_Entertainment_System",
        System::Genesis => "Sega_-_Mega_Drive_-_Genesis",
        System::SegaCd => "Sega_-_Mega-CD_-_Sega_CD",
        System::Sega32x => "Sega_-_32X",
        System::N64 => "Nintendo_-_Nintendo_64",
        System::Ps1 => "Sony_-_PlayStation",
        System::Gb => "Nintendo_-_Game_Boy",
        System::Gbc => "Nintendo_-_Game_Boy_Color",
        System::Gba => "Nintendo_-_Game_Boy_Advance",
        System::Arcade => "FBNeo_-_Arcade_Games",
        System::Dos => "DOS",
        System::Ps2 => "Sony_-_PlayStation_2",
        System::Gamecube => "Nintendo_-_GameCube",
        System::Wii => "Nintendo_-_Wii",
    }
}

/// Every box art name for every system, read once from the shipped index.
fn index() -> &'static HashMap<&'static str, Vec<String>> {
    static INDEX: OnceLock<HashMap<&'static str, Vec<String>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut text = String::new();
        let mut map: HashMap<&'static str, Vec<String>> = HashMap::new();
        if let Err(e) = flate2::read::GzDecoder::new(INDEX_GZ).read_to_string(&mut text) {
            log::warn!("the box art index could not be read: {e}");
            return map;
        }
        let repos: HashMap<String, &'static str> = all_systems()
            .map(|s| (repo_for(s).to_string(), repo_for(s)))
            .collect();
        let mut current: Option<&'static str> = None;
        for line in text.lines() {
            if let Some(repo) = line.strip_prefix('#') {
                current = repos.get(repo).copied();
                continue;
            }
            if let (Some(repo), false) = (current, line.is_empty()) {
                map.entry(repo).or_default().push(line.to_string());
            }
        }
        map
    })
}

fn all_systems() -> impl Iterator<Item = System> {
    [
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
    ]
    .into_iter()
}

/// The names libretro has box art under for `system`.
pub fn names_for(system: System) -> &'static [String] {
    index()
        .get(repo_for(system))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// How many box art names the shipped index holds, across every system.
pub fn total_names() -> usize {
    index().values().map(Vec::len).sum()
}

/// The box art name that best fits a game, from what Play knows of it: its
/// title, and the name of the file it came in as.
pub fn cover_name(system: System, wanted: &[&str]) -> Option<String> {
    choose(wanted, names_for(system))
}

/// libretro's file names cannot hold these characters, and use `_` for them.
pub fn thumbnail_safe(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            c => c,
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The network is not there, or the host did not answer: try later.
    #[error("could not reach {host}: {reason}")]
    Offline { host: String, reason: String },
}

/// Where box art is fetched from. Each base is tried in turn.
#[derive(Debug, Clone)]
pub struct Sources {
    bases: Vec<String>,
}

impl Default for Sources {
    fn default() -> Self {
        if let Ok(base) = std::env::var("PLAY_ART_BASE") {
            return Sources::at(&[base.as_str()]);
        }
        Sources::at(&[
            "https://raw.githubusercontent.com/libretro-thumbnails/{repo}/master/Named_Boxarts/{name}.png",
            "https://thumbnails.libretro.com/{system}/Named_Boxarts/{name}.png",
        ])
    }
}

impl Sources {
    /// URL templates, with `{repo}`, `{system}` and `{name}` filled in per
    /// image. A base without `{name}` gets `/{repo}/{name}.png` added.
    pub fn at(bases: &[&str]) -> Sources {
        Sources {
            bases: bases
                .iter()
                .map(|b| {
                    if b.contains("{name}") {
                        b.to_string()
                    } else {
                        format!("{}/{{repo}}/{{name}}.png", b.trim_end_matches('/'))
                    }
                })
                .collect(),
        }
    }

    fn urls(&self, repo: &str, name: &str) -> Vec<String> {
        let system = repo.replace('_', " ");
        self.bases
            .iter()
            .map(|b| {
                b.replace("{repo}", &encode(repo))
                    .replace("{system}", &encode(&system))
                    .replace("{name}", &encode(name))
            })
            .collect()
    }
}

fn encode(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for b in part.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Fetches box art over HTTPS, trusting the certificates the operating
/// system trusts.
pub struct Fetcher {
    agent: ureq::Agent,
    sources: Sources,
}

const MAX_IMAGE: u64 = 16 * 1024 * 1024;

impl Fetcher {
    pub fn new(sources: Sources) -> Fetcher {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent("Play (local game launcher)")
            .tls_config(tls)
            .build()
            .new_agent();
        Fetcher { agent, sources }
    }

    /// The image for `name` in `system`'s set, if any source has it.
    /// `Ok(None)` means every source answered and none had it.
    pub fn fetch(&self, system: System, name: &str) -> Result<Option<Vec<u8>>, FetchError> {
        self.fetch_following(repo_for(system), name, 0)
    }

    fn fetch_following(
        &self,
        repo: &str,
        name: &str,
        hops: u8,
    ) -> Result<Option<Vec<u8>>, FetchError> {
        let mut offline: Option<FetchError> = None;
        for url in self.sources.urls(repo, name) {
            match self.get(&url) {
                Ok(Some(bytes)) if is_image(&bytes) => return Ok(Some(bytes)),
                Ok(Some(bytes)) => {
                    // libretro keeps alternate names as git symlinks, and
                    // the raw host serves a symlink as the path it points
                    // to. Follow it, once or twice, within the same set.
                    if let Some(target) = symlink_target(&bytes) {
                        if hops < 2 {
                            if let Ok(Some(img)) = self.fetch_following(repo, &target, hops + 1) {
                                return Ok(Some(img));
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => offline = Some(e),
            }
        }
        match offline {
            // Every source failed to answer: say so, so the caller retries
            // later instead of deciding the game has no cover.
            Some(e) => Err(e),
            None => Ok(None),
        }
    }

    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, FetchError> {
        let host = url
            .split("://")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or(url)
            .to_string();
        let mut response = match self.agent.get(url).call() {
            Ok(r) => r,
            Err(e) => {
                return Err(FetchError::Offline {
                    host,
                    reason: e.to_string(),
                })
            }
        };
        let status = response.status().as_u16();
        if status == 404 || status == 410 {
            return Ok(None);
        }
        if !(200..300).contains(&status) {
            return Err(FetchError::Offline {
                host,
                reason: format!("answered {status}"),
            });
        }
        match response
            .body_mut()
            .with_config()
            .limit(MAX_IMAGE)
            .read_to_vec()
        {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) => Err(FetchError::Offline {
                host,
                reason: e.to_string(),
            }),
        }
    }
}

pub fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[0xff, 0xd8, 0xff])
}

fn symlink_target(bytes: &[u8]) -> Option<String> {
    if bytes.len() > 512 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let file = text.rsplit('/').next()?;
    let name = file.strip_suffix(".png")?;
    (!name.is_empty() && !name.contains('\n')).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_has_names_in_the_shipped_index() {
        for system in all_systems() {
            assert!(
                names_for(system).len() > 50,
                "{} has {} names",
                system.name(),
                names_for(system).len()
            );
        }
    }

    #[test]
    fn real_names_resolve_to_real_covers() {
        let cases = [
            (
                System::Genesis,
                "Sonic the Hedgehog (USA, Europe)",
                "Sonic The Hedgehog (USA, Europe)",
            ),
            (
                System::Nes,
                "Super Mario Bros (USA)",
                "Super Mario Bros. (World)",
            ),
            (
                System::Ps1,
                "Final Fantasy VII (USA) (Disc 1)",
                "Final Fantasy VII (USA) (Disc 1)",
            ),
            (System::Snes, "Super Mario World", "Super Mario World (USA)"),
        ];
        for (system, wanted, expected) in cases {
            assert_eq!(
                cover_name(system, &[wanted]).as_deref(),
                Some(expected),
                "{wanted}"
            );
        }
    }

    #[test]
    fn urls_are_encoded_and_templated() {
        let s = Sources::at(&["http://127.0.0.1:9/{repo}/x/{name}.png"]);
        assert_eq!(
            s.urls("Sega_-_32X", "A & B (USA)"),
            vec!["http://127.0.0.1:9/Sega_-_32X/x/A%20%26%20B%20%28USA%29.png"]
        );
        let s = Sources::at(&["http://h/base/"]);
        assert_eq!(s.urls("DOS", "Doom"), vec!["http://h/base/DOS/Doom.png"]);
    }

    #[test]
    fn symlink_bodies_are_followed() {
        assert_eq!(
            symlink_target(b"Super Mario Bros. (World).png").as_deref(),
            Some("Super Mario Bros. (World)")
        );
        assert_eq!(symlink_target(b"\x89PNG"), None);
    }

    #[test]
    fn libretro_forbidden_characters_become_underscores() {
        assert_eq!(thumbnail_safe("Q*bert: A/B?"), "Q_bert_ A_B_");
    }
}
