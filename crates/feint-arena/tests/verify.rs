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

#[test]
fn duel_records_verify_with_their_own_columns() {
    use feint_web::Play;
    let mut s = Play::<duel::QuickDraw>::new(2, 3).unwrap();
    while !s.finished() {
        let v = s.view();
        s.set_input(&[v[7], 1.0, 0.0]);
        s.advance(4);
    }
    let bytes = s.record();
    let v = verify(&bytes, None).unwrap();
    assert_eq!((v.game, v.columns), ("duel", duel::QuickDraw::COLUMNS));
    assert!(v.finished);
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, v.record.ticks);
    let last = &v.frames.last().unwrap().values;
    let at = |name: &str| {
        last[duel::QuickDraw::COLUMNS
            .iter()
            .position(|c| c.name == name)
            .unwrap()]
        .unwrap()
    };
    assert_eq!([at("score0"), at("score1")], s.score().map(f64::from));
    let edited = forge(&bytes, |r| {
        let i = r.actions.iter().position(|a| a.player == 1).unwrap();
        r.actions[i].values = Some(vec![2.0, 1.0, 1.0]);
    });
    assert_eq!(verify(&edited, None).err(), Some(VerifyError::Mismatch));
}

#[test]
fn hand_made_records_still_verify_once_migrated_to_1_0() {
    let old = include_bytes!("fixtures/hand-made-v3-scripted.rec");
    let bytes = feint_arena::migrate::upgrade_record(old).unwrap().unwrap();
    let v = verify(&bytes, None).unwrap();
    assert_eq!(v.digest, 0x3ead_b537_80e6_dc0d);
    assert_eq!(v.record.ticks, 3_000);
    assert!(!v.finished && v.profile.is_none());
}

fn nonzero(id: u64) -> std::num::NonZeroU64 {
    std::num::NonZeroU64::new(id).unwrap()
}

fn trained_session(seed: u64, ticks: u64) -> Session {
    let mut s = Session::trained(5, seed, feint_ml::registry::CHECKPOINTS[0].id).unwrap();
    for _ in 0..ticks {
        let (x, y) = script(s.tick());
        s.set_pointer(x, y);
        s.advance(1);
    }
    s
}

fn hand_made_profile() -> feint_ai::Profile {
    let s = scripted(5, 4, 2_000);
    s.profile().expect("level 5 keeps a profile")
}

#[test]
fn verify_rebuilds_the_trained_opponent() {
    let s = trained_session(17, 3_000);
    let v = verify(&s.record(), None).unwrap();
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, 3_000);
    assert_eq!(
        v.record.header.opponent,
        feint_ai::Opponent::Trained(nonzero(feint_ml::registry::CHECKPOINTS[0].id))
    );
    assert!(v.frames.iter().all(|f| f.intent.is_none()));
}

#[test]
fn a_trained_match_yields_no_profile_where_a_hand_made_one_does() {
    let hand = scripted(5, 4, 2_000);
    assert!(verify(&hand.record(), None).unwrap().profile.is_some());
    let trained = trained_session(4, 2_000);
    assert!(verify(&trained.record(), None).unwrap().profile.is_none());
}

#[test]
fn unknown_checkpoint_is_refused() {
    let s = trained_session(17, 600);
    let id = feint_ml::registry::CHECKPOINTS[0].id;
    let forged = forge(&s.record(), |r| {
        r.header.opponent = feint_ai::Opponent::Trained(nonzero(id ^ 1));
    });
    assert_eq!(verify(&forged, None).err(), Some(VerifyError::Checkpoint));
}

#[test]
fn trained_penalty_record_is_refused() {
    let p = feint_web::Play::<Penalty>::new(3, 5).unwrap();
    let mut p = p;
    p.advance(600);
    let id = feint_ml::registry::CHECKPOINTS[0].id;
    let forged = forge(&p.record(), |r| {
        r.header.opponent = feint_ai::Opponent::Trained(nonzero(id));
    });
    assert_eq!(verify(&forged, None).err(), Some(VerifyError::Game));
}

#[test]
fn trained_with_a_profile_is_refused() {
    let s = trained_session(17, 600);
    let profile = hand_made_profile();
    assert_eq!(
        verify(&s.record(), Some(&profile)).err(),
        Some(VerifyError::Profile)
    );
}

#[test]
fn a_trained_record_relabelled_as_hand_made_is_a_mismatch() {
    let s = trained_session(17, 1_200);
    let forged = forge(&s.record(), |r| {
        r.header.opponent = feint_ai::Opponent::HandMade;
    });
    assert_eq!(verify(&forged, None).err(), Some(VerifyError::Mismatch));
}

const ADVISED_SHOOTOUT: u64 = 0x3a1e_c278_c59e_254a;

