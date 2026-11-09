use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, decode_record, replay_record, run_match};
use feint_protocol::{ActionT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use feint_web::{Pointer, Replayer, Session, SessionError};

fn script(tick: u64) -> (f32, f32) {
    let points = [(-0.6, 0.3), (-0.2, -0.4), (-0.8, 0.0), (-0.4, 0.45)];
    points[(tick / 37) as usize % points.len()]
}

struct Scripted(Pointer);

impl Controller<AirHockey> for Scripted {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.0.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let (x, y) = script(obs.tick);
        self.0.aim(x, y);
        self.0.on_tick(game, obs)
    }
}

fn speed(p: &[f32; 12], at: usize) -> f64 {
    let (vx, vy) = (f64::from(p[at]), f64::from(p[at + 1]));
    (vx * vx + vy * vy).sqrt()
}

#[test]
fn a_session_plays_the_same_match_as_run_match() {
    let mut session = Session::new(3, 41).unwrap();
    for _ in 0..3_000 {
        let (x, y) = script(session.tick());
        session.set_pointer(x, y);
        session.advance(1);
    }
    let mut game = AirHockey::new(Params::default(), 3);
    let header = Session::header(3, 41, &game).unwrap();
    let mut human = Scripted(Pointer::new());
    let mut bot = Bot::new(
        Difficulty::preset(3).unwrap(),
        HockeyPlaybook::new(Params::default()),
        &game.spec(),
    )
    .unwrap();
    let report = run_match(&mut game, [&mut human, &mut bot], header, 3_000).unwrap();
    let record = decode_record(&session.record()).unwrap();
    assert_eq!(record.actions, report.actions);
    assert_eq!(session.digest(), report.digest);
}

#[test]
fn a_session_plays_to_seven_and_stays_finished() {
    let mut session = Session::new(1, 7).unwrap();
    let mut last = [0, 0];
    let mut guard = 0;
    while !session.finished() && guard < 240 * 60 * 20 {
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        session.advance(4);
        let score = session.score();
        assert!(
            score[0] >= last[0] && score[1] >= last[1],
            "{last:?} -> {score:?}"
        );
        last = score;
        guard += 4;
    }
    assert!(
        session.finished(),
        "no one reached 7 in 20 minutes: {last:?}"
    );
    assert!(last.contains(&7), "{last:?}");
    let (tick, digest) = (session.tick(), session.digest());
    assert!(session.advance(100).is_empty());
    assert_eq!((session.tick(), session.digest()), (tick, digest));
}

#[test]
fn the_human_mallet_reaches_the_human_maxima_and_no_more() {
    let params = Params::default();
    let mut session = Session::new(2, 3).unwrap();
    session.set_pointer(-0.95, -0.45);
    session.advance(240);
    session.set_pointer(-0.05, 0.45);
    let mut fastest: f64 = 0.0;
    for _ in 0..240 {
        session.advance(1);
        fastest = fastest.max(speed(&session.positions(), 6));
    }
    assert!(
        fastest >= 0.95 * params.max_body_speed,
        "top speed {fastest}"
    );
    assert!(
        fastest <= params.max_body_speed + 1e-6,
        "top speed {fastest}"
    );
    let header = decode_record(&session.record()).unwrap().header;
    let human = header.bodies[0].values.clone().unwrap();
    assert_eq!(
        human,
        vec![params.max_body_speed as f32, params.max_body_accel as f32]
    );
    assert_eq!(header.levels, [0, 2]);
    let direct = Session::header(2, 3, &AirHockey::new(Params::default(), 2)).unwrap();
    assert_eq!(direct.levels, [0, 2], "seat 0 is the human");
}

#[test]
fn a_pointer_off_the_table_is_clamped_and_still_moves_the_mallet() {
    let params = Params::default();
    let mut session = Session::new(1, 5).unwrap();
    session.set_pointer(5.0, 5.0);
    session.advance(480);
    let p = session.positions();
    let corner = (
        -params.mallet_radius,
        params.half_width - params.mallet_radius,
    );
    assert!(
        (f64::from(p[4]) - corner.0).abs() < 1e-3 && (f64::from(p[5]) - corner.1).abs() < 1e-3,
        "{:?}",
        &p[4..6]
    );
    let human = decode_record(&session.record())
        .unwrap()
        .actions
        .into_iter()
        .filter(|a| a.player == 0)
        .collect::<Vec<_>>();
    assert_eq!(human.len(), 1, "one target, one action");
    assert_eq!(
        human[0].values,
        Some(vec![params.half_length as f32, params.half_width as f32])
    );
}

#[test]
fn a_still_pointer_adds_no_actions() {
    let mut session = Session::new(1, 5).unwrap();
    for _ in 0..100 {
        session.set_pointer(-0.5, 0.1);
        session.advance(1);
    }
    session.set_pointer(-0.4, 0.1);
    session.advance(1);
    let human = decode_record(&session.record())
        .unwrap()
        .actions
        .into_iter()
        .filter(|a| a.player == 0)
        .count();
    assert_eq!(human, 2);
}

