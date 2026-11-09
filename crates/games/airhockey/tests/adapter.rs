#[macro_use]
mod builders;

use airhockey::{AirHockey, GAME_VERSION, Mallet, OBSERVATION_LEN, Params, Puck, Vec2};
use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventKind, EventT, GameAdapter, MessageT,
    ObservationT, TimeMode, decode, encode,
};
use rand_chacha::rand_core::SeedableRng;

fn body(speed: f32, accel: f32) -> BodyParamsT {
    make!(BodyParamsT {
        values: Some(vec![speed, accel])
    })
}

fn fresh() -> AirHockey {
    let mut game = AirHockey::new(Params::default(), 3);
    game.reset(0, &[body(8.0, 60.0), body(8.0, 60.0)])
        .expect("valid bodies");
    game
}

fn action(tick: u64, player: u8, x: f32, y: f32) -> ActionT {
    make!(ActionT {
        tick: tick,
        player: player,
        values: Some(vec![x, y]),
    })
}

fn observe(game: &AirHockey, player: u8) -> ObservationT {
    let mut obs = ObservationT::default();
    game.observe(game.live(), player, &mut obs);
    obs
}

/// An asymmetric world, so that every rotation mistake shows.
fn set_scene(game: &mut AirHockey) {
    let world = &mut game.game_mut().world;
    world.puck = Puck {
        pos: Vec2::new(0.3, -0.1),
        vel: Vec2::new(1.0, 2.0),
    };
    world.mallets[0] = Mallet {
        pos: Vec2::new(-0.6, 0.2),
        vel: Vec2::new(0.5, -0.5),
        target: Vec2::new(-0.4, 0.1),
    };
    world.mallets[1] = Mallet {
        pos: Vec2::new(0.7, -0.3),
        vel: Vec2::new(-1.0, 0.25),
        target: Vec2::new(0.6, 0.0),
    };
}

fn f32s(values: &[f64]) -> Vec<f32> {
    values.iter().map(|&v| v as f32).collect()
}

#[test]
fn spec_describes_air_hockey_v0_1() {
    let spec = fresh().spec();
    assert_eq!(spec.game_id.as_deref(), Some("airhockey"));
    assert_eq!(spec.game_version, GAME_VERSION);
    assert_eq!(spec.num_players, 2);
    assert_eq!(spec.time_mode, TimeMode::Realtime);
    assert_eq!((spec.tick_hz, spec.decision_hz), (240, 60));
    let space = spec.action_space.as_ref().expect("an action space");
    let dims = space.continuous.as_ref().expect("continuous actions");
    let ranges: Vec<_> = dims.iter().map(|d| (d.min, d.max)).collect();
    assert_eq!(ranges, vec![(-1.0, 1.0), (-0.5, 0.5)]);
    assert_eq!(space.discrete, 0);
    assert_eq!(
        spec.observation.as_ref().map(Vec::len),
        Some(OBSERVATION_LEN)
    );
    assert_eq!(spec.body_params.as_ref().map(Vec::len), Some(2));
    assert_eq!(spec.signals.as_ref().map(Vec::len), Some(4));
    assert_eq!(
        spec.intent_labels
            .as_ref()
            .and_then(|l| l.first())
            .map(String::as_str),
        Some("none")
    );
    let message = MessageT::GameSpec(Box::new(spec));
    assert_eq!(decode(&encode(&message)), Ok(message));
}

#[test]
fn observation_is_rotated_for_player_one() {
    let mut game = fresh();
    set_scene(&mut game);
    let stall = 0.0;
    let p0 = observe(&game, 0);
    assert_eq!(
        p0.state,
        Some(f32s(&[
            0.3, -0.1, 1.0, 2.0, -0.6, 0.2, 0.5, -0.5, -0.4, 0.1, 0.7, -0.3, -1.0, 0.25, stall
        ]))
    );
    assert_eq!(p0.opponent_signals, Some(f32s(&[0.7, -0.3, -1.0, 0.25])));
    let p1 = observe(&game, 1);
    assert_eq!(
        p1.state,
        Some(f32s(&[
            -0.3, 0.1, -1.0, -2.0, -0.7, 0.3, 1.0, -0.25, -0.6, -0.0, 0.6, -0.2, -0.5, 0.5, stall
        ]))
    );
    assert_eq!(p1.opponent_signals, Some(f32s(&[0.6, -0.2, -0.5, 0.5])));
    assert_eq!((p0.player, p1.player), (0, 1));
    assert_eq!(p0.to_act, -1);
    assert_eq!(p0.score, Some(vec![0, 0]));
    assert_eq!(p0.legal, Some(vec![]));
}

