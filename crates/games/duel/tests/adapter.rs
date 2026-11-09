use duel::{DuelPlaybook, OBSERVATION_LEN, Params, Phase, QuickDraw};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_protocol::{ActionT, BodyParamsT, ChaCha8Rng, GameAdapter, ObservationT};
use rand_chacha::rand_core::SeedableRng;

fn body(draw: f32, roll: f32) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(vec![draw, roll]);
    b
}

fn fresh() -> QuickDraw {
    let mut game = QuickDraw::new(Params::default(), 3);
    game.reset(4, &[body(2.86, 1.25), body(2.0, 1.0)]).unwrap();
    game
}

fn act(tick: u64, player: u8, values: [f32; 3]) -> ActionT {
    let mut a = ActionT::default();
    a.tick = tick;
    a.player = player;
    a.values = Some(values.to_vec());
    a
}

#[test]
fn bodies_beyond_the_declared_maxima_or_malformed_actions_are_refused() {
    let mut game = QuickDraw::new(Params::default(), 1);
    assert!(game.reset(1, &[body(2.9, 1.25), body(2.0, 1.0)]).is_err());
    assert!(game.reset(1, &[body(2.0, 0.0), body(2.0, 1.0)]).is_err());
    assert!(
        game.reset(1, &[body(f32::NAN, 1.25), body(2.0, 1.0)])
            .is_err()
    );
    assert!(game.reset(1, &[body(2.0, 1.0)]).is_err());
    let mut game = fresh();
    assert!(game.apply(&act(0, 2, [0.0; 3])).is_err());
    assert!(game.apply(&act(5, 0, [0.0; 3])).is_err());
    assert!(game.apply(&act(0, 0, [f32::NAN, 0.0, 0.0])).is_err());
    let mut short = act(0, 0, [0.0; 3]);
    short.values = Some(vec![0.0, 1.0]);
    assert!(game.apply(&short).is_err());
    assert!(game.apply(&act(0, 0, [0.5, 1.0, 0.0])).is_ok());
}

fn script(s: &duel::Duel, t: u64) -> [ActionT; 2] {
    let draw = (t / 300) % 2 == 1;
    let roll = if t % 517 == 400 { 1.0 } else { 0.0 };
    [
        act(
            t,
            0,
            [s.gunners[1].x as f32 + 0.4, f32::from(u8::from(draw)), 0.0],
        ),
        act(t, 1, [-1.2, f32::from(u8::from(t % 700 > 520)), roll]),
    ]
}

#[test]
fn observations_rebuild_a_state_that_simulates_like_the_live_game() {
    let mut game = fresh();
    let mut events = Vec::new();
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    let mut seen = std::collections::BTreeSet::new();
    while game.live().round < 4 && !game.live().over() {
        let t = game.live().tick;
        for a in script(game.live(), t) {
            game.apply(&a).unwrap();
        }
        game.advance(&mut events);
        let live = game.live().clone();
        if !t.is_multiple_of(7) {
            continue;
        }
        for player in [0u8, 1] {
            let mut obs = ObservationT::default();
            game.observe(&live, player, &mut obs);
            assert_eq!(obs.state.as_ref().map(Vec::len), Some(OBSERVATION_LEN));
            let r = game.state_from_observation(&obs, &mut rng);
            let close = |a: f64, b: f64| (a - b).abs() < 1e-5;
            assert_eq!(
                (r.phase, r.round, r.score),
                (live.phase, live.round, live.score)
            );
            assert!(r.t.abs_diff(live.t) <= 1);
            for seat in 0..2 {
                let (a, b) = (&r.gunners[seat], &live.gunners[seat]);
                assert!(close(a.x, b.x) && close(a.hand, b.hand), "seat {seat}");
                assert_eq!((a.loaded, a.committed), (b.loaded, b.committed));
                assert!(a.recovery.abs_diff(b.recovery) <= 1);
                assert_eq!(r.bullets[seat].is_some(), live.bullets[seat].is_some());
                if let (Some(x), Some(y)) = (r.bullets[seat], live.bullets[seat]) {
                    assert!(close(x.x, y.x) && close(x.lead, y.lead));
                }
            }
            let mine = usize::from(player);
            if live.gunners[mine].committed {
                assert!(r.gunners[mine].aim.is_some());
                assert!(close(
                    r.gunners[mine].aim.unwrap(),
                    live.gunners[mine].aim.unwrap()
                ));
            }
            let them = 1 - mine;
            if live.gunners[them].committed {
                assert_eq!(
                    r.gunners[them].aim,
                    Some(r.gunners[mine].x),
                    "a hidden aim is assumed to be at me"
                );
                seen.insert("hidden aim");
            }
            if live.phase == Phase::Live {
                let held = script(&live, live.tick);
                let ahead = game.simulate(&r, &held, 12);
                let truth = game.simulate(&live, &held, 12);
                for seat in 0..2 {
                    assert!(close(ahead.gunners[seat].x, truth.gunners[seat].x));
                    let step = live.bodies[seat].draw_rate / f64::from(live.params.tick_hz);
                    assert!(
                        (ahead.gunners[seat].hand - truth.gunners[seat].hand).abs() <= 2.0 * step,
                        "an f32 observation may move the shot by one tick, not more"
                    );
                }
                seen.insert("simulated");
            }
            if live.bullets.iter().any(Option::is_some) {
                seen.insert("bullet");
            }
            if live.gunners.iter().any(|g| g.recovery > 0) {
                seen.insert("recovery");
            }
        }
    }
    assert_eq!(seen.len(), 4, "{seen:?}");
}

