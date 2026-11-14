mod common;

use std::fs;
use std::path::{Path, PathBuf};

use feint_ai::{decode_profile, decode_record, encode_profile, encode_record};
use feint_arena::migrate::{MigrateError, migrate, resolve, upgrade_envelope, upgrade_record};
use feint_arena::{Store, Upload, VerifyError, verify};
use feint_protocol::{DecodeError, EnvelopeT, MessageT, PROTOCOL_VERSION, finish_envelope_buffer};

const FIXTURE: &[u8] = include_bytes!("fixtures/hand-made-v3-scripted.rec");
const NEW: [u8; 8] = [3, 0, 0, 0, b'1', b'.', b'0', 0];
const OLD: [u8; 8] = [3, 0, 0, 0, b'0', b'.', b'1', 0];

fn swap(bytes: &[u8], from: [u8; 8], to: [u8; 8]) -> (Vec<u8>, usize) {
    let mut out = bytes.to_vec();
    let mut n = 0;
    let mut i = 0;
    while i + 8 <= out.len() {
        if out[i..i + 8] == from {
            out[i..i + 8].copy_from_slice(&to);
            n += 1;
            i += 8;
        } else {
            i += 1;
        }
    }
    (out, n)
}

fn frames(bytes: &[u8]) -> usize {
    let mut n = 0;
    let mut rest = bytes;
    while !rest.is_empty() {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        rest = &rest[4 + len..];
        n += 1;
    }
    n
}

fn downgrade(bytes: &[u8]) -> Vec<u8> {
    let (old, n) = swap(bytes, NEW, OLD);
    assert_eq!(n, frames(bytes), "one version string per frame");
    old
}

fn downgrade_blob(blob: &[u8]) -> Vec<u8> {
    let (old, n) = swap(blob, NEW, OLD);
    assert_eq!(n, 1);
    old
}

#[test]
fn a_0_1_record_upgrades_to_the_same_bytes_but_the_version() {
    assert_eq!(PROTOCOL_VERSION, "1.0");
    assert_eq!(
        verify(FIXTURE, None).err(),
        Some(VerifyError::Decode(feint_ai::RecordError::Message(
            DecodeError::Version("0.1".into())
        )))
    );
    let new = upgrade_record(FIXTURE)
        .unwrap()
        .expect("a 0.1 record is upgraded");
    assert_eq!(new.len(), FIXTURE.len());
    let differ = new.iter().zip(FIXTURE).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 2 * frames(FIXTURE));
    assert_eq!(swap(FIXTURE, OLD, NEW).0, new);
    let v = verify(&new, None).unwrap();
    assert_eq!(v.digest, 0x3ead_b537_80e6_dc0d);
    assert_eq!(frames(&new), v.record.actions.len() + 2);
}

#[test]
fn an_upgraded_record_is_left_alone() {
    let new = upgrade_record(FIXTURE).unwrap().unwrap();
    assert_eq!(upgrade_record(&new), Ok(None));
    let first = &new[..4 + u32::from_le_bytes(new[..4].try_into().unwrap()) as usize];
    assert_eq!(upgrade_envelope(&first[4..]), Ok(None));
}

#[test]
fn another_version_or_mixed_versions_are_refused() {
    let (other, n) = swap(FIXTURE, OLD, [3, 0, 0, 0, b'0', b'.', b'2', 0]);
    assert_eq!(n, frames(FIXTURE));
    assert_eq!(
        upgrade_record(&other),
        Err(MigrateError::Version("0.2".into()))
    );
    let new = upgrade_record(FIXTURE).unwrap().unwrap();
    let head = 4 + u32::from_le_bytes(new[..4].try_into().unwrap()) as usize;
    let mut mixed = new[..head].to_vec();
    mixed.extend_from_slice(&FIXTURE[head..]);
    assert_eq!(upgrade_record(&mixed), Err(MigrateError::Mixed));
    let mut mixed = FIXTURE[..head].to_vec();
    mixed.extend_from_slice(&new[head..]);
    assert_eq!(upgrade_record(&mixed), Err(MigrateError::Mixed));
    assert_eq!(
        upgrade_record(&FIXTURE[..FIXTURE.len() - 1]),
        Err(MigrateError::Frame)
    );
    assert_eq!(upgrade_record(&[]), Err(MigrateError::Frame));
    assert_eq!(
        upgrade_envelope(b"not an envelope at all"),
        Err(MigrateError::Envelope)
    );
}

