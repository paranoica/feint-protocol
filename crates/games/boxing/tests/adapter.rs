use boxing::human::{Human, human_body};
use boxing::{
    ACTION_LEN, Boxing, BoxingPlaybook, Height, Kind, OBSERVATION_LEN, Params, Phase, Stage, order,
};
use feint_ai::{Bot, Difficulty, Opponent, RunHeader, run_match};
use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventKind, GameAdapter, ObservationT,
};
use rand_chacha::rand_core::SeedableRng;

fn body(speed: f32, hand: f32) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(vec![speed, hand]);
    b
}

fn fresh() -> Boxing {
    let mut game = Boxing::new(Params::default(), 3);
    let b = human_body(&game);
    game.reset(4, &[b.clone(), b]).unwrap();
    game
}

fn send(game: &mut Boxing, mut a: ActionT) -> Result<(), AdapterError> {
    a.tick = game.live().tick;
    game.apply(&a)
}

fn step(game: &mut Boxing, ticks: u32) -> Vec<feint_protocol::EventT> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        game.advance(&mut events);
    }
    events
}

fn mid_exchange() -> Boxing {
    let mut game = fresh();
    send(&mut game, order(0, 0.0, Height::Low, None, false)).unwrap();
    send(&mut game, order(1, 0.0, Height::High, None, false)).unwrap();
    step(&mut game, 120);
    send(
        &mut game,
        order(1, 0.0, Height::Low, Some((Kind::Hook, Height::High)), false),
    )
    .unwrap();
    send(
        &mut game,
        order(0, 0.0, Height::High, Some((Kind::Jab, Height::Low)), false),
    )
    .unwrap();
    step(&mut game, 30);
    game
}

#[test]
fn the_spec_declares_what_observe_and_apply_use() {
    let game = fresh();
    let spec = game.spec();
    assert_eq!(spec.game_id.as_deref(), Some("boxing"));
    assert_eq!(spec.observation.unwrap().len(), OBSERVATION_LEN);
    let space = spec.action_space.unwrap();
    assert_eq!(space.continuous.unwrap().len(), ACTION_LEN);
    assert_eq!(spec.body_params.unwrap().len(), 2);
    assert_eq!(spec.signals.unwrap().len(), 3);
}

#[test]
fn each_seat_sees_itself_facing_forward() {
    let game = mid_exchange();
    let s = game.live();
    let mut a = ObservationT::default();
    let mut b = ObservationT::default();
    game.observe(s, 0, &mut a);
    game.observe(s, 1, &mut b);
    let (a, b) = (a.state.unwrap(), b.state.unwrap());
    assert_eq!(a.len(), OBSERVATION_LEN);
    assert_eq!(b.len(), OBSERVATION_LEN);
    assert_eq!(a[0], s.fighters[0].x as f32);
    assert_eq!(b[0], -s.fighters[1].x as f32);
    assert!(a[1] > a[0] && b[1] > b[0], "the opponent is in front");
    assert_eq!(a[2..9], b[9..16], "my fighter is their opponent");
    assert_eq!(a[9..16], b[2..9]);
    assert_eq!(a[2], 1.0, "seat 0 is winding up");
    assert_eq!(b[2], 1.0, "seat 1 is winding up");
    assert_eq!(a[3], 1.0, "a jab");
    assert_eq!(b[3], 2.0, "a hook");
}

#[test]
fn the_signal_is_the_visible_wind_up() {
    let game = mid_exchange();
    let mut obs = ObservationT::default();
    game.observe(game.live(), 0, &mut obs);
    assert_eq!(
        obs.opponent_signals.clone().unwrap()[..2],
        [1.0, 2.0],
        "their hook winds up high"
    );
    let game = fresh();
    game.observe(game.live(), 0, &mut obs);
    assert_eq!(obs.opponent_signals.clone().unwrap()[..2], [0.0, 0.0]);
}