#[test]
fn a_record_replays_to_the_session_digest_finished_or_not() {
    let mut session = Session::new(4, 11).unwrap();
    for _ in 0..2_000 {
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        session.advance(1);
    }
    let record = decode_record(&session.record()).unwrap();
    assert_eq!(record.ticks, 2_000);
    let replayed = replay_record(&mut AirHockey::new(Params::default(), 4), &record).unwrap();
    assert_eq!(replayed, session.digest());

    let mut done = Session::new(1, 12).unwrap();
    for _ in 0..240 * 60 * 20 / 8 {
        if done.finished() {
            break;
        }
        let p = done.positions();
        done.set_pointer(p[0], p[1]);
        done.advance(8);
    }
    assert!(done.finished());
    let record = decode_record(&done.record()).unwrap();
    assert!(record.score.contains(&7));
    assert_eq!(
        replay_record(&mut AirHockey::new(Params::default(), 1), &record).unwrap(),
        done.digest()
    );
}

#[test]
fn levels_differ_and_play_through_the_core() {
    let bot_actions = |level: u8| {
        let mut s = Session::new(level, 9).unwrap();
        for _ in 0..1_500 {
            let (x, y) = script(s.tick());
            s.set_pointer(x, y);
            s.advance(1);
        }
        decode_record(&s.record())
            .unwrap()
            .actions
            .into_iter()
            .filter(|a| a.player == 1)
            .collect::<Vec<_>>()
    };
    let (easy, hard) = (bot_actions(1), bot_actions(5));
    assert_ne!(easy, hard);
    assert!(
        hard.iter().all(|a| a.intent != 0),
        "bot actions carry intents"
    );
    assert_eq!(Session::new(0, 1).err(), Some(SessionError::Level));
    assert_eq!(Session::new(6, 1).err(), Some(SessionError::Level));
}

#[test]
fn advance_reports_the_goals_it_played() {
    let mut session = Session::new(1, 2).unwrap();
    let mut goals = 0;
    for _ in 0..240 * 60 * 5 / 8 {
        let before = session.score();
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        let seen = session
            .advance(8)
            .iter()
            .filter(|e| e.kind == feint_protocol::EventKind::Score)
            .count();
        let after = session.score();
        assert_eq!(seen as u32, after[0] + after[1] - before[0] - before[1]);
        goals += seen;
        if goals >= 2 {
            break;
        }
    }
    assert!(goals >= 2, "no goals in five minutes");
}

fn play_script(session: &mut Session, ticks: u64) {
    for _ in 0..ticks {
        let (x, y) = script(session.tick());
        session.set_pointer(x, y);
        session.advance(1);
    }
}

#[test]
fn a_profile_changes_the_bot_and_carries_forward() {
    let mut first = Session::new(5, 77).unwrap();
    play_script(&mut first, 4_000);
    let v1 = first.profile().expect("level 5 keeps a profile");
    assert!(
        v1.shot_events + v1.bite_events.iter().sum::<u32>() > 0,
        "nothing learned: {v1:?}"
    );

    let mut fresh = Session::new(5, 78).unwrap();
    let mut primed = Session::with_profile(5, 78, Some(&v1)).unwrap();
    play_script(&mut fresh, 3_000);
    play_script(&mut primed, 3_000);
    let bot = |s: &Session| {
        decode_record(&s.record())
            .unwrap()
            .actions
            .into_iter()
            .filter(|a| a.player == 1)
            .collect::<Vec<_>>()
    };
    assert_ne!(
        bot(&fresh),
        bot(&primed),
        "the loaded profile did not change the bot"
    );
    let v2 = primed.profile().unwrap();
    assert!(v2.shot_events >= v1.shot_events);
    assert!(
        v2.bite_events
            .iter()
            .zip(&v1.bite_events)
            .all(|(b, a)| b >= a)
    );

    assert_eq!(
        Session::with_profile(4, 1, Some(&v1)).err(),
        Some(SessionError::Profile)
    );
    assert!(Session::with_profile(4, 1, None).is_ok());
    assert!(
        Session::new(3, 1).unwrap().profile().is_some(),
        "levels 3-4 keep a match model"
    );
    assert!(Session::new(2, 1).unwrap().profile().is_none());
}

#[test]
fn a_replayer_reproduces_a_session_record_byte_for_byte() {
    let mut first = Session::new(5, 90).unwrap();
    play_script(&mut first, 2_000);
    let profile = first.profile();
    let mut session = Session::with_profile(5, 91, profile.as_ref()).unwrap();
    play_script(&mut session, 2_500);
    let bytes = session.record();
    let record = decode_record(&bytes).unwrap();
    let human: Vec<_> = record
        .actions
        .iter()
        .filter(|a| a.player == 0)
        .cloned()
        .collect();

    let mut game = AirHockey::new(Params::default(), 5);
    let mut replayer = Replayer::new(human);
    let mut bot = Bot::new(
        Difficulty::preset(5).unwrap(),
        HockeyPlaybook::new(Params::default()),
        &game.spec(),
    )
    .unwrap();
    bot.load_profile(profile.as_ref().unwrap(), "airhockey")
        .unwrap();
    let report = run_match(
        &mut game,
        [&mut replayer, &mut bot],
        record.header.clone(),
        record.ticks,
    )
    .unwrap();
    assert_eq!(feint_ai::encode_record(&report.record()), bytes);
}
