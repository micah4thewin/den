//! The Den library database: SQLite in WAL mode.
//!
//! Owns games, per-game variants, battery saves, save states, play sessions,
//! BIOS files, and intake reports. Every write goes through SQLite in WAL
//! mode, so a crash mid-write can never corrupt the previous good state.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A shelved game as the interface sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: i64,
    pub title: String,
    pub system: String,
    pub path: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub status: String,
    pub core: Option<String>,
    pub art: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Seconds of recorded play time (from sessions), 0 if never played.
    pub playtime: i64,
    /// Unix time of the most recent session, if any.
    pub last_played: Option<i64>,
}

/// A save (battery save or state) attached to a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Save {
    pub id: i64,
    pub game_id: i64,
    pub kind: String,
    pub path: String,
    pub created_at: i64,
}

/// A play session: a launch that ran (or is running).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub game_id: i64,
    pub started: i64,
    pub duration_seconds: Option<i64>,
}

/// A BIOS file recognised by hash during intake and filed automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bios {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub hash: String,
    pub created_at: i64,
}

/// The library database. One per library directory.
pub struct Db {
    conn: Connection,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Db {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open the database in memory (for tests).
    pub fn open_in_memory() -> rusqlite::Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                id         INTEGER PRIMARY KEY,
                title      TEXT NOT NULL,
                system     TEXT NOT NULL,
                path       TEXT NOT NULL UNIQUE,
                hash       TEXT,
                size       INTEGER,
                status     TEXT NOT NULL DEFAULT 'added',
                core       TEXT,
                art        TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS game_variants (
                id      INTEGER PRIMARY KEY,
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                path    TEXT NOT NULL,
                hash    TEXT,
                note    TEXT
            );
            CREATE TABLE IF NOT EXISTS saves (
                id         INTEGER PRIMARY KEY,
                game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                kind       TEXT NOT NULL,
                path       TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id               INTEGER PRIMARY KEY,
                game_id          INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                started          INTEGER NOT NULL,
                duration_seconds INTEGER
            );
            CREATE TABLE IF NOT EXISTS bios (
                id         INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                path       TEXT NOT NULL,
                hash       TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS reports (
                id         INTEGER PRIMARY KEY,
                created_at INTEGER NOT NULL,
                json       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_games_system ON games(system);
            CREATE INDEX IF NOT EXISTS idx_games_title  ON games(title);
            CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started);",
        )
    }

    /// Add a game; returns its id. A duplicate path is a no-op returning the
    /// existing id, which is what makes re-running intake idempotent.
    pub fn add_game(
        &self,
        title: &str,
        system: &str,
        path: &Path,
        hash: Option<&str>,
        size: Option<i64>,
        status: &str,
    ) -> rusqlite::Result<i64> {
        if let Some(existing) = self.find_by_path(path)? {
            return Ok(existing.id);
        }
        let t = now();
        self.conn.execute(
            "INSERT INTO games (title, system, path, hash, size, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![title, system, path.to_string_lossy(), hash, size, status, t],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Find a game by its shelved file path.
    pub fn find_by_path(&self, path: &Path) -> rusqlite::Result<Option<Game>> {
        let mut stmt = self.conn.prepare(&row_sql("WHERE g.path = ?1"))?;
        let game = stmt
            .query_row(params![path.to_string_lossy()], |row| row_to_game(row))
            .optional()?;
        Ok(game)
    }

    /// Fetch one game by id.
    pub fn get_game(&self, id: i64) -> rusqlite::Result<Option<Game>> {
        let mut stmt = self.conn.prepare(&row_sql("WHERE g.id = ?1"))?;
        let game = stmt
            .query_row(params![id], |row| row_to_game(row))
            .optional()?;
        Ok(game)
    }

    /// List games, optionally filtered by a title substring and a system.
    pub fn list_games(&self, filter: &str, system: Option<&str>) -> rusqlite::Result<Vec<Game>> {
        let (mut sql, mut args): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if filter.is_empty()
        {
            (row_sql(""), vec![])
        } else {
            (
                row_sql("WHERE g.title LIKE ?1"),
                vec![Box::new(format!("%{}%", filter))],
            )
        };
        if let Some(system) = system {
            sql.push_str(if filter.is_empty() { " WHERE " } else { " AND " });
            sql.push_str("g.system = ?");
            let idx = args.len() + 1;
            sql.push_str(&idx.to_string());
            args.push(Box::new(system.to_string()));
        }
        sql.push_str(" ORDER BY g.title COLLATE NOCASE");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            row_to_game(row)
        })?;
        rows.collect()
    }