#[test]
fn rotating_back_from_player_one_restores_the_world() {
    let mut game = fresh();
    set_scene(&mut game);
    let obs = observe(&game, 1);
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let guess = game.state_from_observation(&obs, &mut rng);
    let live = game.live();
    let near = |a: Vec2, b: Vec2| (a - b).length() < 1e-6;
    assert!(near(guess.world.puck.pos, live.world.puck.pos));
    assert!(near(guess.world.puck.vel, live.world.puck.vel));
    for i in 0..2 {
        assert!(
            near(guess.world.mallets[i].pos, live.world.mallets[i].pos),
            "{i}"
        );
        assert!(
            near(guess.world.mallets[i].vel, live.world.mallets[i].vel),
            "{i}"
        );
    }
    // Player 1 knows its own target; the opponent's is hidden and guessed as standing still.
    assert!(near(
        guess.world.mallets[1].target,
        live.world.mallets[1].target
    ));
    assert_eq!(guess.world.mallets[0].target, guess.world.mallets[0].pos);
    assert_eq!(guess.tick(), live.tick());
}

#[test]
fn player_one_action_maps_to_rotated_world_target() {
    let mut game = fresh();
    game.apply(&action(0, 1, 0.3, -0.2)).expect("valid action");
    assert_eq!(
        game.live().world.mallets[1].target,
        Vec2::new(f64::from(-0.3f32), f64::from(0.2f32))
    );
    game.apply(&action(0, 0, -0.3, 0.2)).expect("valid action");
    assert_eq!(
        game.live().world.mallets[0].target,
        Vec2::new(f64::from(-0.3f32), f64::from(0.2f32))
    );
}

#[test]
fn apply_moves_only_own_mallet() {
    let mut game = fresh();
    let before = game.live().world.mallets;
    game.apply(&action(0, 0, -0.3, 0.4)).expect("valid action");
    let mut events = Vec::new();
    for _ in 0..60 {
        game.advance(&mut events);
    }
    let after = game.live().world.mallets;
    assert_ne!(after[0].pos, before[0].pos);
    assert_eq!(after[1], before[1]);
    assert!(events.is_empty());
}

#[test]
fn bad_action_is_refused_and_target_kept() {
    let mut game = fresh();
    let before = game.digest(game.live());
    let refused: Vec<(ActionT, AdapterError)> = vec![
        (
            action(0, 0, f32::NAN, 0.0),
            AdapterError::BadAction("not finite"),
        ),
        (
            action(0, 0, 0.0, f32::INFINITY),
            AdapterError::BadAction("not finite"),
        ),
        (
            action(0, 0, f32::MAX, 0.0),
            AdapterError::BadAction("outside the action space"),
        ),
        (
            action(0, 0, 0.0, -0.51),
            AdapterError::BadAction("outside the action space"),
        ),
        (
            make!(ActionT {
                values: Some(vec![0.1])
            }),
            AdapterError::BadAction("two values"),
        ),
        (
            make!(ActionT {
                values: Some(vec![0.1, 0.1, 0.1])
            }),
            AdapterError::BadAction("two values"),
        ),
        (
            make!(ActionT { values: None }),
            AdapterError::BadAction("two values"),
        ),
        (action(0, 2, 0.0, 0.0), AdapterError::BadPlayer(2)),
        (
            action(9, 0, 0.0, 0.0),
            AdapterError::WrongTick {
                expected: 0,
                got: 9,
            },
        ),
    ];
    for (bad, why) in refused {
        assert_eq!(game.apply(&bad), Err(why), "{bad:?}");
    }
    assert_eq!(
        game.digest(game.live()),
        before,
        "a refused action changed the game"
    );
    assert_eq!(
        game.apply(&action(0, 0, -1.0, 0.5)),
        Ok(()),
        "the range's edge is legal"
    );
}

