use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use airhockey::AirHockey;
use believe::Believe;
use duel::QuickDraw;
use feint_ai::{Record, decode_profile, decode_record, replay_record};
use feint_protocol::{
    GameAdapter, MAX_MESSAGE_BYTES, MessageT, ObservationT, PROTOCOL_VERSION, decode, encode,
    envelope_buffer_has_identifier, root_as_envelope_with_opts,
};
use feint_web::HubGame;
use feint_web::cards::CARD_GAME_ID;
use penalty::Penalty;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::verify::{VerifyError, verify};

pub const FROM: &str = "0.1";
pub const BACKUP: &str = "backup-0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    Frame,
    Envelope,
    Version(String),
    Mixed,
    Layout,
    Path(String),
    Io(String),
    Db(String),
    Digest(i64),
    Verify(i64, String),
    Profile(i64),
    Backup(String),
    Changed,
    Unfinished,
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame => write!(f, "a record is not a sequence of length-prefixed frames"),
            Self::Envelope => write!(f, "not a valid Feint envelope"),
            Self::Version(v) => write!(
                f,
                "protocol version {v:?}, expected {FROM} or {PROTOCOL_VERSION}"
            ),
            Self::Mixed => write!(f, "a record mixes protocol versions"),
            Self::Layout => write!(f, "re-encoding changes more than the version string"),
            Self::Path(p) => write!(f, "record path {p:?} is not a file under matches/"),
            Self::Io(why) => write!(f, "file error: {why}"),
            Self::Db(why) => write!(f, "database error: {why}"),
            Self::Digest(id) => write!(
                f,
                "match {id}: the replay digest differs from the stored one"
            ),
            Self::Verify(id, why) => write!(f, "match {id} does not verify: {why}"),
            Self::Profile(id) => write!(
                f,
                "match {id}: the profile it was played with is missing or unreadable"
            ),
            Self::Backup(why) => write!(f, "backup: {why}"),
            Self::Changed => write!(
                f,
                "the database changed during the migration: is the arena running?"
            ),
            Self::Unfinished => write!(f, "data still at {FROM} after writing"),
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<rusqlite::Error> for MigrateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<std::io::Error> for MigrateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

fn version_at(bytes: &[u8]) -> Result<(String, usize, MessageT), MigrateError> {
    if bytes.len() > MAX_MESSAGE_BYTES || bytes.len() < 8 || !envelope_buffer_has_identifier(bytes)
    {
        return Err(MigrateError::Envelope);
    }
    let options = flatbuffers::VerifierOptions {
        max_depth: 16,
        max_tables: 4096,
        max_apparent_size: MAX_MESSAGE_BYTES,
        ignore_missing_null_terminator: false,
    };
    let envelope =
        root_as_envelope_with_opts(&options, bytes).map_err(|_| MigrateError::Envelope)?;
    let version = envelope.protocol_version().ok_or(MigrateError::Envelope)?;
    let at = version.as_ptr() as usize - bytes.as_ptr() as usize;
    let message = envelope.unpack().message;
    if matches!(message, MessageT::NONE) {
        return Err(MigrateError::Envelope);
    }
    Ok((version.to_string(), at, message))
}

pub fn upgrade_envelope(bytes: &[u8]) -> Result<Option<Vec<u8>>, MigrateError> {
    let (version, at, message) = version_at(bytes)?;
    if version == PROTOCOL_VERSION {
        decode(bytes).map_err(|_| MigrateError::Envelope)?;
        return Ok(None);
    }
    if version != FROM {
        return Err(MigrateError::Version(version.chars().take(16).collect()));
    }
    let new = encode(&message);
    let mut patched = bytes.to_vec();
    patched[at..at + FROM.len()].copy_from_slice(PROTOCOL_VERSION.as_bytes());
    if new != patched {
        return Err(MigrateError::Layout);
    }
    Ok(Some(new))
}

