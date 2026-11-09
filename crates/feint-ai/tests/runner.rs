mod line_game;

use feint_ai::{Bot, Controller, Difficulty, RunHeader, Runner, replay, replay_record, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use line_game::{Line, LinePlaybook};

fn body(speed: f32) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(vec![speed]);
    b
}

fn header(seed: u64) -> RunHeader {
    RunHeader {
        game_id: "line".into(),
        game_version: 1,
        seed,
        bodies: vec![body(0.8), body(1.2)],
        levels: [2, 3],
    }
}

fn bot(level: u8, game: &Line) -> Bot<Line, LinePlaybook> {
    let d = Difficulty::preset(level).unwrap();
    Bot::new(d, LinePlaybook::new(&[-0.6, 0.0, 0.6], 60), &game.spec()).unwrap()
}

struct Late;

impl Controller<Line> for Late {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {}

    fn on_tick(&mut self, _game: &Line, obs: &ObservationT) -> Option<ActionT> {
        let mut a = ActionT::default();
        a.tick = obs.tick + 1;
        a.player = 1;
        a.values = Some(vec![0.5]);
        Some(a)
    }
}

#[test]
fn runner_records_accepted_actions_and_they_replay_to_the_digest() {
    let mut game = Line::new(0.0, 0.3);
    let (mut a, mut b) = (bot(2, &game), bot(3, &game));
    let report = run_match(&mut game, [&mut a, &mut b], header(4), 2_000).unwrap();
    assert_eq!(report.header, header(4));
    assert_eq!(report.ticks, 2_000);
    assert!(!report.finished);
    assert!(
        report.actions.len() > 500,
        "{} actions",
        report.actions.len()
    );
    assert!(
        report.actions.iter().any(|x| x.player == 0)
            && report.actions.iter().any(|x| x.player == 1)
    );
    assert!(report.actions.windows(2).all(|w| w[0].tick <= w[1].tick));
    assert_eq!(report.digest, game.digest(&game.state));

    let mut fresh = Line::new(0.0, 0.3);
    assert_eq!(replay(&mut fresh, &report).unwrap(), report.digest);
}

#[test]
fn refused_actions_are_not_recorded() {
    let mut game = Line::new(0.0, 0.0);
    let mut a = bot(2, &game);
    let report = run_match(&mut game, [&mut a, &mut Late], header(1), 300).unwrap();
    assert!(
        report.actions.iter().all(|x| x.player == 0),
        "a refused action was recorded"
    );
    assert!(!report.actions.is_empty());
}

#[test]
fn runner_stops_at_the_result_and_reports_it() {
    let mut game = Line::new(0.0, 0.0);
    game.end_at = Some(100);
    let (mut a, mut b) = (bot(1, &game), bot(1, &game));
    let report = run_match(&mut game, [&mut a, &mut b], header(2), 1_000).unwrap();
    assert!(report.finished);
    assert_eq!(report.ticks, 100);
    assert_eq!(report.score, vec![1, 0]);
}

#[test]
fn runner_reports_bad_bodies() {
    let mut game = Line::new(0.0, 0.0);
    let (mut a, mut b) = (bot(1, &game), bot(1, &game));
    let mut bad = header(2);
    bad.bodies.pop();
    assert!(run_match(&mut game, [&mut a, &mut b], bad, 10).is_err());
}

#[test]
fn a_stepped_runner_plays_the_same_match_as_run_match() {
    let mut game = Line::new(0.0, 0.3);
    let (mut a, mut b) = (bot(2, &game), bot(3, &game));
    let whole = run_match(&mut game, [&mut a, &mut b], header(9), 900).unwrap();

    let mut game = Line::new(0.0, 0.3);
    let (mut a, mut b) = (bot(2, &game), bot(3, &game));
    let mut runner = Runner::start(&mut game, [&mut a, &mut b], header(9)).unwrap();
    let mut steps = 0;
    while runner.ticks() < 900 && runner.step(&mut game, [&mut a, &mut b]) {
        steps += 1;
    }
    assert_eq!(steps, 900);
    let stepped = runner.finish(&mut game, [&mut a, &mut b]);
    assert_eq!(stepped, whole);
}

#[test]
fn a_runner_snapshot_replays_mid_match() {
    let mut game = Line::new(0.0, 0.3);
    let (mut a, mut b) = (bot(2, &game), bot(3, &game));
    let mut runner = Runner::start(&mut game, [&mut a, &mut b], header(5)).unwrap();
    for _ in 0..300 {
        runner.step(&mut game, [&mut a, &mut b]);
    }
    let snapshot = runner.report(&game);
    assert_eq!(snapshot.ticks, 300);
    assert!(!snapshot.finished);
    let mut fresh = Line::new(0.0, 0.3);
    assert_eq!(
        replay_record(&mut fresh, &snapshot.record()).unwrap(),
        snapshot.digest
    );
}

#[test]
fn a_finished_runner_steps_no_more_and_reports_no_stale_events() {
    let mut game = Line::new(0.0, 0.3);
    game.end_at = Some(50);
    let (mut a, mut b) = (bot(2, &game), bot(3, &game));
    let mut runner = Runner::start(&mut game, [&mut a, &mut b], header(3)).unwrap();
    let mut saw_events = false;
    while runner.step(&mut game, [&mut a, &mut b]) {
        saw_events |= !runner.events().is_empty();
    }
    assert!(saw_events, "the line game reports its end as an event");
    assert_eq!(runner.ticks(), 50);
    assert!(!runner.step(&mut game, [&mut a, &mut b]));
    assert!(runner.events().is_empty());
}