#[test]
fn a_0_1_envelope_laid_out_otherwise_is_refused() {
    let message = decode_record(&upgrade_record(FIXTURE).unwrap().unwrap())
        .map(|r| MessageT::Action(Box::new(r.actions[0].clone())))
        .unwrap();
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let version = fbb.create_string("0.1");
    let body = message.pack(&mut fbb);
    let mut envelope = EnvelopeT::default();
    envelope.message = message;
    let root = feint_protocol::Envelope::create(
        &mut fbb,
        &feint_protocol::EnvelopeArgs {
            protocol_version: Some(version),
            message_type: envelope.message.message_type(),
            message: body,
        },
    );
    finish_envelope_buffer(&mut fbb, root);
    let odd = fbb.finished_data().to_vec();
    assert_eq!(upgrade_envelope(&odd), Err(MigrateError::Layout));
}

#[test]
fn a_profile_blob_upgrades_and_decodes_to_the_same_profile() {
    let (five, _) = common::shootout(5, 8, None);
    let profile = five.profile().expect("level 5 keeps a profile");
    let blob = encode_profile(&profile);
    let old = downgrade_blob(&blob);
    assert!(decode_profile(&old).is_err());
    let new = upgrade_envelope(&old).unwrap().unwrap();
    assert_eq!(new, blob);
    assert_eq!(decode_profile(&new).unwrap(), profile);
    assert_eq!(upgrade_envelope(&new), Ok(None));
}