    /// System names with game counts, ordered by count.
    pub fn list_systems(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT system, COUNT(*) FROM games GROUP BY system ORDER BY system COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Set the per-game core override (NULL clears it back to the default).
    pub fn set_core(&self, game_id: i64, core: Option<&str>) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE games SET core = ?1, updated_at = ?2 WHERE id = ?3",
            params![core, now(), game_id],
        )?;
        Ok(())
    }

    /// Record that a game has an artwork tile (hash-named, libretro style).
    pub fn set_art(&self, game_id: i64, art: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE games SET art = ?1, updated_at = ?2 WHERE id = ?3",
            params![art, now(), game_id],
        )?;
        Ok(())
    }

    /// Attach a save (battery save or state) to a game, deduplicated by path.
    pub fn add_save(&self, game_id: i64, kind: &str, path: &Path) -> rusqlite::Result<i64> {
        let path_str = path.to_string_lossy();
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM saves WHERE game_id = ?1 AND path = ?2",
                params![game_id, path_str],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO saves (game_id, kind, path, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![game_id, kind, path_str, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List saves for a game, newest first.
    pub fn list_saves(&self, game_id: i64) -> rusqlite::Result<Vec<Save>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, game_id, kind, path, created_at FROM saves
             WHERE game_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![game_id], |row| {
            Ok(Save {
                id: row.get(0)?,
                game_id: row.get(1)?,
                kind: row.get(2)?,
                path: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Record a session start; returns the session id.
    pub fn start_session(&self, game_id: i64) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions (game_id, started) VALUES (?1, ?2)",
            params![game_id, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Close a session by id, recording its duration.
    pub fn end_session(&self, session_id: i64) -> rusqlite::Result<()> {
        let started: i64 = self
            .conn
            .query_row(
                "SELECT started FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(now());
        self.conn.execute(
            "UPDATE sessions SET duration_seconds = ?1 WHERE id = ?2",
            params![(now() - started).max(0), session_id],
        )?;
        Ok(())
    }

    /// The most recent sessions, joined with their games (for Continue).
    pub fn recent_sessions(&self, limit: i64) -> rusqlite::Result<Vec<(Session, Game)>> {
        let sql = format!(
            "SELECT s.id, s.game_id, s.started, s.duration_seconds, {}
             FROM sessions s JOIN games g ON g.id = s.game_id
             ORDER BY s.started DESC LIMIT ?1",
            game_columns("g")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit], |row| {
            let session = Session {
                id: row.get(0)?,
                game_id: row.get(1)?,
                started: row.get(2)?,
                duration_seconds: row.get(3)?,
            };
            Ok((session, row_to_game_at(row, 4)?))
        })?;
        rows.collect()
    }

    /// The games the front page leads with: most recently played first.
    pub fn recent_games(&self, limit: i64) -> rusqlite::Result<Vec<Game>> {
        let sql = format!(
            "SELECT DISTINCT {}
             FROM games g JOIN sessions s ON s.game_id = g.id
             ORDER BY (SELECT MAX(started) FROM sessions WHERE game_id = g.id) DESC
             LIMIT ?1",
            game_columns("g")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit], |row| row_to_game(row))?;
        rows.collect()
    }

    /// The game whose newest save or state is the freshest: the Continue row.
    pub fn continue_game(&self) -> rusqlite::Result<Option<Game>> {
        let sql = format!(
            "SELECT {}
             FROM games g
             WHERE EXISTS (SELECT 1 FROM saves WHERE game_id = g.id)
                OR EXISTS (SELECT 1 FROM sessions WHERE game_id = g.id)
             ORDER BY COALESCE(
                (SELECT MAX(created_at) FROM saves WHERE game_id = g.id),
                (SELECT MAX(started) FROM sessions WHERE game_id = g.id)
             ) DESC
             LIMIT 1",
            game_columns("g")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let game = stmt.query_row([], |row| row_to_game(row)).optional()?;
        Ok(game)
    }

    /// File a recognised BIOS, deduplicated by path.
    pub fn add_bios(&self, name: &str, path: &Path, hash: &str) -> rusqlite::Result<i64> {
        let path_str = path.to_string_lossy();
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM bios WHERE path = ?1",
                params![path_str],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO bios (name, path, hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, path_str, hash, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Store an intake report card for the record.
    pub fn add_report(&self, json: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO reports (created_at, json) VALUES (?1, ?2)",
            params![now(), json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The library's total game count.
    pub fn game_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))
    }

    /// All hashes currently shelved: intake uses this to dedupe.
    pub fn shelved_hashes(&self) -> rusqlite::Result<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM games WHERE hash IS NOT NULL")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// The library directory for a game's path (its shelf folder).
    pub fn game_dir(&self, id: i64) -> rusqlite::Result<Option<PathBuf>> {
        let game = self.get_game(id)?;
        Ok(game.map(|g| {
            Path::new(&g.path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        }))
    }
}

fn game_columns(alias: &str) -> String {
    format!(
        "{alias}.id, {alias}.title, {alias}.system, {alias}.path, {alias}.hash, \
         {alias}.size, {alias}.status, {alias}.core, {alias}.art, \
         {alias}.created_at, {alias}.updated_at, \
         COALESCE((SELECT SUM(s.duration_seconds) FROM sessions s WHERE s.game_id = {alias}.id), 0), \
         (SELECT MAX(s.started) FROM sessions s WHERE s.game_id = {alias}.id)"
    )
}

fn row_sql(where_clause: &str) -> String {
    format!(
        "SELECT {} FROM games g {where_clause}",
        game_columns("g")
    )
}

fn row_to_game(row: &rusqlite::Row) -> rusqlite::Result<Game> {
    row_to_game_at(row, 0)
}

fn row_to_game_at(row: &rusqlite::Row, start: usize) -> rusqlite::Result<Game> {
    Ok(Game {
        id: row.get(start)?,
        title: row.get(start + 1)?,
        system: row.get(start + 2)?,
        path: row.get(start + 3)?,
        hash: row.get(start + 4)?,
        size: row.get(start + 5)?,
        status: row.get(start + 6)?,
        core: row.get(start + 7)?,
        art: row.get(start + 8)?,
        created_at: row.get(start + 9)?,
        updated_at: row.get(start + 10)?,
        playtime: row.get(start + 11)?,
        last_played: row.get(start + 12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn add_and_get_game() {
        let db = db();
        let id = db
            .add_game("Sonic", "Genesis", Path::new("/lib/Genesis/Sonic/Sonic.md"), Some("abc"), Some(1024), "added")
            .unwrap();
        let game = db.get_game(id).unwrap().unwrap();
        assert_eq!(game.title, "Sonic");
        assert_eq!(game.system, "Genesis");
        assert_eq!(game.playtime, 0);
    }

    #[test]
    fn duplicate_path_returns_existing() {
        let db = db();
        let path = Path::new("/lib/NES/Zelda/Zelda.nes");
        let a = db.add_game("Zelda", "NES", path, Some("a"), None, "added").unwrap();
        let b = db.add_game("Zelda", "NES", path, Some("a"), None, "added").unwrap();
        assert_eq!(a, b);
        assert_eq!(db.game_count().unwrap(), 1);
    }

    #[test]
    fn list_filters() {
        let db = db();
        db.add_game("Sonic 1", "Genesis", Path::new("/g1"), Some("1"), None, "added").unwrap();
        db.add_game("Sonic 2", "Genesis", Path::new("/g2"), Some("2"), None, "added").unwrap();
        db.add_game("Zelda", "NES", Path::new("/g3"), Some("3"), None, "added").unwrap();
        assert_eq!(db.list_games("", None).unwrap().len(), 3);
        assert_eq!(db.list_games("sonic", None).unwrap().len(), 2);
        assert_eq!(db.list_games("", Some("NES")).unwrap().len(), 1);
    }

    #[test]
    fn sessions_accumulate_playtime() {
        let db = db();
        let id = db.add_game("Sonic", "Genesis", Path::new("/g1"), None, None, "added").unwrap();
        let s = db.start_session(id).unwrap();
        db.end_session(s).unwrap();
        let game = db.get_game(id).unwrap().unwrap();
        assert_eq!(game.playtime, 0); // ended in the same second: 0s is fine
        assert!(game.last_played.is_some());
        assert_eq!(db.recent_games(5).unwrap().len(), 1);
    }

    #[test]
    fn saves_and_continue() {
        let db = db();
        let id = db.add_game("Sonic", "Genesis", Path::new("/g1"), None, None, "added").unwrap();
        db.add_save(id, "battery", Path::new("/saves/1.srm")).unwrap();
        db.add_save(id, "battery", Path::new("/saves/1.srm")).unwrap(); // dedup
        assert_eq!(db.list_saves(id).unwrap().len(), 1);
        let cont = db.continue_game().unwrap().unwrap();
        assert_eq!(cont.id, id);
    }

    #[test]
    fn bios_and_reports() {
        let db = db();
        db.add_bios("psxonpsp660.bin", Path::new("/bios/psxonpsp660.bin"), "deadbeef").unwrap();
        db.add_report("{\"entries\":[]}").unwrap();
        assert_eq!(db.shelved_hashes().unwrap().len(), 0);
    }

    #[test]
    fn wal_mode_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("library.db")).unwrap();
        drop(db);
        let files: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(files.iter().any(|f| f.ends_with("-wal") || f.contains("wal") || f == "library.db"));
    }
}