#[test]
fn simulate_matches_live_stepping_and_hostile_observations_stay_inside_the_rules() {
    let mut game = fresh();
    let actions = [act(0, 0, [0.7, 1.0, 1.0]), act(0, 1, [-1.0, 1.0, -1.0])];
    let simulated = game.simulate(game.live(), &actions, 900);
    let mut events = Vec::new();
    for t in 0..900u64 {
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
    obs.state.as_mut().unwrap()[0] = 1e30;
    obs.state.as_mut().unwrap()[4] = 1e30;
    obs.score = Some(vec![-4, i64::MAX]);
    let s = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(1));
    let w = s.params.half_width;
    assert!(
        s.gunners
            .iter()
            .all(|g| g.x.abs() <= w && g.hand.is_finite())
    );
    assert!(
        s.gunners
            .iter()
            .all(|g| g.recovery <= s.recovery_ticks(0).max(s.recovery_ticks(1)))
    );
    assert!(s.score[1] <= s.params.max_rounds);
}

#[test]
fn two_bots_duel_to_the_end() {
    for (a, b) in [(1u8, 5u8), (5, 5), (3, 2)] {
        let mut game = QuickDraw::new(Params::default(), b);
        let spec = game.spec();
        let make = |level: u8| {
            Bot::new(
                Difficulty::preset(level).unwrap(),
                DuelPlaybook::new(Params::default()),
                &spec,
            )
            .unwrap()
        };
        let header = RunHeader {
            game_id: "duel".into(),
            game_version: duel::GAME_VERSION,
            seed: 11,
            bodies: [a, b]
                .iter()
                .map(|&l| Difficulty::preset(l).unwrap().body(&spec))
                .collect(),
            levels: [a, b],
        };
        let (mut x, mut y) = (make(a), make(b));
        let report = run_match(&mut game, [&mut x, &mut y], header, 240 * 900).unwrap();
        assert!(
            report.finished,
            "{a} vs {b} did not finish: {:?}",
            report.score
        );
        assert!(
            report.score.iter().sum::<i64>() > 0,
            "{a} vs {b}: nobody hit anyone"
        );
    }
}

struct RollsLeft {
    me: u8,
}

impl feint_ai::Controller<QuickDraw> for RollsLeft {
    fn new_match(&mut self, _spec: &feint_protocol::GameSpecT, me: u8, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, game: &QuickDraw, obs: &ObservationT) -> Option<ActionT> {
        let s = game.live();
        let them = &s.gunners[usize::from(1 - self.me)];
        let mine = &s.gunners[usize::from(self.me)];
        let roll = if them.loaded && them.hand > 0.2 && mine.can_roll() && mine.x > -1.5 {
            -1.0
        } else {
            0.0
        };
        Some(act(obs.tick, self.me, [them.x as f32, 0.0, roll]))
    }
}

#[test]
fn a_bot_learns_which_way_its_opponent_rolls() {
    let mut game = QuickDraw::new(Params::default(), 4);
    let spec = game.spec();
    let mut bot = Bot::new(
        Difficulty::preset(4).unwrap(),
        DuelPlaybook::new(Params::default()),
        &spec,
    )
    .unwrap();
    let mut target = RollsLeft { me: 0 };
    let header = RunHeader {
        game_id: "duel".into(),
        game_version: duel::GAME_VERSION,
        seed: 2,
        bodies: vec![body(2.86, 1.25), Difficulty::preset(4).unwrap().body(&spec)],
        levels: [0, 4],
    };
    let report = run_match(&mut game, [&mut target, &mut bot], header, 240 * 900).unwrap();
    assert_eq!(
        bot.insight().likely_cover.get(1).copied().flatten(),
        Some(0),
        "an opponent who always rolls left should be read"
    );
    assert!(
        report.score[1] >= 4,
        "and then shot where it lands: {:?}",
        report.score
    );
}