#[test]
fn a_record_path_resolves_like_bc_and_stays_under_the_matches_folder() {
    let base = scratch("resolve");
    let root = base.join("data");
    fs::create_dir_all(root.join("matches/penalty")).unwrap();
    fs::write(root.join("matches/penalty/6.rec"), b"x").unwrap();
    fs::write(root.join("feint.db"), b"x").unwrap();
    let canon = root.canonicalize().unwrap();
    assert_eq!(
        resolve(&root, "data/matches/penalty/6.rec").unwrap(),
        canon.join("matches/penalty/6.rec")
    );
    let absolute = canon.join("matches/penalty/6.rec");
    assert_eq!(
        resolve(&root, absolute.to_str().unwrap()).unwrap(),
        absolute
    );
    for bad in [
        "data/feint.db",
        "data/matches/../feint.db",
        "data/matches/penalty/7.rec",
        "/etc/passwd",
        "",
    ] {
        assert!(
            matches!(resolve(&root, bad), Err(MigrateError::Path(_))),
            "{bad:?}"
        );
    }
    std::os::unix::fs::symlink(root.join("feint.db"), root.join("matches/link.rec")).unwrap();
    std::os::unix::fs::symlink(
        root.join("matches/penalty/6.rec"),
        root.join("matches/inner.rec"),
    )
    .unwrap();
    for bad in [
        "data/matches/link.rec",
        "data/matches/inner.rec",
        "data/matches/penalty/../penalty/6.rec",
        "data/matches/penalty",
    ] {
        assert!(
            matches!(resolve(&root, bad), Err(MigrateError::Path(_))),
            "{bad:?}"
        );
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("feint-migrate-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Stored {
    root: PathBuf,
    proven: i64,
}

fn store_with_old_data(name: &str) -> Stored {
    let root = scratch(name).join("data");
    let mut store = Store::open(&root).unwrap();
    let save = |store: &mut Store,
                token: &str,
                seed: u32,
                bytes: &[u8],
                used,
                profile: Option<&feint_ai::Profile>| {
        store.issue_ticket(100, token, seed).unwrap();
        let verified = verify(bytes, profile).unwrap();
        let upload = Upload {
            now: 200,
            nick: "ann",
            token,
            used,
            bytes,
            verified: &verified,
            profile: verified.profile.as_ref().map(encode_profile),
        };
        store.save(&upload).unwrap()
    };
    let (first, _) = common::shootout(5, 8, None);
    let saved = save(&mut store, "t1", 8, &first.record(), None, None);
    assert_eq!(saved.profile_version, Some(1));
    let p1 = decode_profile(&store.profile_blob("ann", 1).unwrap()).unwrap();
    let (second, _) = common::shootout(5, 9, Some(&p1));
    let saved = save(&mut store, "t2", 9, &second.record(), Some(1), Some(&p1));
    assert_eq!(saved.profile_version, Some(2));
    let scripted = upgrade_record(FIXTURE).unwrap().unwrap();
    let fixture_seed = decode_record(&scripted).unwrap().header.seed as u32;
    save(&mut store, "t3", fixture_seed, &scripted, None, None);
    let mut legacy = decode_record(&scripted).unwrap();
    legacy.header.game_version = 1;
    fs::create_dir_all(root.join("matches/legacy")).unwrap();
    fs::write(root.join("matches/legacy/4.rec"), encode_record(&legacy)).unwrap();
    let relative = format!(
        "{}/matches/legacy/4.rec",
        root.file_name().unwrap().to_str().unwrap()
    );
    store
        .connection()
        .execute(
            "INSERT INTO matches (player_id, game, level, seed, score_human, score_bot, ticks,
             finished, digest, created, checkpoint, record_path)
             VALUES (1, 'airhockey', ?1, ?2, ?3, ?4, ?5, 0, 'feed', 1, '', ?6)",
            rusqlite::params![
                i64::from(legacy.header.levels[1]),
                legacy.header.seed as i64,
                legacy.score[0],
                legacy.score[1],
                legacy.ticks as i64,
                relative
            ],
        )
        .unwrap();
    let proven = store.connection().last_insert_rowid();
    let paths: Vec<String> = column(&store, "SELECT record_path FROM matches ORDER BY id");
    assert_eq!(paths.len(), 4);
    for p in &paths {
        let path = resolve(&root, p).unwrap();
        fs::write(&path, downgrade(&fs::read(&path).unwrap())).unwrap();
    }
    let blobs: Vec<(i64, i64, Vec<u8>)> = store
        .connection()
        .prepare("SELECT player_id, version, blob FROM profiles")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(blobs.len(), 2);
    for (player, version, blob) in blobs {
        store
            .connection()
            .execute(
                "UPDATE profiles SET blob = ?1 WHERE player_id = ?2 AND version = ?3",
                rusqlite::params![downgrade_blob(&blob), player, version],
            )
            .unwrap();
    }
    drop(store);
    Stored { root, proven }
}

fn column<T: rusqlite::types::FromSql>(store: &Store, sql: &str) -> Vec<T> {
    store
        .connection()
        .prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if !path.to_string_lossy().contains("feint.db") {
                out.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

fn blobs(db: &Path) -> Vec<Vec<u8>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare("SELECT blob FROM profiles ORDER BY player_id, version")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn migrating_a_data_dir_keeps_a_backup_and_is_idempotent() {
    let Stored { root, proven } = store_with_old_data("dir");
    let before = snapshot(&root);
    let old_blobs = blobs(&root.join("feint.db"));
    fs::create_dir_all(root.join("backup-0.1.tmp/matches")).unwrap();
    fs::write(root.join("backup-0.1.tmp/matches/junk.rec"), b"junk").unwrap();
    let check = migrate(&root, false).unwrap();
    assert_eq!((check.records_upgraded, check.profiles_upgraded), (4, 2));
    assert_eq!(
        snapshot(&root)
            .into_iter()
            .filter(|f| !f.0.starts_with("backup-0.1.tmp"))
            .collect::<Vec<_>>(),
        before,
        "a check writes nothing"
    );
    assert!(!root.join("backup-0.1").exists());

    let report = migrate(&root, true).unwrap();
    assert_eq!(report.records_upgraded, 4);
    assert_eq!(report.profiles_upgraded, 2);
    assert_eq!(report.verified, 3);
    assert_eq!(
        report.byte_proven,
        vec![(proven, "airhockey".to_string(), 1)]
    );
    assert!(report.backup_made);

    let backup = root.join("backup-0.1");
    let kept: Vec<_> = snapshot(&backup);
    let originals: Vec<_> = before
        .iter()
        .filter(|(p, _)| p.starts_with("matches"))
        .cloned()
        .collect();
    assert_eq!(kept, originals);
    assert_eq!(blobs(&backup.join("feint.db")), old_blobs);
    assert!(!root.join("backup-0.1.tmp").exists());

    let after = snapshot(&root);
    for (path, bytes) in after
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "rec") && p.starts_with("matches"))
    {
        assert_eq!(upgrade_record(bytes), Ok(None), "{path:?}");
    }
    for blob in blobs(&root.join("feint.db")) {
        assert!(decode_profile(&blob).is_ok());
    }

    let again = migrate(&root, true).unwrap();
    assert_eq!((again.records_upgraded, again.profiles_upgraded), (0, 0));
    assert_eq!(again.verified, 3);
    assert!(!again.backup_made);
    assert_eq!(snapshot(&root), after);
}

#[test]
fn a_rerun_after_records_but_before_profiles_finishes_the_job() {
    let Stored { root, .. } = store_with_old_data("rerun");
    migrate(&root, true).unwrap();
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    let rows: Vec<(i64, i64, Vec<u8>)> = conn
        .prepare("SELECT player_id, version, blob FROM profiles")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (player, version, blob) in rows {
        conn.execute(
            "UPDATE profiles SET blob = ?1 WHERE player_id = ?2 AND version = ?3",
            rusqlite::params![downgrade_blob(&blob), player, version],
        )
        .unwrap();
    }
    drop(conn);
    let report = migrate(&root, true).unwrap();
    assert_eq!((report.records_upgraded, report.profiles_upgraded), (0, 2));
    assert_eq!(report.verified, 3);
    assert!(!report.backup_made);
}

#[test]
fn a_digest_that_disagrees_stops_before_any_write() {
    let Stored { root, .. } = store_with_old_data("digest");
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    conn.execute(
        "UPDATE matches SET digest = '0000000000000000' WHERE id = 2",
        [],
    )
    .unwrap();
    drop(conn);
    let before = snapshot(&root);
    let old_blobs = blobs(&root.join("feint.db"));
    assert_eq!(migrate(&root, true).err(), Some(MigrateError::Digest(2)));
    assert_eq!(snapshot(&root), before);
    assert_eq!(blobs(&root.join("feint.db")), old_blobs);
    assert!(!root.join("backup-0.1").exists());
    assert!(!root.join("backup-0.1.tmp").exists());
}

#[test]
fn an_incomplete_backup_is_refused() {
    let Stored { root, .. } = store_with_old_data("partial");
    let backup = root.join("backup-0.1");
    fs::create_dir_all(&backup).unwrap();
    for file in snapshot(&root) {
        if file.0.starts_with("matches") {
            fs::create_dir_all(backup.join(&file.0).parent().unwrap()).unwrap();
            fs::write(backup.join(&file.0), &file.1).unwrap();
        }
    }
    let before = snapshot(&root);
    assert!(
        matches!(migrate(&root, true), Err(MigrateError::Backup(_))),
        "no database"
    );
    assert_eq!(snapshot(&root), before);
    fs::remove_dir_all(&backup).unwrap();
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("feint.db"), b"db").unwrap();
    let before = snapshot(&root);
    assert!(
        matches!(migrate(&root, true), Err(MigrateError::Backup(_))),
        "no records"
    );
    assert_eq!(snapshot(&root), before);
    let files: Vec<_> = snapshot(&root)
        .into_iter()
        .filter(|f| f.0.starts_with("matches"))
        .collect();
    let first_rec = files
        .iter()
        .position(|f| f.0.extension().is_some_and(|e| e == "rec"))
        .unwrap();
    for (i, (path, bytes)) in files.iter().enumerate() {
        let mut copy = bytes.clone();
        if i == first_rec {
            copy[40] ^= 1;
        }
        fs::create_dir_all(backup.join(path).parent().unwrap()).unwrap();
        fs::write(backup.join(path), copy).unwrap();
    }
    let before = snapshot(&root);
    assert!(
        matches!(migrate(&root, true), Err(MigrateError::Backup(_))),
        "a record differs"
    );
    assert_eq!(snapshot(&root), before);
    let (path, bytes) = &files[first_rec];
    fs::write(backup.join(path), bytes).unwrap();
    let before = snapshot(&root);
    assert!(
        matches!(migrate(&root, true), Err(MigrateError::Backup(_))),
        "the database holds none of the profiles"
    );
    assert_eq!(snapshot(&root), before);
    fs::remove_file(backup.join("feint.db")).unwrap();
    rusqlite::Connection::open(root.join("feint.db"))
        .unwrap()
        .execute(
            "VACUUM INTO ?1",
            [backup.join("feint.db").to_string_lossy().as_ref()],
        )
        .unwrap();
    let mut copy = bytes.clone();
    copy[40] ^= 1;
    fs::write(backup.join(path), copy).unwrap();
    let before = snapshot(&root);
    assert!(
        matches!(migrate(&root, true), Err(MigrateError::Backup(_))),
        "a record differs while the database is whole"
    );
    assert_eq!(snapshot(&root), before);
    fs::write(backup.join(path), bytes).unwrap();
    let report = migrate(&root, true).unwrap();
    assert!(!report.backup_made, "a whole backup is kept and used");
    assert_eq!(report.records_upgraded, 4);
}