pub fn upgrade_record(bytes: &[u8]) -> Result<Option<Vec<u8>>, MigrateError> {
    if bytes.is_empty() {
        return Err(MigrateError::Frame);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut upgraded = None;
    let mut rest = bytes;
    while !rest.is_empty() {
        let (len, body) = rest.split_at_checked(4).ok_or(MigrateError::Frame)?;
        let len = u32::from_le_bytes(len.try_into().map_err(|_| MigrateError::Frame)?) as usize;
        let (frame, tail) = body.split_at_checked(len).ok_or(MigrateError::Frame)?;
        rest = tail;
        let new = upgrade_envelope(frame)?;
        if upgraded.is_some_and(|u| u != new.is_some()) {
            return Err(MigrateError::Mixed);
        }
        upgraded = Some(new.is_some());
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(new.as_deref().unwrap_or(frame));
    }
    Ok(upgraded.unwrap_or(false).then_some(out))
}

fn replayed<A: GameAdapter>(mut game: A, record: &Record) -> Option<(u64, Vec<i64>)> {
    let digest = replay_record(&mut game, record).ok()?;
    let score = match game.result() {
        Some(result) => result.score.unwrap_or_default(),
        None => {
            let mut seen = ObservationT::default();
            game.observe(game.live(), 0, &mut seen);
            seen.score.unwrap_or_default()
        }
    };
    Some((digest, score))
}

fn replay_inputs(record: &Record) -> Option<(u64, Vec<i64>)> {
    let level = record.header.levels[1];
    let version = record.header.game_version;
    match record.header.game_id.as_str() {
        AirHockey::ID if version == AirHockey::VERSION => replayed(AirHockey::build(level), record),
        Penalty::ID if version == Penalty::VERSION => replayed(Penalty::build(level), record),
        QuickDraw::ID if version == QuickDraw::VERSION => replayed(QuickDraw::build(level), record),
        CARD_GAME_ID if version == believe::GAME_VERSION => replayed(Believe::default(), record),
        _ => None,
    }
}

pub fn resolve(root: &Path, stored: &str) -> Result<PathBuf, MigrateError> {
    let refuse = || MigrateError::Path(stored.chars().take(200).collect());
    let raw = Path::new(stored);
    if stored.is_empty() || raw.components().any(|c| c == Component::ParentDir) {
        return Err(refuse());
    }
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let base = root
            .parent()
            .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
        base.join(raw)
    };
    let meta = fs::symlink_metadata(&joined).map_err(|_| refuse())?;
    if !meta.is_file() {
        return Err(refuse());
    }
    let matches = root.join("matches").canonicalize().map_err(|_| refuse())?;
    let path = joined.canonicalize().map_err(|_| refuse())?;
    if !path.starts_with(&matches) {
        return Err(refuse());
    }
    Ok(path)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub records_upgraded: usize,
    pub records_current: usize,
    pub verified: usize,
    pub replayed: Vec<(i64, String, String)>,
    pub byte_proven: Vec<(i64, String, u32)>,
    pub profiles_upgraded: usize,
    pub profiles_current: usize,
    pub stray: Vec<PathBuf>,
    pub backup_made: bool,
}

struct Plan {
    report: Report,
    profiles: Vec<(i64, i64, Vec<u8>)>,
    records: Vec<(PathBuf, Vec<u8>)>,
    shape: [i64; 4],
}

fn shape(conn: &Connection) -> Result<[i64; 4], MigrateError> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM matches), (SELECT IFNULL(MAX(id), 0) FROM matches),
                (SELECT COUNT(*) FROM profiles),
                (SELECT IFNULL(MAX(rowid), 0) FROM profiles)",
        [],
        |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]),
    )?)
}

