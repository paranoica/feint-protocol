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
    let script = |t: u64| {
        let lean = if t < 200 { -0.8 } else { 0.6 };
        [
            act(t, 0, [lean, 1.0, f32::from(u8::from(t == 250)), 2.6]),
            act(t, 1, [0.4, 0.0, f32::from(u8::from(t == 230)), -2.0]),
        ]
    };
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    for t in 0..300u64 {
        for a in script(t) {
            game.apply(&a).unwrap();
        }
        game.advance(&mut events);
        if t % 37 == 0 {
            for player in [0u8, 1] {
                let mut obs = ObservationT::default();
                game.observe(game.live(), player, &mut obs);
                assert_eq!(obs.state.as_ref().map(Vec::len), Some(OBSERVATION_LEN));
                let rebuilt = game.state_from_observation(&obs, &mut rng);
                assert_eq!(rebuilt.shooter(), game.live().shooter());
                assert_eq!(rebuilt.phase, game.live().phase);
                assert!((rebuilt.keeper_y - game.live().keeper_y).abs() < 1e-5);
                assert!((rebuilt.dist - game.live().dist).abs() < 1e-5);
            }
        }
    }
    assert_eq!(game.live().phase, Phase::Flight);
    assert_eq!(
        game.live().aim,
        Some(f64::from(2.6f32)),
        "the kick was taken"
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
