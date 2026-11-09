use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_protocol::{ActionT, BodyParamsT, ChaCha8Rng, GameAdapter, ObservationT};
use penalty::{OBSERVATION_LEN, Params, Penalty, PenaltyPlaybook, Phase};
use rand_chacha::rand_core::SeedableRng;

fn body(dive: f32, lean: f32) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(vec![dive, lean]);
    b
}

fn fresh() -> Penalty {
    let mut game = Penalty::new(Params::default(), 3);
    game.reset(4, &[body(9.0, 4.0), body(6.0, 3.0)]).unwrap();
    game
}

fn act(tick: u64, player: u8, values: [f32; 4]) -> ActionT {
    let mut a = ActionT::default();
    a.tick = tick;
    a.player = player;
    a.values = Some(values.to_vec());
    a
}

#[test]
fn bodies_beyond_the_declared_maxima_or_malformed_actions_are_refused() {
    let mut game = Penalty::new(Params::default(), 1);
    assert!(game.reset(1, &[body(9.5, 4.0), body(6.0, 3.0)]).is_err());
    assert!(
        game.reset(1, &[body(f32::NAN, 4.0), body(6.0, 3.0)])
            .is_err()
    );
    assert!(game.reset(1, &[body(6.0, 3.0)]).is_err());
    let mut game = fresh();
    assert!(game.apply(&act(0, 2, [0.0; 4])).is_err());
    assert!(game.apply(&act(5, 0, [0.0; 4])).is_err());
    assert!(game.apply(&act(0, 0, [f32::NAN, 0.0, 0.0, 0.0])).is_err());
    let mut short = act(0, 0, [0.0; 4]);
    short.values = Some(vec![0.0, 1.0]);
    assert!(game.apply(&short).is_err());
    assert!(game.apply(&act(0, 0, [0.5, 1.0, 0.0, 0.0])).is_ok());
}

#[test]
fn observations_rebuild_a_state_that_simulates_like_the_live_game() {
    let mut game = fresh();
    let mut events = Vec::new();
    let script = |s: &penalty::Shootout, t: u64| {
        let near = s.dist < 0.6 && s.phase == Phase::RunUp;
        let shooter = s.shooter();
        let lean = if t % 600 < 200 { -0.8 } else { 0.6 };
        let shot = act(t, shooter, [lean, 1.0, f32::from(u8::from(near)), 2.6]);
        let dive = s.phase == Phase::RunUp && s.dist < 1.0;
        let keep = act(t, 1 - shooter, [0.4, 0.0, f32::from(u8::from(dive)), -2.0]);
        [shot, keep]
    };
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    let mut roles = std::collections::BTreeSet::new();
    let mut phases = std::collections::BTreeSet::new();
    while game.live().kicks < 3 {
        let t = game.live().tick;
        for a in script(game.live(), t) {
            game.apply(&a).unwrap();
        }
        game.advance(&mut events);
        let live = game.live().clone();
        if !t.is_multiple_of(13) || live.phase == Phase::Strike {
            continue;
        }
        roles.insert(live.shooter());
        phases.insert(live.phase.code() as i64);
        for player in [0u8, 1] {
            let mut obs = ObservationT::default();
            game.observe(&live, player, &mut obs);
            assert_eq!(obs.state.as_ref().map(Vec::len), Some(OBSERVATION_LEN));
            let r = game.state_from_observation(&obs, &mut rng);
            let close = |a: f64, b: f64| (a - b).abs() < 1e-5;
            assert_eq!(
                (r.shooter(), r.phase, r.kicks, r.score),
                (live.shooter(), live.phase, live.kicks, live.score)
            );
            assert!(r.t.abs_diff(live.t) <= 1, "{} vs {}", r.t, live.t);
            assert!(
                close(r.dist, live.dist)
                    && close(r.lean, live.lean)
                    && close(r.ball_y, live.ball_y)
            );
            assert!(close(r.keeper_y, live.keeper_y) && close(r.keeper_target, live.keeper_target));
            assert_eq!(r.dived, live.dived);
            assert_eq!(r.dive.map(|d| d.left > 0), live.dive.map(|d| d.left > 0));
            if live.phase == Phase::Flight && live.t > 0 {
                assert!(
                    close(r.aim.unwrap(), live.aim.unwrap()),
                    "the aim is recovered from the ball in flight"
                );
            }
            if live.phase == Phase::RunUp && close(live.lean, live.lean_target) && live.running {
                let held = script(&live, live.tick);
                let ahead = game.simulate(&r, &held, 12);
                let truth = game.simulate(&live, &held, 12);
                assert!(close(ahead.dist, truth.dist) && close(ahead.keeper_y, truth.keeper_y));
            }
        }
    }
    assert_eq!(roles.len(), 2, "both seats shot during the check");
    assert!(
        phases.contains(&0) && phases.contains(&2) && phases.contains(&3),
        "{phases:?}"
    );
}