fn files_under(dir: &Path) -> Result<Vec<PathBuf>, MigrateError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn plan(root: &Path) -> Result<Plan, MigrateError> {
    let db = root.join("feint.db");
    if !db.is_file() {
        return Err(MigrateError::Db(format!("no database at {}", db.display())));
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut report = Report::default();
    let mut current: BTreeMap<(i64, i64), Vec<u8>> = BTreeMap::new();
    let mut profiles = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT player_id, version, blob FROM profiles ORDER BY player_id, version")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        for row in rows {
            let (player, version, blob) = row?;
            match upgrade_envelope(&blob)? {
                Some(new) => {
                    report.profiles_upgraded += 1;
                    current.insert((player, version), new.clone());
                    profiles.push((player, version, new));
                }
                None => {
                    report.profiles_current += 1;
                    current.insert((player, version), blob);
                }
            }
        }
    }
    let mut records = Vec::new();
    let mut known = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, player_id, digest, profile_used, record_path FROM matches
             WHERE record_path IS NOT NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (id, player, digest, used, stored) = row?;
            let path = resolve(root, &stored)?;
            let bytes = fs::read(&path)?;
            let new = upgrade_record(&bytes)?;
            let now = new.as_deref().unwrap_or(&bytes);
            let profile = match used {
                None => None,
                Some(v) => {
                    let blob = current.get(&(player, v)).ok_or(MigrateError::Profile(id))?;
                    Some(decode_profile(blob).map_err(|_| MigrateError::Profile(id))?)
                }
            };
            match verify(now, profile.as_ref()) {
                Ok(v) => {
                    if format!("{:016x}", v.digest) != digest {
                        return Err(MigrateError::Digest(id));
                    }
                    report.verified += 1;
                }
                Err(VerifyError::Game) => {
                    let record = decode_record(now)
                        .map_err(|e| MigrateError::Verify(id, format!("{e:?}")))?;
                    report.byte_proven.push((
                        id,
                        record.header.game_id,
                        record.header.game_version,
                    ));
                }
                Err(e) => {
                    let why = format!("{e:?}");
                    let record =
                        decode_record(now).map_err(|_| MigrateError::Verify(id, why.clone()))?;
                    match replay_inputs(&record) {
                        Some((d, score))
                            if format!("{d:016x}") == digest && score == record.score =>
                        {
                            report.replayed.push((id, record.header.game_id, why));
                        }
                        _ => return Err(MigrateError::Verify(id, why)),
                    }
                }
            }
            known.push(path.clone());
            match new {
                Some(new) => {
                    report.records_upgraded += 1;
                    records.push((path, new));
                }
                None => report.records_current += 1,
            }
        }
    }
    let matches = root.join("matches").canonicalize()?;
    for file in files_under(&matches)? {
        if file.extension().is_some_and(|e| e == "rec") && !known.contains(&file) {
            report.stray.push(
                file.strip_prefix(&root.canonicalize()?)
                    .unwrap_or(&file)
                    .to_path_buf(),
            );
        }
    }
    let shape = shape(&conn)?;
    Ok(Plan {
        report,
        profiles,
        records,
        shape,
    })
}

fn sync_dir(dir: &Path) -> Result<(), MigrateError> {
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

fn backup(root: &Path, plan: &Plan) -> Result<bool, MigrateError> {
    let done = root.join(BACKUP);
    let canon = root.canonicalize()?;
    if done.exists() {
        if !done.join("feint.db").is_file() {
            return Err(MigrateError::Backup(format!(
                "{} has no feint.db",
                done.display()
            )));
        }
        for (path, _) in &plan.records {
            let rel = path
                .strip_prefix(&canon)
                .map_err(|_| MigrateError::Backup("a record outside the root".into()))?;
            if !done.join(rel).is_file() {
                return Err(MigrateError::Backup(format!(
                    "{} lacks {}",
                    done.display(),
                    rel.display()
                )));
            }
        }
        return Ok(false);
    }
    let tmp = root.join(format!("{BACKUP}.tmp"));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    let conn =
        Connection::open_with_flags(root.join("feint.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let target = tmp.join("feint.db");
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    drop(conn);
    fs::File::open(&target)?.sync_all()?;
    let matches = canon.join("matches");
    for file in files_under(&matches)? {
        let rel = file
            .strip_prefix(&canon)
            .map_err(|_| MigrateError::Backup("a file outside the root".into()))?;
        let to = tmp.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&file, &to)?;
        fs::File::open(&to)?.sync_all()?;
    }
    sync_dir(&tmp)?;
    fs::rename(&tmp, &done)?;
    sync_dir(root)?;
    Ok(true)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), MigrateError> {
    let name = path
        .file_name()
        .ok_or_else(|| MigrateError::Path(path.display().to_string()))?;
    let tmp = path.with_file_name(format!("{}.v1tmp", name.to_string_lossy()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

pub fn migrate(root: &Path, write: bool) -> Result<Report, MigrateError> {
    let plan = plan(root)?;
    if !write {
        return Ok(plan.report);
    }
    let mut report = plan.report.clone();
    if plan.records.is_empty() && plan.profiles.is_empty() {
        return Ok(report);
    }
    report.backup_made = backup(root, &plan)?;
    let mut conn =
        Connection::open_with_flags(root.join("feint.db"), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if shape(&tx)? != plan.shape {
        return Err(MigrateError::Changed);
    }
    for (player, version, blob) in &plan.profiles {
        tx.execute(
            "UPDATE profiles SET blob = ?1 WHERE player_id = ?2 AND version = ?3",
            rusqlite::params![blob, player, version],
        )?;
    }
    for (path, bytes) in &plan.records {
        write_atomically(path, bytes)?;
    }
    tx.commit()?;
    let after = self::plan(root)?;
    if after.report.records_upgraded != 0 || after.report.profiles_upgraded != 0 {
        return Err(MigrateError::Unfinished);
    }
    report.verified = after.report.verified;
    Ok(report)
}
