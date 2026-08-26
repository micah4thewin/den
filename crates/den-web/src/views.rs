use den_core::{CoreStatus, Den, Game, RetroArchStatus, Save};
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
}

pub fn library_view(den: &Den) -> Result<LibraryView, String> {
    den.reap();
    let games = den.db().list_games("", None).map_err(|e| e.to_string())?;
    let systems = den
        .db()
        .list_systems()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(name, count)| SystemRow { name, count })
        .collect();
    let continue_game = den.db().continue_game().map_err(|e| e.to_string())?;
    let recent = den.db().recent_games(8).map_err(|e| e.to_string())?;
    Ok(LibraryView {
        path: den.library.display().to_string(),
        games,
        systems,
        continue_game,
        recent,
        retroarch: den.retroarch_status(),
    })
}

pub fn game_view(den: &Den, id: i64) -> Result<GameView, String> {
    den.reap();
    let game = den
        .db()
        .get_game(id)
        .map_err(|e| e.to_string())?
        .ok_or("game not found")?;
    let saves = den.db().list_saves(id).map_err(|e| e.to_string())?;
    let core = den.core_status(&game);
    Ok(GameView {
        game,
        saves,
        retroarch: den.retroarch_status(),
        core,
    })
}