#[test]
fn a_state_read_back_from_an_observation_observes_the_same() {
    let game = mid_exchange();
    let s = game.live();
    for seat in [0u8, 1] {
        let mut obs = ObservationT::default();
        game.observe(s, seat, &mut obs);
        let back = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0));
        let mut again = ObservationT::default();
        game.observe(&back, seat, &mut again);
        assert_eq!(obs.state, again.state, "seat {seat}");
        assert_eq!(obs.score, again.score);
        assert_eq!(back.fighters[1].stage, s.fighters[1].stage);
        assert_eq!(back.fighters[0].queued, None, "an intention is not visible");
    }
}

#[test]
fn a_hostile_observation_reads_back_inside_the_ring() {
    let game = fresh();
    let mut obs = ObservationT::default();
    game.observe(game.live(), 0, &mut obs);
    let mut v = obs.state.clone().unwrap();
    v[0] = f32::NAN;
    v[1] = 1e30;
    v[5] = -1e30;
    v[8] = 1e30;
    v[17] = 1e30;
    obs.state = Some(v);
    obs.score = Some(vec![-5, i64::from(Params::default().points_to_win) + 1]);
    obs.tick = u64::MAX;
    let back = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0));
    let p = Params::default();
    for f in &back.fighters {
        assert!(f.x.abs() <= p.ring && f.x.is_finite());
        assert!(f.switching <= p.guard_switch);
    }
    assert!(back.gap() >= p.min_gap - 1e-9);
    assert!(back.phase_left <= p.break_ticks);
    assert_eq!(back.score, [0, p.points_to_win]);
    assert_eq!(back.tick, p.match_ticks());
    assert!(game.simulate(&back, &[], 4).over());
}

#[test]
fn seat_one_moves_in_its_own_frame() {
    let start = Params::default().start_gap / 2.0;
    let mut game = fresh();
    let gap = game.live().gap();
    send(&mut game, order(1, 0.0, Height::High, None, false)).unwrap();
    step(&mut game, 48);
    assert!(game.live().gap() < gap, "forward closes the gap");
    let mut game = fresh();
    send(&mut game, order(1, -2.0, Height::High, None, false)).unwrap();
    step(&mut game, 48);
    assert!(game.live().gap() > gap, "backward opens it");
    assert!(game.live().fighters[1].x > start);
}

#[test]
fn malformed_actions_are_refused() {
    let mut game = fresh();
    let p = Params::default();
    let mut short = order(0, 0.0, Height::High, None, false);
    short.values = Some(vec![0.0; ACTION_LEN - 1]);
    let mut nan = order(0, 0.0, Height::High, None, false);
    nan.values.as_mut().unwrap()[1] = f32::NAN;
    let mut none = order(0, 0.0, Height::High, None, false);
    none.values = None;
    let outside = |i: usize, v: f32| {
        let mut a = order(0, 0.0, Height::High, None, false);
        a.values.as_mut().unwrap()[i] = v;
        a
    };
    for bad in [
        short,
        nan,
        none,
        outside(0, p.ring as f32 + 0.01),
        outside(1, 1.01),
        outside(2, 2.01),
        outside(2, -0.01),
        outside(3, -1.01),
        outside(4, 1.01),
    ] {
        assert!(matches!(
            send(&mut game, bad),
            Err(AdapterError::BadAction(_))
        ));
    }
    let mut stranger = order(2, 0.0, Height::High, None, false);
    stranger.player = 2;
    assert_eq!(send(&mut game, stranger), Err(AdapterError::BadPlayer(2)));
    let mut late = order(0, 0.0, Height::High, None, false);
    late.tick = 5;
    assert!(matches!(
        game.apply(&late),
        Err(AdapterError::WrongTick { .. })
    ));
    assert_eq!(
        send(&mut game, order(0, p.ring, Height::Low, None, false)),
        Ok(()),
        "the edge of the space is inside"
    );
}

