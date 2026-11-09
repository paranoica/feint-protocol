mod common;

use feint_ai::{Record, decode_record, encode_record};
use feint_arena::{MAX_TICKS, VerifyError, verify};
use feint_web::{HubGame, Session};
use penalty::Penalty;

fn script(tick: u64) -> (f32, f32) {
    let points = [(-0.6, 0.3), (-0.2, -0.4), (-0.8, 0.0), (-0.4, 0.45)];
    points[(tick / 41) as usize % points.len()]
}

fn chase(session: &mut Session, steps: u64) {
    for _ in 0..steps {
        if session.finished() {
            break;
        }
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        session.advance(4);
    }
}

fn scripted(level: u8, seed: u64, ticks: u64) -> Session {
    let mut s = Session::new(level, seed).unwrap();
    for _ in 0..ticks {
        let (x, y) = script(s.tick());
        s.set_pointer(x, y);
        s.advance(1);
    }
    s
}

fn forge(bytes: &[u8], edit: impl FnOnce(&mut Record)) -> Vec<u8> {
    let mut record = decode_record(bytes).unwrap();
    edit(&mut record);
    encode_record(&record)
}

#[test]
fn genuine_records_verify_and_reproduce_their_match() {
    let s = scripted(1, 3, 3_000);
    let v = verify(&s.record(), None).unwrap();
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, v.record.ticks);
    assert!(!v.finished && v.profile.is_none());

    let mut first = Session::new(5, 8).unwrap();
    chase(&mut first, 240 * 60 * 15 / 4);
    assert!(first.finished());
    let v1 = verify(&first.record(), None).unwrap();
    assert!(v1.finished);
    assert_eq!(v1.profile, first.profile());
    let p1 = v1.profile.clone().unwrap();

    let mut second = Session::with_profile(5, 9, Some(&p1)).unwrap();
    chase(&mut second, 240 * 60 * 15 / 4);
    let v2 = verify(&second.record(), Some(&p1)).unwrap();
    assert_eq!(v2.profile, second.profile());
    assert!(
        v2.profile.unwrap().shot_events >= p1.shot_events,
        "v2 carries v1 forward"
    );
    assert_eq!(
        verify(&second.record(), None).err(),
        Some(VerifyError::Mismatch),
        "the wrong profile"
    );
}

#[test]
fn forged_records_are_refused() {
    let s = scripted(3, 4, 2_500);
    let bytes = s.record();
    let bot_at = |r: &Record| r.actions.iter().position(|a| a.player == 1).unwrap();
    let cases: Vec<(&str, Vec<u8>, VerifyError)> = vec![
        (
            "edited bot action",
            forge(&bytes, |r| {
                let i = bot_at(r);
                r.actions[i].values = Some(vec![0.0, 0.0]);
            }),
            VerifyError::Mismatch,
        ),
        (
            "removed bot action",
            forge(&bytes, |r| {
                let i = bot_at(r);
                r.actions.remove(i);
            }),
            VerifyError::Mismatch,
        ),
        (
            "added bot action",
            forge(&bytes, |r| {
                let i = bot_at(r);
                let a = r.actions[i].clone();
                r.actions.insert(i, a);
            }),
            VerifyError::Mismatch,
        ),
        (
            "edited score",
            forge(&bytes, |r| r.score = vec![7, 0]),
            VerifyError::Mismatch,
        ),
        (
            "seat 1 for the human",
            forge(&bytes, |r| {
                for a in r.actions.iter_mut().filter(|a| a.player == 0) {
                    a.player = 2;
                }
            }),
            VerifyError::Seat,
        ),
        (
            "two human actions on one tick",
            forge(&bytes, |r| {
                let i = r.actions.iter().position(|a| a.player == 0).unwrap();
                let a = r.actions[i].clone();
                r.actions.insert(i, a);
            }),
            VerifyError::Twice,
        ),
        (
            "foreign bodies",
            forge(&bytes, |r| r.header.bodies[1].values = Some(vec![1.0, 1.0])),
            VerifyError::Bodies,
        ),
        (
            "level 0",
            forge(&bytes, |r| r.header.levels[1] = 0),
            VerifyError::Level,
        ),
        (
            "level 6",
            forge(&bytes, |r| r.header.levels[1] = 6),
            VerifyError::Level,
        ),
        (
            "too long",
            forge(&bytes, |r| r.ticks = MAX_TICKS + 1),
            VerifyError::Ticks,
        ),
        (
            "no ticks",
            forge(&bytes, |r| r.ticks = 0),
            VerifyError::Ticks,
        ),
        (
            "other game",
            forge(&bytes, |r| r.header.game_id = "chess".into()),
            VerifyError::Game,
        ),
        (
            "other version",
            forge(&bytes, |r| r.header.game_version += 1),
            VerifyError::Game,
        ),
    ];
    for (name, forged, want) in cases {
        assert_eq!(verify(&forged, None).err(), Some(want), "{name}");
    }
    let mut done = Session::new(1, 6).unwrap();
    chase(&mut done, 240 * 60 * 15 / 4);
    assert!(done.finished());
    let past_the_end = forge(&done.record(), |r| r.ticks += 240);
    assert_eq!(
        verify(&past_the_end, None).err(),
        Some(VerifyError::Mismatch),
        "ticks past a finished match"
    );
    let prefix = forge(&bytes, |r| r.ticks -= 1);
    assert!(
        verify(&prefix, None).is_ok_and(|v| !v.finished),
        "a shorter prefix is a genuine unfinished record"
    );
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(matches!(verify(&extra, None), Err(VerifyError::Decode(_))));
    let p = Session::new(5, 1).unwrap().profile();
    assert_eq!(
        verify(&bytes, p.as_ref()).err(),
        Some(VerifyError::Profile),
        "a profile below level 5"
    );
}

