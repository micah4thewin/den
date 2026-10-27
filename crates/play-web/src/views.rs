use play_core::{CoreStatus, Game, Play, RetroArchStatus, Save};
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
    Ok(GameView {
        game,
        saves,
        retroarch: play.retroarch_status(),
        core,
        playing,
    })
}