#[test]
fn bodies_outside_the_declared_range_are_refused() {
    let mut game = Boxing::new(Params::default(), 1);
    let p = Params::default();
    let good = body(p.max_speed as f32, p.max_hand as f32);
    for bad in [
        body(0.0, 1.0),
        body(p.max_speed as f32 + 0.1, 1.0),
        body(1.0, p.min_hand as f32 - 0.01),
        body(1.0, p.max_hand as f32 + 0.01),
        body(f32::NAN, 1.0),
        {
            let mut b = body(1.0, 1.0);
            b.values = Some(vec![1.0, 1.0, 1.0]);
            b
        },
    ] {
        assert!(matches!(
            game.reset(1, &[good.clone(), bad]),
            Err(AdapterError::BadBody(_))
        ));
    }
    assert!(game.reset(1, std::slice::from_ref(&good)).is_err());
    assert_eq!(game.reset(1, &[good, body(1.0, p.min_hand as f32)]), Ok(()));
}

#[test]
fn a_hit_is_a_score_event_and_a_round_end_is_announced() {
    let mut game = fresh();
    let p = Params::default();
    send(&mut game, order(0, 0.5, Height::High, None, false)).unwrap();
    send(&mut game, order(1, 0.5, Height::Low, None, false)).unwrap();
    step(&mut game, 120);
    send(
        &mut game,
        order(0, 0.5, Height::High, Some((Kind::Jab, Height::High)), false),
    )
    .unwrap();
    send(&mut game, order(1, 0.5, Height::Low, None, false)).unwrap();
    let events = step(&mut game, p.jab_windup + 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::Score);
    assert_eq!(events[0].player, 0);
    assert_eq!(game.live().phase, Phase::Break);
    let mut game = fresh();
    let events = step(&mut game, p.round_ticks);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::RoundEnd);
    assert_eq!(events[0].tick, u64::from(p.round_ticks) - 1);
}

#[test]
fn simulate_matches_the_live_game() {
    let game = mid_exchange();
    let actions = [
        order(0, 0.0, Height::Low, None, false),
        order(1, 0.0, Height::Low, None, true),
    ];
    let sim = game.simulate(game.live(), &actions, 200);
    let mut live = mid_exchange();
    for _ in 0..200 {
        for a in &actions {
            send(&mut live, a.clone()).unwrap();
        }
        step(&mut live, 1);
    }
    assert_eq!(sim.digest(), live.live().digest());
    assert_eq!(live.live().fighters[1].feints, 1);
    assert_eq!(live.live().fighters[1].stage, Stage::Idle);
}

#[test]
fn a_bot_match_against_the_human_model_is_deterministic() {
    let play = || {
        let mut game = Boxing::new(Params::default(), 4);
        let spec = game.spec();
        let d = Difficulty::preset(4).unwrap();
        let mut bot = Bot::new(d, BoxingPlaybook::new(Params::default()), &spec).unwrap();
        let mut human = Human::new();
        let header = RunHeader {
            game_id: "boxing".into(),
            game_version: boxing::GAME_VERSION,
            seed: 11,
            bodies: vec![d.body(&spec), human_body(&game)],
            levels: [4, 0],
            opponent: Opponent::HandMade,
        };
        let report = run_match(&mut game, [&mut bot, &mut human], header, 240 * 600).unwrap();
        (report.digest, game.live().score, game.live().over())
    };
    let first = play();
    assert_eq!(first, play());
    assert!(first.2, "the match ends");
    assert!(first.1[0] + first.1[1] > 0, "someone lands a punch");
}

#[test]
fn a_cancel_is_visible_and_reads_back() {
    let mut game = mid_exchange();
    send(&mut game, order(1, 0.0, Height::Low, None, true)).unwrap();
    send(&mut game, order(0, 0.0, Height::High, None, false)).unwrap();
    step(&mut game, 1);
    assert!(game.live().fighters[1].cancelled);
    for seat in [0u8, 1] {
        let mut obs = ObservationT::default();
        game.observe(game.live(), seat, &mut obs);
        let back = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0));
        assert!(back.fighters[1].cancelled, "seat {seat}");
        assert!(!back.fighters[0].cancelled, "seat {seat}");
    }
}
