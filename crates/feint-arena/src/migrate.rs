use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use airhockey::AirHockey;
use arrow::compute::concat_batches;
use arrow::record_batch::RecordBatch;
use duel::QuickDraw;
use feint_ai::{Opponent, Record, decode_profile, decode_record};
use feint_protocol::{
    MAX_MESSAGE_BYTES, MessageT, ObservationT, PROTOCOL_VERSION, decode, encode,
    envelope_buffer_has_identifier, root_as_envelope_with_opts,
};
use feint_web::cards::CARD_GAME_ID;
use feint_web::{Column, HubGame};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use penalty::Penalty;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::parquet_out;
use crate::verify::{Frame, VerifyError, verify};

pub const FROM: &str = "0.1";
pub const BACKUP: &str = "backup-0.1";
pub const LOCK: &str = "migrate-v1.lock";

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
    Row(i64),
    Locked(String),
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
            Self::Row(id) => write!(f, "match {id}: the database row disagrees with its record"),
            Self::Locked(p) => write!(
                f,
                "{p} exists: another run is going, or remove the leftover lock"
            ),
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

struct Replay {
    digest: u64,
    score: Vec<i64>,
    finished: bool,
    columns: &'static [Column],
    ticks: RecordBatch,
}

fn trajectory<G: HubGame>(record: &Record) -> Option<Replay> {
    let level = record.header.levels[1];
    let mut game = G::build(level);
    game.reset(record.header.seed, &record.header.bodies).ok()?;
    let mut pending = record.actions.iter().peekable();
    let mut events = Vec::new();
    let mut frames = Vec::new();
    let mut intent = None;
    for tick in 0..record.ticks {
        if game.result().is_some() {
            return None;
        }
        while let Some(action) = pending.next_if(|a| a.tick <= tick) {
            game.apply(action).ok()?;
            if action.player == 1 {
                intent = Some((action.intent, action.deceptive));
            }
        }
        events.clear();
        game.advance(&mut events);
        frames.push(Frame {
            tick,
            values: game.frame(),
            events: events.iter().map(|e| (e.kind.0, e.player)).collect(),
            intent,
        });
    }
    if pending.next().is_some() {
        return None;
    }
    let score = match game.result() {
        Some(result) => result.score.unwrap_or_default(),
        None => {
            let mut seen = ObservationT::default();
            game.observe(game.live(), 0, &mut seen);
            seen.score.unwrap_or_default()
        }
    };
    let schema = parquet_out::schema(G::COLUMNS);
    let ticks =
        parquet_out::batch(&frames, G::COLUMNS, level, record.header.seed, 0, &schema).ok()?;
    Some(Replay {
        digest: game.digest(game.live()),
        score,
        finished: game.result().is_some(),
        columns: G::COLUMNS,
        ticks,
    })
}