#[test]
fn reset_checks_bodies_and_applies_them() {
    let mut game = AirHockey::new(Params::default(), 1);
    for bodies in [
        vec![body(8.0, 60.0)],
        vec![body(8.0, 60.0), body(0.0, 60.0)],
        vec![body(f32::NAN, 60.0), body(8.0, 60.0)],
        vec![body(12.01, 60.0), body(8.0, 60.0)],
        vec![body(8.0, 60.0), body(8.0, 200.5)],
        vec![body(1e4, 1e4), body(8.0, 60.0)],
        vec![
            body(8.0, 60.0),
            make!(BodyParamsT {
                values: Some(vec![8.0])
            }),
        ],
    ] {
        assert_eq!(
            game.reset(1, &bodies),
            Err(AdapterError::BadBody(
                "two positive finite values per player"
            )),
            "{bodies:?}"
        );
    }
    game.reset(1, &[body(12.0, 200.0), body(12.0, 200.0)])
        .expect("the declared limits themselves are legal");
    let declared = game.spec().body_params.unwrap();
    assert_eq!((declared[0].max, declared[1].max), (12.0, 200.0));
    game.reset(1, &[body(3.0, 25.0), body(8.0, 60.0)])
        .expect("valid bodies");
    assert_eq!(game.live().seed(), 1);
    game.apply(&action(0, 0, -0.1, 0.4)).unwrap();
    let mut events = Vec::new();
    let mut top: f64 = 0.0;
    for _ in 0..240 {
        game.advance(&mut events);
        top = top.max(game.live().world.mallets[0].vel.length());
    }
    assert!(
        top <= 3.0 + 1e-9 && top > 2.5,
        "slow body not applied: top speed {top}"
    );
}

#[test]
fn simulate_matches_live_stepping_and_leaves_game_untouched() {
    let mut game = fresh();
    let actions = [action(0, 0, -0.5, 0.3), action(0, 1, 0.2, 0.1)];
    let before = game.digest(game.live());
    let simulated = game.simulate(game.live(), &actions, 120);
    assert_eq!(
        game.digest(game.live()),
        before,
        "simulate touched the live game"
    );
    let mut events = Vec::new();
    for t in 0..120 {
        for a in &actions {
            let mut stamped = a.clone();
            stamped.tick = t;
            game.apply(&stamped).unwrap();
        }
        game.advance(&mut events);
    }
    assert_eq!(game.digest(&simulated), game.digest(game.live()));
    assert_ne!(game.digest(&simulated), before);
}

#[test]
fn state_from_observation_simulates_like_the_live_game() {
    let mut game = fresh();
    game.game_mut().world.puck = Puck {
        pos: Vec2::new(0.1, 0.2),
        vel: Vec2::new(0.3, -0.4),
    };
    let actions = [action(0, 0, -0.7, -0.2), action(0, 1, 0.3, 0.2)];
    let mut rng = ChaCha8Rng::seed_from_u64(5);
    let guess = game.state_from_observation(&observe(&game, 0), &mut rng);
    let from_guess = game.simulate(&guess, &actions, 30);
    let from_live = game.simulate(game.live(), &actions, 30);
    let near = |a: Vec2, b: Vec2| (a - b).length() < 1e-5;
    assert!(near(from_guess.world.puck.pos, from_live.world.puck.pos));
    for i in 0..2 {
        assert!(
            near(
                from_guess.world.mallets[i].pos,
                from_live.world.mallets[i].pos
            ),
            "{i}"
        );
    }
    assert_ne!(
        from_live.world.mallets[0].pos,
        game.live().world.mallets[0].pos
    );
}

#[test]
fn signals_match_what_the_opponent_observes_after_the_tick() {
    for (mover, watcher) in [(0u8, 1u8), (1, 0)] {
        let mut game = fresh();
        let mut events = Vec::new();
        game.apply(&action(0, 0, -0.3, 0.1)).unwrap();
        game.apply(&action(0, 1, 0.4, -0.1)).unwrap();
        for _ in 0..10 {
            game.advance(&mut events);
        }
        let t = game.live().tick();
        let mine = action(t, mover, -0.5, 0.35);
        let mut signals = Vec::new();
        game.signals(game.live(), &mine, &mut signals);
        game.apply(&mine).unwrap();
        game.advance(&mut events);
        let seen = observe(&game, watcher).opponent_signals.unwrap();
        assert_eq!(signals, seen, "mover {mover}");
        assert_eq!(signals.len(), 4);
    }
}

#[test]
fn signals_ignore_intent_and_deceptive_flag() {
    let game = fresh();
    let mut honest = action(0, 0, -0.4, 0.2);
    honest.intent = 1;
    let mut fake = honest.clone();
    fake.intent = 3;
    fake.deceptive = true;
    let (mut a, mut b) = (Vec::new(), Vec::new());
    game.signals(game.live(), &honest, &mut a);
    game.signals(game.live(), &fake, &mut b);
    assert_eq!(a, b);
    assert!(!a.is_empty());
}

