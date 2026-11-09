use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OptionalExtension, params};

use crate::parquet_out;
use crate::verify::Verified;

pub const SCHEMA_VERSION: i64 = 3;
pub const MAX_ATTEMPTS: i64 = 3;
pub const TICKET_SECONDS: u64 = 45 * 60;
pub const MAX_OPEN_TICKETS: i64 = 512;
pub const MAX_NICK: usize = 24;

static ATTEMPTS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    Nick,
    Ticket,
    Busy,
    Profile,
    Io(String),
    Db(String),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Db(error.to_string())
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: i64,
    pub profile_version: Option<i64>,
}

pub struct Upload<'a> {
    pub now: u64,
    pub nick: &'a str,
    pub token: &'a str,
    pub used: Option<i64>,
    pub bytes: &'a [u8],
    pub verified: &'a Verified,
    pub profile: Option<Vec<u8>>,
}

pub struct Staged {
    rec: PathBuf,
    parquet: PathBuf,
}

pub fn stage(dir: &Path, upload: &Upload<'_>) -> Result<Staged, StoreError> {
    let attempt = ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    let stem = format!("upload-{}-{attempt}", upload.token);
    let staged = Staged {
        rec: dir.join(format!("{stem}.rec.tmp")),
        parquet: dir.join(format!("{stem}.parquet.tmp")),
    };
    if let Err(error) = write_files(&staged, upload.bytes, upload.verified) {
        let _ = fs::remove_file(&staged.rec);
        let _ = fs::remove_file(&staged.parquet);
        return Err(error);
    }
    Ok(staged)
}

pub struct Store {
    conn: Connection,
    root: PathBuf,
}

pub fn clean_nick(raw: &str) -> Result<String, StoreError> {
    let nick = raw.trim();
    let count = nick.chars().count();
    let bidi = |c: char| matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    if count == 0 || count > MAX_NICK || nick.chars().any(|c| c.is_control() || bidi(c)) {
        return Err(StoreError::Nick);
    }
    Ok(nick.to_owned())
}