#[test]
fn a_record_that_no_longer_replays_stops_before_any_write() {
    let Stored { root, .. } = store_with_old_data("replay");
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    let stored: String = conn
        .query_row("SELECT record_path FROM matches WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    drop(conn);
    let path = resolve(&root, &stored).unwrap();
    let mut record =
        decode_record(&upgrade_record(&fs::read(&path).unwrap()).unwrap().unwrap()).unwrap();
    record.score[0] += 1;
    fs::write(&path, downgrade(&encode_record(&record))).unwrap();
    let before = snapshot(&root);
    assert_eq!(
        migrate(&root, true).err(),
        Some(MigrateError::Verify(1, "Mismatch".into()))
    );
    assert_eq!(snapshot(&root), before);
    assert!(!root.join("backup-0.1").exists());
}

#[test]
fn a_record_file_without_a_row_is_reported_and_left_alone() {
    let Stored { root, .. } = store_with_old_data("stray");
    fs::write(root.join("matches/stray.rec"), FIXTURE).unwrap();
    let report = migrate(&root, true).unwrap();
    assert_eq!(report.stray, vec![PathBuf::from("matches/stray.rec")]);
    assert_eq!(fs::read(root.join("matches/stray.rec")).unwrap(), FIXTURE);
}

fn old_blob(root: &Path, version: i64) -> Vec<u8> {
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    conn.query_row(
        "SELECT blob FROM profiles WHERE version = ?1",
        [version],
        |r| r.get(0),
    )
    .unwrap()
}

fn edit_row(root: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    conn.execute(sql, []).unwrap();
}

fn stored_path(root: &Path, id: i64, column: &str) -> PathBuf {
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    let stored: String = conn
        .query_row(
            &format!("SELECT {column} FROM matches WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap();
    resolve(root, &stored).unwrap()
}

#[test]
fn a_match_the_bot_now_plays_otherwise_is_proven_by_its_inputs_and_parquet() {
    let Stored { root, .. } = store_with_old_data("bot");
    let rec = stored_path(&root, 2, "record_path");
    let bytes = upgrade_record(&fs::read(&rec).unwrap()).unwrap().unwrap();
    assert_eq!(
        verify(&bytes, None).err(),
        Some(VerifyError::Mismatch),
        "without the profile it learned from, the bot plays the shootout otherwise"
    );
    edit_row(&root, "UPDATE matches SET profile_used = NULL WHERE id = 2");
    let report = migrate(&root, false).unwrap();
    assert_eq!(report.replayed, vec![(2, "penalty".to_string())]);
    assert_eq!(report.verified, 2);

    let parquet = stored_path(&root, 2, "parquet_path");
    let kept = fs::read(&parquet).unwrap();
    fs::copy(stored_path(&root, 1, "parquet_path"), &parquet).unwrap();
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(2, "Mismatch".into())),
        "another match's ticks"
    );
    let p1 = decode_profile(&upgrade_envelope(&old_blob(&root, 1)).unwrap().unwrap()).unwrap();
    let truth = verify(&bytes, Some(&p1)).unwrap();
    let rewrite = |frames: &[feint_arena::Frame]| {
        let file = fs::File::create(&parquet).unwrap();
        feint_arena::parquet_out::write(
            frames,
            truth.columns,
            5,
            truth.record.header.seed,
            0,
            file,
        )
        .unwrap();
    };
    let mut frames = truth.frames.clone();
    rewrite(&frames);
    assert_eq!(
        migrate(&root, false).unwrap().replayed,
        vec![(2, "penalty".to_string())]
    );
    let last = frames.len() - 1;
    let v = frames[last]
        .values
        .iter()
        .position(Option::is_some)
        .unwrap();
    frames[last].values[v] = frames[last].values[v].map(|x| x + 1.0);
    rewrite(&frames);
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(2, "Mismatch".into())),
        "one value of one tick differs"
    );
    let bare: Vec<feint_arena::Frame> = truth
        .frames
        .iter()
        .map(|f| feint_arena::Frame {
            values: Vec::new(),
            ..f.clone()
        })
        .collect();
    feint_arena::parquet_out::write(
        &bare,
        &[],
        5,
        truth.record.header.seed,
        0,
        fs::File::create(&parquet).unwrap(),
    )
    .unwrap();
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(2, "Mismatch".into())),
        "a file without the game's columns proves nothing"
    );
    frames.pop();
    rewrite(&frames);
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(2, "Mismatch".into())),
        "one tick short"
    );
    fs::write(&parquet, &kept).unwrap();
    edit_row(
        &root,
        "UPDATE matches SET digest = '0000000000000000' WHERE id = 2",
    );
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(2, "Mismatch".into())),
        "another final state"
    );
}