#[test]
fn a_level_four_shootout_names_its_context_model_and_verifies() {
    let (s, _) = common::shootout(4, 11, None);
    let bytes = s.record();
    let record = feint_ai::decode_record(&bytes).unwrap();
    assert_eq!(
        record.header.opponent,
        feint_ai::Opponent::Advised(nonzero(penalty::trained::SHIPPED))
    );
    let v = verify(&bytes, None).unwrap();
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.digest, ADVISED_SHOOTOUT, "the advised path is pinned");
    let unknown = forge(&bytes, |r| {
        r.header.opponent = feint_ai::Opponent::Advised(nonzero(penalty::trained::SHIPPED ^ 1));
    });
    assert_eq!(verify(&unknown, None).err(), Some(VerifyError::Checkpoint));
    let plain = forge(&bytes, |r| r.header.opponent = feint_ai::Opponent::HandMade);
    assert_eq!(
        verify(&plain, None).err(),
        Some(VerifyError::Level),
        "level 4 plays with the advisor, so a plain hand-made record is refused"
    );
}

#[test]
fn an_advisor_where_none_ships_is_refused() {
    let (five, _) = common::shootout(5, 12, None);
    let advised = forge(&five.record(), |r| {
        r.header.opponent = feint_ai::Opponent::Advised(nonzero(penalty::trained::SHIPPED));
    });
    assert_eq!(verify(&advised, None).err(), Some(VerifyError::Level));
    let (three, _) = common::shootout(3, 13, None);
    let advised = forge(&three.record(), |r| {
        r.header.opponent = feint_ai::Opponent::Advised(nonzero(penalty::trained::SHIPPED));
    });
    assert_eq!(
        verify(&advised, None).err(),
        Some(VerifyError::Level),
        "level 3 has no advisor"
    );
    let hockey = scripted(3, 9, 600);
    let advised = forge(&hockey.record(), |r| {
        r.header.opponent = feint_ai::Opponent::Advised(nonzero(penalty::trained::SHIPPED));
    });
    assert_eq!(verify(&advised, None).err(), Some(VerifyError::Game));
}

#[test]
fn table_tennis_records_verify_with_their_own_columns() {
    use feint_web::Play;
    let mut s = Play::<pingpong::PingPong>::new(2, 5).unwrap();
    while !s.finished() && s.tick() < 240 * 1800 {
        let v = s.view();
        s.set_input(&[-1.9, v[3], 1.0, 1.0, 0.3, 0.6, 0.8]);
        s.advance(4);
    }
    assert!(s.finished());
    let bytes = s.record();
    let v = verify(&bytes, None).unwrap();
    assert_eq!(
        (v.game, v.columns),
        ("pingpong", pingpong::PingPong::COLUMNS)
    );
    assert!(v.finished);
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, v.record.ticks);
    let last = &v.frames.last().unwrap().values;
    let at = |name: &str| {
        last[pingpong::PingPong::COLUMNS
            .iter()
            .position(|c| c.name == name)
            .unwrap()]
        .unwrap()
    };
    assert_eq!([at("score0"), at("score1")], s.score().map(f64::from));
    let edited = forge(&bytes, |r| {
        let i = r.actions.iter().position(|a| a.player == 1).unwrap();
        r.actions[i].values = Some(vec![1.9, 0.5, 1.0, 1.0, -0.5, 0.5, 0.0]);
    });
    assert_eq!(verify(&edited, None).err(), Some(VerifyError::Mismatch));
}

#[test]
fn boxing_records_verify_with_their_own_columns() {
    use feint_web::Play;
    let mut s = Play::<boxing::Boxing>::new(3, 5).unwrap();
    let mut round = 0u32;
    while !s.finished() && s.tick() < 240 * 1800 {
        s.set_input(&[0.0, 1.0, 0.0, 0.0, 0.0]);
        if s.tick().is_multiple_of(120) {
            let height = if round.is_multiple_of(2) { 1.0 } else { -1.0 };
            s.push_input(&[0.0, 1.0, 1.0, height, 0.0]);
            round += 1;
        }
        s.advance(4);
    }
    assert!(s.finished());
    let bytes = s.record();
    let v = verify(&bytes, None).unwrap();
    assert_eq!((v.game, v.columns), ("boxing", boxing::Boxing::COLUMNS));
    assert!(v.finished);
    assert_eq!(v.digest, s.digest());
    assert_eq!(v.frames.len() as u64, v.record.ticks);
    let last = &v.frames.last().unwrap().values;
    let at = |name: &str| {
        last[boxing::Boxing::COLUMNS
            .iter()
            .position(|c| c.name == name)
            .unwrap()]
        .unwrap()
    };
    assert_eq!([at("score0"), at("score1")], s.score().map(f64::from));
    assert!(at("score0") + at("score1") > 0.0);
    let edited = forge(&bytes, |r| {
        let i = r.actions.iter().position(|a| a.player == 0).unwrap();
        r.actions[i].values = Some(vec![2.0, -1.0, 2.0, -1.0, 0.0]);
    });
    assert_eq!(verify(&edited, None).err(), Some(VerifyError::Mismatch));
}