impl Store {
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        fs::create_dir_all(root.join("matches"))?;
        let conn = Connection::open(root.join("feint.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE players (id INTEGER PRIMARY KEY, nick TEXT NOT NULL UNIQUE);
                 CREATE TABLE tickets (token TEXT PRIMARY KEY, seed INTEGER NOT NULL,
                     expires INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE matches (id INTEGER PRIMARY KEY,
                     player_id INTEGER NOT NULL REFERENCES players(id),
                     level INTEGER NOT NULL, seed INTEGER NOT NULL,
                     score_human INTEGER NOT NULL, score_bot INTEGER NOT NULL,
                     ticks INTEGER NOT NULL, finished INTEGER NOT NULL, digest TEXT NOT NULL,
                     profile_used INTEGER, record_path TEXT, parquet_path TEXT,
                     created INTEGER NOT NULL);
                 CREATE TABLE profiles (player_id INTEGER NOT NULL REFERENCES players(id),
                     version INTEGER NOT NULL, parent INTEGER, blob BLOB NOT NULL,
                     PRIMARY KEY (player_id, version));
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE tickets ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if version < 3 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE matches ADD COLUMN game TEXT NOT NULL DEFAULT 'airhockey';
                 ALTER TABLE profiles ADD COLUMN game TEXT NOT NULL DEFAULT 'airhockey';
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        Ok(Self {
            conn,
            root: root.to_path_buf(),
        })
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn issue_ticket(&mut self, now: u64, token: &str, seed: u32) -> Result<(), StoreError> {
        let now = to_i64(now);
        self.conn.execute(
            "DELETE FROM tickets WHERE expires <= ?1 AND used = 0",
            [now],
        )?;
        let open: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM tickets WHERE used = 0", [], |r| {
                    r.get(0)
                })?;
        if open >= MAX_OPEN_TICKETS {
            return Err(StoreError::Busy);
        }
        self.conn.execute(
            "INSERT INTO tickets (token, seed, expires) VALUES (?1, ?2, ?3)",
            params![token, i64::from(seed), now + to_i64(TICKET_SECONDS)],
        )?;
        Ok(())
    }

    pub fn ticket_seed(&self, now: u64, token: &str) -> Result<u64, StoreError> {
        let seed: Option<i64> = self
            .conn
            .query_row(
                "SELECT seed FROM tickets WHERE token = ?1 AND used = 0 AND expires > ?2",
                params![token, to_i64(now)],
                |r| r.get(0),
            )
            .optional()?;
        seed.and_then(|s| u64::try_from(s).ok())
            .ok_or(StoreError::Ticket)
    }

    fn player(&self, nick: &str) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT id FROM players WHERE nick = ?1", [nick], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn latest_profile(
        &self,
        nick: &str,
        game: &str,
    ) -> Result<Option<(i64, Vec<u8>)>, StoreError> {
        let Some(player) = self.player(nick)? else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT version, blob FROM profiles WHERE player_id = ?1 AND game = ?2
                 ORDER BY version DESC LIMIT 1",
                params![player, game],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn profile_blob(&self, nick: &str, version: i64) -> Result<Vec<u8>, StoreError> {
        let player = self.player(nick)?.ok_or(StoreError::Profile)?;
        self.conn
            .query_row(
                "SELECT blob FROM profiles WHERE player_id = ?1 AND version = ?2",
                params![player, version],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StoreError::Profile)
    }

    #[must_use]
    pub fn matches_dir(&self) -> PathBuf {
        self.root.join("matches")
    }

    pub fn try_ticket(&mut self, now: u64, token: &str) -> Result<u64, StoreError> {
        let claimed = self.conn.execute(
            "UPDATE tickets SET attempts = attempts + 1
             WHERE token = ?1 AND used = 0 AND expires > ?2 AND attempts < ?3",
            params![token, to_i64(now), MAX_ATTEMPTS],
        )?;
        if claimed != 1 {
            return Err(StoreError::Ticket);
        }
        let seed: i64 =
            self.conn
                .query_row("SELECT seed FROM tickets WHERE token = ?1", [token], |r| {
                    r.get(0)
                })?;
        u64::try_from(seed).map_err(|_| StoreError::Ticket)
    }

    pub fn save(&mut self, upload: &Upload<'_>) -> Result<Saved, StoreError> {
        let staged = stage(&self.matches_dir(), upload)?;
        self.commit_staged(upload, staged)
    }

    pub fn commit_staged(
        &mut self,
        upload: &Upload<'_>,
        staged: Staged,
    ) -> Result<Saved, StoreError> {
        let dir = self.matches_dir();
        let result = self.commit(upload, &dir, &staged);
        if result.is_err() {
            let _ = fs::remove_file(&staged.rec);
            let _ = fs::remove_file(&staged.parquet);
        }
        result
    }

    fn commit(
        &mut self,
        upload: &Upload<'_>,
        dir: &Path,
        temp: &Staged,
    ) -> Result<Saved, StoreError> {
        let Upload {
            now,
            nick,
            token,
            used,
            verified,
            ..
        } = *upload;
        let profile = upload.profile.clone();
        let record = &verified.record;
        let tx = self.conn.transaction()?;
        let spent = tx.execute(
            "UPDATE tickets SET used = 1 WHERE token = ?1 AND used = 0 AND expires > ?2
             AND seed = ?3",
            params![token, to_i64(now), to_i64(record.header.seed)],
        )?;
        if spent != 1 {
            return Err(StoreError::Ticket);
        }
        tx.execute("INSERT OR IGNORE INTO players (nick) VALUES (?1)", [nick])?;
        let player: i64 = tx.query_row("SELECT id FROM players WHERE nick = ?1", [nick], |r| {
            r.get(0)
        })?;
        let score = |i: usize| verified.score.get(i).copied().unwrap_or(0);
        tx.execute(
            "INSERT INTO matches (player_id, game, level, seed, score_human, score_bot, ticks,
                 finished, digest, profile_used, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                player,
                verified.game,
                i64::from(record.header.levels[1]),
                to_i64(record.header.seed),
                score(0),
                score(1),
                to_i64(record.ticks),
                verified.finished,
                format!("{:016x}", verified.digest),
                used,
                to_i64(now),
            ],
        )?;
        let id = tx.last_insert_rowid();
        let newest = |game: Option<&str>| -> Result<Option<i64>, rusqlite::Error> {
            Ok(tx
                .query_row(
                    "SELECT MAX(version) FROM profiles
                     WHERE player_id = ?1 AND (?2 IS NULL OR game = ?2)",
                    params![player, game],
                    |r| r.get(0),
                )
                .optional()?
                .flatten())
        };
        let latest = newest(Some(verified.game))?;
        let mut profile_version = None;
        if let Some(blob) = profile.filter(|_| verified.finished && used == latest) {
            let version = newest(None)?.unwrap_or(0) + 1;
            tx.execute(
                "INSERT INTO profiles (player_id, game, version, parent, blob)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![player, verified.game, version, used, blob],
            )?;
            profile_version = Some(version);
        }
        let dir = dir.join(verified.game);
        fs::create_dir_all(&dir)?;
        let rec = dir.join(format!("{id}.rec"));
        let pq = dir.join(format!("{id}.parquet"));
        fs::rename(&temp.rec, &rec)?;
        if let Err(error) = fs::rename(&temp.parquet, &pq) {
            let _ = fs::remove_file(&rec);
            return Err(error.into());
        }
        let finished = tx
            .execute(
                "UPDATE matches SET record_path = ?1, parquet_path = ?2 WHERE id = ?3",
                params![rec.to_string_lossy(), pq.to_string_lossy(), id],
            )
            .and_then(|_| tx.commit());
        if let Err(error) = finished {
            let _ = fs::remove_file(&rec);
            let _ = fs::remove_file(&pq);
            return Err(error.into());
        }
        Ok(Saved {
            id,
            profile_version,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn write_files(temp: &Staged, bytes: &[u8], verified: &Verified) -> Result<(), StoreError> {
    fs::write(&temp.rec, bytes)?;
    let file = fs::File::create(&temp.parquet)?;
    let header = &verified.record.header;
    parquet_out::write(
        &verified.frames,
        verified.columns,
        header.levels[1],
        header.seed,
        &file,
    )
    .map_err(|e| StoreError::Io(e.to_string()))?;
    file.sync_all()?;
    Ok(())
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}