#[test]
fn a_row_that_disagrees_with_its_record_stops_the_run() {
    for (i, sql) in [
        "UPDATE matches SET score_human = score_human + 1 WHERE id = 1",
        "UPDATE matches SET seed = seed + 1 WHERE id = 1",
        "UPDATE matches SET level = 4 WHERE id = 1",
        "UPDATE matches SET ticks = ticks + 1 WHERE id = 1",
        "UPDATE matches SET finished = 0 WHERE id = 1",
        "UPDATE matches SET game = 'duel' WHERE id = 1",
        "UPDATE matches SET checkpoint = 'aa' WHERE id = 1",
    ]
    .into_iter()
    .enumerate()
    {
        let Stored { root, .. } = store_with_old_data(&format!("row{i}"));
        edit_row(&root, sql);
        assert_eq!(
            migrate(&root, false).err(),
            Some(MigrateError::Row(1)),
            "{sql}"
        );
    }
    let Stored { root, proven } = store_with_old_data("rowold");
    edit_row(
        &root,
        &format!("UPDATE matches SET score_bot = 9 WHERE id = {proven}"),
    );
    assert_eq!(migrate(&root, false).err(), Some(MigrateError::Row(proven)));
}

#[test]
fn only_an_older_game_version_is_byte_proven() {
    for (name, edit) in [
        (
            "newer",
            Box::new(|r: &mut feint_ai::Record| r.header.game_version = 99)
                as Box<dyn Fn(&mut feint_ai::Record)>,
        ),
        (
            "unknown",
            Box::new(|r: &mut feint_ai::Record| r.header.game_id = "nosuch".into()),
        ),
    ] {
        let Stored { root, proven } = store_with_old_data(name);
        let rec = stored_path(&root, proven, "record_path");
        let mut record =
            decode_record(&upgrade_record(&fs::read(&rec).unwrap()).unwrap().unwrap()).unwrap();
        edit(&mut record);
        fs::write(&rec, downgrade(&encode_record(&record))).unwrap();
        assert_eq!(
            migrate(&root, false).err(),
            Some(MigrateError::Verify(proven, "Game".into())),
            "{name}"
        );
    }
}