#[test]
fn a_forgery_early_in_the_match_is_refused_without_replaying_the_rest() {
    let s = scripted(5, 21, 20_000);
    let bytes = s.record();
    let began = std::time::Instant::now();
    verify(&bytes, None).unwrap();
    let whole = began.elapsed();
    let early = forge(&bytes, |r| {
        let i = r.actions.iter().position(|a| a.player == 1).unwrap();
        r.actions[i].intent = 9;
    });
    let began = std::time::Instant::now();
    assert_eq!(verify(&early, None).err(), Some(VerifyError::Mismatch));
    let refused = began.elapsed();
    assert!(
        refused * 4 < whole,
        "refused in {refused:?}, a full verification took {whole:?}"
    );
}

#[test]
fn penalty_records_verify_with_their_own_columns_and_profiles() {
    let (s, kicks) = common::shootout(3, 7, None);
    let bytes = s.record();
    let v = verify(&bytes, None).unwrap();
    assert_eq!((v.game, v.columns), (Penalty::ID, Penalty::COLUMNS));
    assert!(v.finished);
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, v.record.ticks);
    assert!(
        v.frames
            .iter()
            .all(|f| f.values.len() == Penalty::COLUMNS.len())
    );
    let last = &v.frames.last().unwrap().values;
    let at = |name: &str| {
        last[Penalty::COLUMNS
            .iter()
            .position(|c| c.name == name)
            .unwrap()]
        .unwrap()
    };
    assert_eq!([at("score0"), at("score1")], s.score().map(f64::from));
    assert_eq!(at("kicks"), f64::from(kicks), "one kick per goal or save");
    assert_eq!(
        verify(&bytes, None),
        verify(&bytes, None),
        "a verification is a value"
    );

    let bot_at = |r: &Record| r.actions.iter().position(|a| a.player == 1).unwrap();
    let edited = forge(&bytes, |r| {
        let i = bot_at(r);
        r.actions[i].values = Some(vec![0.5, 1.0, 1.0, 3.0]);
    });
    assert_eq!(verify(&edited, None).err(), Some(VerifyError::Mismatch));
    let newer = forge(&bytes, |r| r.header.game_version += 1);
    assert_eq!(verify(&newer, None).err(), Some(VerifyError::Game));
    let as_hockey = forge(&bytes, |r| r.header.game_id = "airhockey".into());
    assert!(
        verify(&as_hockey, None).is_err(),
        "a penalty record is not hockey"
    );

    let (five, _) = common::shootout(5, 8, None);
    let v5 = verify(&five.record(), None).unwrap();
    assert_eq!(v5.profile, five.profile());
    let p = v5.profile.unwrap();
    assert_eq!(p.game_id, Penalty::ID);
    let (second, _) = common::shootout(5, 9, Some(&p));
    assert!(verify(&second.record(), Some(&p)).is_ok());
    let hockey = Session::new(5, 1).unwrap().profile().unwrap();
    assert_eq!(
        verify(&second.record(), Some(&hockey)).err(),
        Some(VerifyError::Profile),
        "a hockey profile cannot drive a penalty keeper"
    );
}