fn same_ticks(path: &Path, replay: &Replay) -> Result<bool, MigrateError> {
    let bad = |e: &dyn std::fmt::Display| MigrateError::Io(format!("{}: {e}", path.display()));
    let reader = ParquetRecordBatchReaderBuilder::try_new(fs::File::open(path)?)
        .map_err(|e| bad(&e))?
        .build()
        .map_err(|e| bad(&e))?;
    let batches = reader
        .collect::<Result<Vec<RecordBatch>, _>>()
        .map_err(|e| bad(&e))?;
    let Some(first) = batches.first() else {
        return Ok(false);
    };
    let stored = concat_batches(&first.schema(), &batches).map_err(|e| bad(&e))?;
    let required = ["tick", "events"]
        .into_iter()
        .chain(replay.columns.iter().map(|c| c.name));
    for name in required {
        if stored.column_by_name(name).is_none() {
            return Ok(false);
        }
    }
    for (i, field) in stored.schema().fields().iter().enumerate() {
        match replay.ticks.column_by_name(field.name()) {
            Some(mine) if mine.as_ref() == stored.column(i).as_ref() => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn replay_hand_made(record: &Record) -> Option<Replay> {
    if matches!(record.header.opponent, Opponent::Trained(_)) {
        return None;
    }
    let version = record.header.game_version;
    match record.header.game_id.as_str() {
        AirHockey::ID if version == AirHockey::VERSION => trajectory::<AirHockey>(record),
        Penalty::ID if version == Penalty::VERSION => trajectory::<Penalty>(record),
        QuickDraw::ID if version == QuickDraw::VERSION => trajectory::<QuickDraw>(record),
        _ => None,
    }
}

fn older_game_version(record: &Record) -> bool {
    let version = record.header.game_version;
    match record.header.game_id.as_str() {
        AirHockey::ID => version < AirHockey::VERSION,
        Penalty::ID => version < Penalty::VERSION,
        QuickDraw::ID => version < QuickDraw::VERSION,
        CARD_GAME_ID => version < believe::GAME_VERSION,
        _ => false,
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
    pub replayed: Vec<(i64, String)>,
    pub byte_proven: Vec<(i64, String, u32)>,
    pub profiles_upgraded: usize,
    pub profiles_current: usize,
    pub stray: Vec<PathBuf>,
    pub backup_made: bool,
}

struct Plan {
    report: Report,
    profiles: Vec<(i64, i64, Vec<u8>, Vec<u8>)>,
    records: Vec<(PathBuf, Vec<u8>, Vec<u8>)>,
    shape: [i64; 4],
}

struct Row {
    id: i64,
    player: i64,
    game: String,
    level: i64,
    seed: i64,
    score: [i64; 2],
    ticks: i64,
    finished: bool,
    digest: String,
    used: Option<i64>,
    record_path: String,
    parquet_path: Option<String>,
    checkpoint: String,
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl Row {
    fn agrees(&self, record: &Record, finished: Option<bool>) -> bool {
        let checkpoint = match record.header.opponent {
            Opponent::HandMade | Opponent::Advised(_) => String::new(),
            Opponent::Trained(id) => format!("{:016x}", id.get()),
        };
        let score = |i: usize| record.score.get(i).copied().unwrap_or(0);
        self.game == record.header.game_id
            && self.level == i64::from(record.header.levels[1])
            && self.seed == to_i64(record.header.seed)
            && self.ticks == to_i64(record.ticks)
            && self.score == [score(0), score(1)]
            && self.checkpoint == checkpoint
            && finished.is_none_or(|f| f == self.finished)
    }
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

fn rows(conn: &Connection) -> Result<Vec<Row>, MigrateError> {
    let mut stmt = conn.prepare(
        "SELECT id, player_id, game, level, seed, score_human, score_bot, ticks, finished,
                digest, profile_used, record_path, parquet_path, checkpoint
         FROM matches WHERE record_path IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Row {
            id: r.get(0)?,
            player: r.get(1)?,
            game: r.get(2)?,
            level: r.get(3)?,
            seed: r.get(4)?,
            score: [r.get(5)?, r.get(6)?],
            ticks: r.get(7)?,
            finished: r.get(8)?,
            digest: r.get(9)?,
            used: r.get(10)?,
            record_path: r.get(11)?,
            parquet_path: r.get(12)?,
            checkpoint: r.get(13)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn check(
    root: &Path,
    row: &Row,
    now: &[u8],
    profiles: &BTreeMap<(i64, i64), Vec<u8>>,
    report: &mut Report,
) -> Result<(), MigrateError> {
    let id = row.id;
    let profile = match row.used {
        None => None,
        Some(v) => {
            let blob = profiles
                .get(&(row.player, v))
                .ok_or(MigrateError::Profile(id))?;
            Some(decode_profile(blob).map_err(|_| MigrateError::Profile(id))?)
        }
    };
    let record = decode_record(now).map_err(|e| MigrateError::Verify(id, format!("{e:?}")))?;
    match verify(now, profile.as_ref()) {
        Ok(v) => {
            if format!("{:016x}", v.digest) != row.digest {
                return Err(MigrateError::Digest(id));
            }
            if !row.agrees(&record, Some(v.finished)) {
                return Err(MigrateError::Row(id));
            }
            report.verified += 1;
        }
        Err(VerifyError::Game) if older_game_version(&record) => {
            if !row.agrees(&record, None) {
                return Err(MigrateError::Row(id));
            }
            report
                .byte_proven
                .push((id, record.header.game_id, record.header.game_version));
        }
        Err(VerifyError::Mismatch) => {
            let refuse = || MigrateError::Verify(id, "Mismatch".into());
            let replay = replay_hand_made(&record).ok_or_else(refuse)?;
            if format!("{:016x}", replay.digest) != row.digest || replay.score != record.score {
                return Err(refuse());
            }
            let stored = row.parquet_path.as_deref().ok_or_else(refuse)?;
            if !same_ticks(&resolve(root, stored)?, &replay)? {
                return Err(refuse());
            }
            if !row.agrees(&record, Some(replay.finished)) {
                return Err(MigrateError::Row(id));
            }
            report.replayed.push((id, record.header.game_id));
        }
        Err(e) => return Err(MigrateError::Verify(id, format!("{e:?}"))),
    }
    Ok(())
}

fn plan(root: &Path) -> Result<Plan, MigrateError> {
    let db = root.join("feint.db");
    if !db.is_file() {
        return Err(MigrateError::Db(format!("no database at {}", db.display())));
    }
    let mut conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = conn.transaction()?;
    let shape = shape(&tx)?;
    let mut report = Report::default();
    let mut current: BTreeMap<(i64, i64), Vec<u8>> = BTreeMap::new();
    let mut profiles = Vec::new();
    {
        let mut stmt = tx
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
                    profiles.push((player, version, blob, new));
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
    for row in rows(&tx)? {
        let path = resolve(root, &row.record_path)?;
        let bytes = fs::read(&path)?;
        let new = upgrade_record(&bytes)?;
        check(
            root,
            &row,
            new.as_deref().unwrap_or(&bytes),
            &current,
            &mut report,
        )?;
        known.push(path.clone());
        match new {
            Some(new) => {
                report.records_upgraded += 1;
                records.push((path, bytes, new));
            }
            None => report.records_current += 1,
        }
    }
    drop(tx);
    let canon = root.canonicalize()?;
    for file in files_under(&canon.join("matches"))? {
        if file.extension().is_some_and(|e| e == "rec") && !known.contains(&file) {
            report
                .stray
                .push(file.strip_prefix(&canon).unwrap_or(&file).to_path_buf());
        }
    }
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

fn backup_holds(done: &Path, canon: &Path, plan: &Plan) -> Result<(), MigrateError> {
    let refuse = |why: String| MigrateError::Backup(format!("{}: {why}", done.display()));
    let db = done.join("feint.db");
    if !db.is_file() {
        return Err(refuse("no feint.db".into()));
    }
    for (path, old, _) in &plan.records {
        let rel = path
            .strip_prefix(canon)
            .map_err(|_| refuse("a record outside the root".into()))?;
        if fs::read(done.join(rel)).ok().as_deref() != Some(old.as_slice()) {
            return Err(refuse(format!("{} is missing or differs", rel.display())));
        }
    }
    if !plan.profiles.is_empty() {
        let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| refuse("feint.db does not open".into()))?;
        for (player, version, old, _) in &plan.profiles {
            let kept: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT blob FROM profiles WHERE player_id = ?1 AND version = ?2",
                    rusqlite::params![player, version],
                    |r| r.get(0),
                )
                .ok();
            if kept.as_ref() != Some(old) {
                return Err(refuse(format!(
                    "profile {player}/{version} is missing or differs"
                )));
            }
        }
    }
    Ok(())
}

fn backup(root: &Path, plan: &Plan) -> Result<bool, MigrateError> {
    let done = root.join(BACKUP);
    let canon = root.canonicalize()?;
    if fs::symlink_metadata(&done).is_ok() {
        backup_holds(&done, &canon, plan)?;
        return Ok(false);
    }
    let tmp = root.join(format!("{BACKUP}.tmp"));
    if fs::symlink_metadata(&tmp).is_ok() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir(&tmp)?;
    let conn =
        Connection::open_with_flags(root.join("feint.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let target = tmp.join("feint.db");
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    drop(conn);
    fs::File::open(&target)?.sync_all()?;
    let mut dirs = std::collections::BTreeSet::new();
    for file in files_under(&canon.join("matches"))? {
        let rel = file
            .strip_prefix(&canon)
            .map_err(|_| MigrateError::Backup("a file outside the root".into()))?;
        let to = tmp.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
            let mut dir = parent.to_path_buf();
            while dir != tmp && dirs.insert(dir.clone()) {
                if !dir.pop() {
                    break;
                }
            }
        }
        fs::copy(&file, &to)?;
        fs::File::open(&to)?.sync_all()?;
    }
    for dir in dirs.iter().rev() {
        sync_dir(dir)?;
    }
    sync_dir(&tmp)?;
    backup_holds(&tmp, &canon, plan)?;
    fs::rename(&tmp, &done)?;
    sync_dir(root)?;
    Ok(true)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), MigrateError> {
    let name = path
        .file_name()
        .ok_or_else(|| MigrateError::Path(path.display().to_string()))?;
    let tmp = path.with_file_name(format!("{}.v1tmp", name.to_string_lossy()));
    if let Ok(meta) = fs::symlink_metadata(&tmp) {
        if !meta.is_file() {
            return Err(MigrateError::Path(tmp.display().to_string()));
        }
        fs::remove_file(&tmp)?;
    }
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

struct Lock(PathBuf);

impl Lock {
    fn take(root: &Path) -> Result<Self, MigrateError> {
        let path = root.join(LOCK);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| MigrateError::Locked(path.display().to_string()))?;
        Ok(Self(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
    let _lock = Lock::take(root)?;
    report.backup_made = backup(root, &plan)?;
    let mut conn =
        Connection::open_with_flags(root.join("feint.db"), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if shape(&tx)? != plan.shape {
        return Err(MigrateError::Changed);
    }
    for (player, version, old, new) in &plan.profiles {
        let changed = tx.execute(
            "UPDATE profiles SET blob = ?1 WHERE player_id = ?2 AND version = ?3 AND blob = ?4",
            rusqlite::params![new, player, version, old],
        )?;
        if changed != 1 {
            return Err(MigrateError::Changed);
        }
    }
    for (path, old, new) in &plan.records {
        if fs::read(path)? != *old {
            return Err(MigrateError::Changed);
        }
        write_atomically(path, new)?;
    }
    tx.commit()?;
    let after = self::plan(root)?;
    if after.report.records_upgraded != 0 || after.report.profiles_upgraded != 0 {
        return Err(MigrateError::Unfinished);
    }
    report.verified = after.report.verified;
    Ok(report)
}