#[test]
fn a_second_run_waits_for_the_lock_and_a_check_needs_none() {
    let Stored { root, .. } = store_with_old_data("lock");
    fs::write(root.join("migrate-v1.lock"), b"").unwrap();
    let before = snapshot(&root);
    assert!(matches!(migrate(&root, true), Err(MigrateError::Locked(_))));
    assert_eq!(snapshot(&root), before);
    assert_eq!(migrate(&root, false).unwrap().records_upgraded, 4);
    fs::remove_file(root.join("migrate-v1.lock")).unwrap();
    migrate(&root, true).unwrap();
    assert!(
        !root.join("migrate-v1.lock").exists(),
        "the lock goes with the run"
    );
}

#[test]
fn a_trained_match_that_does_not_replay_with_its_bot_is_refused() {
    let root = scratch("trained").join("data");
    let mut store = Store::open(&root).unwrap();
    let mut s = feint_web::Session::trained(5, 31, feint_ml::registry::CHECKPOINTS[0].id).unwrap();
    for _ in 0..150 {
        let p = s.positions();
        s.set_pointer(p[0], p[1]);
        s.advance(4);
    }
    let bytes = s.record();
    store.issue_ticket(100, "t", 31).unwrap();
    let verified = verify(&bytes, None).unwrap();
    store
        .save(&Upload {
            now: 200,
            nick: "ann",
            token: "t",
            used: None,
            bytes: &bytes,
            verified: &verified,
            profile: None,
        })
        .unwrap();
    drop(store);
    let rec = stored_path(&root, 1, "record_path");
    let mut record = decode_record(&bytes).unwrap();
    let bot = record.actions.iter().position(|a| a.player == 1).unwrap();
    record.actions[bot].deceptive = !record.actions[bot].deceptive;
    let edited = encode_record(&record);
    assert_eq!(verify(&edited, None).err(), Some(VerifyError::Mismatch));
    fs::write(&rec, downgrade(&edited)).unwrap();
    assert_eq!(
        migrate(&root, false).err(),
        Some(MigrateError::Verify(1, "Mismatch".into()))
    );
}