fn park_and_shoot_at_high(game: &mut AirHockey) {
    let world = &mut game.game_mut().world;
    world.mallets[0] = Mallet::at_rest(Vec2::new(-0.95, 0.45));
    world.mallets[1] = Mallet::at_rest(Vec2::new(0.95, 0.45));
    world.puck = Puck {
        pos: Vec2::new(0.8, 0.0),
        vel: Vec2::new(5.0, 0.0),
    };
}

#[test]
fn goals_become_score_events_and_seven_end_the_match() {
    let mut game = fresh();
    let mut events: Vec<EventT> = Vec::new();
    for goal in 1..=7 {
        park_and_shoot_at_high(&mut game);
        events.clear();
        for _ in 0..240 {
            game.advance(&mut events);
            if !events.is_empty() {
                break;
            }
        }
        let tick = game.live().tick() - 1;
        assert_eq!(
            events,
            vec![make!(EventT {
                tick: tick,
                kind: EventKind::Score,
                player: 0,
            })],
            "goal {goal}"
        );
        assert_eq!(game.result().is_some(), goal == 7, "goal {goal}");
    }
    let result = game.result().unwrap();
    assert_eq!(result.score, Some(vec![7, 0]));
    assert_eq!((result.level, result.seed), (3, 0));
    assert_eq!(result.duration_ticks, game.live().tick());
    assert_eq!(
        game.apply(&action(game.live().tick(), 0, 0.0, 0.0)),
        Err(AdapterError::MatchOver)
    );
}

#[test]
fn simulate_from_mid_match_holds_the_actions_whatever_their_tick() {
    let mut game = fresh();
    let mut events = Vec::new();
    for _ in 0..50 {
        game.advance(&mut events);
    }
    let planned = game.simulate(game.live(), &[action(0, 0, -0.3, 0.4)], 60);
    assert_eq!(
        planned.world.mallets[0].target,
        Vec2::new(f64::from(-0.3f32), f64::from(0.4f32))
    );
    assert_eq!(planned.tick(), game.live().tick() + 60);
}

#[test]
fn stall_becomes_a_round_end_event_for_the_receiving_player() {
    let mut game = fresh();
    {
        let world = &mut game.game_mut().world;
        world.mallets[0] = Mallet::at_rest(Vec2::new(-0.95, 0.45));
        world.mallets[1] = Mallet::at_rest(Vec2::new(0.95, 0.45));
    }
    let mut events = Vec::new();
    for _ in 0..7 * 240 {
        game.advance(&mut events);
    }
    assert_eq!(
        events,
        vec![make!(EventT {
            tick: 7 * 240 - 1,
            kind: EventKind::RoundEnd,
            player: 1,
        })]
    );
}

#[test]
fn hostile_observation_gives_a_finite_state_inside_the_rules() {
    let game = fresh();
    let params = Params::default();
    let mut obs = observe(&game, 0);
    let mut state = vec![f32::NAN; OBSERVATION_LEN];
    state[2] = f32::INFINITY;
    state[4] = 1e30;
    state[10] = -f32::MAX;
    state[14] = 1e30;
    obs.state = Some(state);
    obs.tick = u64::MAX;
    obs.score = Some(vec![i64::from(u32::MAX), -5]);
    let mut rng = ChaCha8Rng::seed_from_u64(9);
    let guess = game.state_from_observation(&obs, &mut rng);
    let world = guess.world;
    assert!(
        world.puck.pos.is_finite() && world.puck.vel.is_finite(),
        "{world:?}"
    );
    assert!(world.puck.pos.x.abs() <= 1.0 && world.puck.pos.y.abs() <= 0.5);
    assert!(world.puck.vel.length() <= params.puck_speed_cap);
    for (mallet, region) in world.mallets.iter().zip(airhockey::regions(&params)) {
        assert_eq!(region.clamp(mallet.pos), mallet.pos, "{mallet:?}");
        assert!(
            mallet.vel.is_finite() && mallet.target.is_finite(),
            "{mallet:?}"
        );
    }
    assert!(guess.score().iter().all(|&g| g <= params.goals_to_win));
    assert!(guess.stall_ticks() <= 7 * 240);
    let later = game.simulate(&guess, &[action(0, 0, -0.2, 0.1)], 30);
    assert!(later.world.puck.pos.is_finite());
}

#[test]
fn observed_winning_score_means_the_match_is_over() {
    let game = fresh();
    let mut obs = observe(&game, 1);
    obs.score = Some(vec![3, 7]);
    let mut rng = ChaCha8Rng::seed_from_u64(2);
    assert!(game.state_from_observation(&obs, &mut rng).is_over());
    obs.score = Some(vec![3, 6]);
    assert!(!game.state_from_observation(&obs, &mut rng).is_over());
}