#[test]
fn simulate_matches_live_stepping_and_hostile_observations_stay_inside_the_rules() {
    let mut game = fresh();
    let actions = [
        act(0, 0, [0.7, 1.0, 1.0, 3.0]),
        act(0, 1, [-1.0, 0.0, 1.0, 2.0]),
    ];
    let simulated = game.simulate(game.live(), &actions, 400);
    let mut events = Vec::new();
    for t in 0..400u64 {
        for a in &actions {
            let mut stamped = a.clone();
            stamped.tick = t;
            let _ = game.apply(&stamped);
        }
        game.advance(&mut events);
    }
    assert_eq!(game.digest(&simulated), game.digest(game.live()));

    let mut obs = ObservationT::default();
    game.observe(game.live(), 1, &mut obs);
    obs.state = Some(vec![f32::NAN; OBSERVATION_LEN]);
    obs.state.as_mut().unwrap()[7] = 1e30;
    obs.score = Some(vec![-4, i64::MAX]);
    let s = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(1));
    assert!(s.keeper_y.abs() <= s.params.half_goal);
    assert!(s.dist.is_finite() && s.lean.is_finite() && s.ball_y.is_finite());
    assert!(s.score[1] <= s.params.max_kicks);
}

#[test]
fn two_bots_play_a_shootout_to_its_end() {
    for (a, b) in [(1u8, 5u8), (5, 5), (3, 2)] {
        let mut game = Penalty::new(Params::default(), b);
        let spec = game.spec();
        let make = |level: u8| {
            Bot::new(
                Difficulty::preset(level).unwrap(),
                PenaltyPlaybook::new(Params::default()),
                &spec,
            )
            .unwrap()
        };
        let header = RunHeader {
            game_id: "penalty".into(),
            game_version: penalty::GAME_VERSION,
            seed: 11,
            bodies: [a, b]
                .iter()
                .map(|&l| Difficulty::preset(l).unwrap().body(&spec))
                .collect(),
            levels: [a, b],
        };
        let (mut x, mut y) = (make(a), make(b));
        let report = run_match(&mut game, [&mut x, &mut y], header, 240 * 600).unwrap();
        assert!(
            report.finished,
            "{a} vs {b} did not finish: {:?}",
            report.score
        );
        assert!(
            report.score.iter().sum::<i64>() > 0,
            "{a} vs {b}: nobody scored"
        );
    }
}

struct LeftFooter {
    me: u8,
}

impl feint_ai::Controller<Penalty> for LeftFooter {
    fn new_match(&mut self, _spec: &feint_protocol::GameSpecT, me: u8, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        let s = game.live();
        let shooting = s.shooter() == self.me;
        let fire = shooting && s.dist < 0.6;
        Some(act(
            obs.tick,
            self.me,
            [0.0, 1.0, f32::from(u8::from(fire)), -2.8],
        ))
    }
}

#[test]
fn a_keeper_bot_learns_where_its_opponent_shoots() {
    let mut game = Penalty::new(Params::default(), 3);
    let spec = game.spec();
    let mut keeper = Bot::new(
        Difficulty::preset(3).unwrap(),
        PenaltyPlaybook::new(Params::default()),
        &spec,
    )
    .unwrap();
    let mut shooter = LeftFooter { me: 0 };
    let header = RunHeader {
        game_id: "penalty".into(),
        game_version: penalty::GAME_VERSION,
        seed: 2,
        bodies: vec![body(9.0, 4.0), Difficulty::preset(3).unwrap().body(&spec)],
        levels: [0, 3],
    };
    run_match(&mut game, [&mut shooter, &mut keeper], header, 240 * 900).unwrap();
    assert_eq!(
        keeper.insight().likely_shot,
        Some(0),
        "five kicks to the left should teach the keeper"
    );
}
