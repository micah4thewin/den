use std::path::Path;

use play_core::{CoreStatus, Game, Play, RetroArchStatus, Save};
use play_ident::System;
use serde::Serialize;

#[derive(Serialize)]
pub struct SystemRow {
    pub name: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct LibraryView {
    pub path: String,
    pub games: Vec<Game>,
    pub systems: Vec<SystemRow>,
    pub continue_game: Option<Game>,
    pub recent: Vec<Game>,
    pub retroarch: RetroArchStatus,
}

#[derive(Serialize)]
pub struct GameView {
    pub game: Game,
    pub saves: Vec<Save>,
    pub retroarch: RetroArchStatus,
    pub core: CoreStatus,
    /// Whether this one is playing right now, so a screen can offer to stop it
    /// rather than offering to start it a second time.
    pub playing: bool,
    /// Whether the phone can run it itself, in its own browser.
    pub phone: PhoneView,
}

#[derive(Serialize, Default)]
pub struct PhoneView {
    /// EmulatorJS's name for the system, when the phone can play it.
    pub system: Option<&'static str>,
    /// Where the phone fetches the game from.
    pub rom: Option<String>,
    /// Why the phone cannot play it, in words.
    pub problem: Option<String>,
}

/// Disc images that are really several files: the phone is handed one file,
/// so these play on the machine and not on the phone, for now.
const SEVERAL_FILES: &[&str] = &["cue", "m3u", "gdi", "ccd", "toc"];

pub fn phone_view(game: &Game) -> PhoneView {
    let refuse = |why: &str| PhoneView {
        problem: Some(why.to_string()),
        ..PhoneView::default()
    };
    let system = match System::from_name(&game.system) {
        Some(System::Nes) => "nes",
        Some(System::Snes) => "snes",
        Some(System::Genesis) => "segaMD",
        Some(System::Sega32x) => "sega32x",
        Some(System::N64) => "n64",
        Some(System::Ps1) => "psx",
        Some(System::Gb) | Some(System::Gbc) => "gb",
        Some(System::Gba) => "gba",
        Some(System::Arcade) => "arcade",
        Some(System::SegaCd) => {
            return refuse("Sega CD games need the console's BIOS, so they play on the machine.")
        }
        Some(System::Dos) => return refuse("DOS games play on the machine, not on the phone."),
        Some(System::Ps2) | Some(System::Gamecube) | Some(System::Wii) => {
            return refuse("This system is too much for a phone's browser.")
        }
        None => return refuse("Play does not know how to run this system on a phone."),
    };
    let path = Path::new(&game.path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if SEVERAL_FILES.contains(&ext.as_str()) {
        return refuse(
            "This game is several files on disk, and the phone takes one. \
             A .chd or .pbp copy of it plays on the phone.",
        );
    }
    // The emulator names the game after the last part of its address and
    // the core goes by the extension, so the address ends in a plain one.
    let file = if ext.is_empty() {
        "game".to_string()
    } else {
        format!("game.{ext}")
    };
    PhoneView {
        system: Some(system),
        rom: Some(format!("/api/rom/{}/{file}", game.id)),
        problem: None,
    }
}

pub fn library_view(play: &Play) -> Result<LibraryView, String> {
    play.reap();
    let games = play.db().list_games("", None).map_err(|e| e.to_string())?;
    let systems = play
        .db()
        .list_systems()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(name, count)| SystemRow { name, count })
        .collect();
    let continue_game = play.db().continue_game().map_err(|e| e.to_string())?;
    let recent = play.db().recent_games(8).map_err(|e| e.to_string())?;
    Ok(LibraryView {
        path: play.library.display().to_string(),
        games,
        systems,
        continue_game,
        recent,
        retroarch: play.retroarch_status(),
    })
}

pub fn game_view(play: &Play, id: i64) -> Result<GameView, String> {
    play.reap();
    let game = play
        .db()
        .get_game(id)
        .map_err(|e| e.to_string())?
        .ok_or("game not found")?;
    let saves = play.db().list_saves(id).map_err(|e| e.to_string())?;
    let core = play.core_status(&game);
    let playing = play.running().iter().any(|live| live.game_id == id);
    let phone = phone_view(&game);
    Ok(GameView {
        game,
        saves,
        retroarch: play.retroarch_status(),
        core,
        playing,
        phone,
    })
}
