use airhockey::{AirHockey, Params};
use duel::QuickDraw;
use feint_ai::{Controller, Difficulty, Opponent, decode_record, run_match};
use feint_ml::registry::CHECKPOINTS;
use feint_ml::trained::trained_pilot;
use feint_protocol::{ActionT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use feint_web::{HubGame, Play, Pointer, Session, SessionError};
use penalty::Penalty;

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

fn id() -> u64 {
    CHECKPOINTS[0].id
}

fn played(seed: u64, ticks: u64) -> Session {
    let mut s = Session::trained(5, seed, id()).unwrap();
    for _ in 0..ticks {
        let (x, y) = script(s.tick());
        s.set_pointer(x, y);
        s.advance(1);
    }
    s
}

#[test]
fn a_trained_bot_match_is_deterministic_for_a_seed() {
    let a = played(41, 3_000);
    let b = played(41, 3_000);
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.record(), b.record());
    assert_ne!(played(42, 3_000).digest(), a.digest());
}

#[test]
fn the_header_names_the_trained_opponent_with_the_levels_body() {
    let s = played(41, 600);
    let header = decode_record(&s.record()).unwrap().header;
    let game = AirHockey::new(Params::default(), 5);
    let body = Difficulty::preset(5).unwrap().body(&game.spec());
    assert_eq!(header.opponent, Opponent::Trained(id()));
    assert_eq!(header.levels, [0, 5]);
    assert_eq!(header.bodies, vec![AirHockey::human_body(), body]);
}

#[test]
fn a_trained_session_plays_what_run_match_plays_with_the_same_pilot() {
    let session = played(41, 3_000);
    let mut game = AirHockey::new(Params::default(), 5);
    let header = Session::header(5, 41, Opponent::Trained(id()), &game).unwrap();
    let mut human = Scripted(Pointer::new());
    let mut pilot = trained_pilot(5, id(), &game.spec()).unwrap();
    let report = run_match(&mut game, [&mut human, &mut pilot], header, 3_000).unwrap();
    assert_eq!(report.digest, session.digest());
    assert!(report.actions.iter().any(|a| a.player == 1));
}

#[test]
fn an_unknown_checkpoint_or_level_is_refused() {
    assert_eq!(
        Session::trained(5, 1, 0).err(),
        Some(SessionError::Checkpoint)
    );
    assert_eq!(
        Session::trained(5, 1, id() ^ 1).err(),
        Some(SessionError::Checkpoint)
    );
    assert_eq!(
        Session::trained(0, 1, id()).err(),
        Some(SessionError::Level)
    );
    assert_eq!(
        Session::trained(6, 1, id()).err(),
        Some(SessionError::Level)
    );
}

#[test]
fn penalty_and_duel_refuse_a_trained_bot() {
    assert_eq!(
        Play::<Penalty>::trained(5, 1, id()).err(),
        Some(SessionError::Unsupported)
    );
    assert_eq!(
        Play::<QuickDraw>::trained(5, 1, id()).err(),
        Some(SessionError::Unsupported)
    );
}

#[test]
fn a_trained_seat_has_no_insight_or_profile() {
    let s = played(41, 600);
    assert!(s.profile().is_none());
    let insight = s.insight();
    assert!(insight.current.is_none() && insight.intent.is_none() && !insight.memory);
    assert!(insight.likely_shot.is_none() && insight.likely_cover.is_empty());
    assert_eq!(s.insight_of(1), insight);
}
