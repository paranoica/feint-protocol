use airhockey::{AirHockey, HockeyPlaybook, Params, human::Reader};
use feint_ai::{
    Anticipation, BotError, Controller, Difficulty, RunHeader, Runner, delay_ticks, run_match,
};
use feint_ml::{Pilot, Policy};
use feint_protocol::{ActionT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::rand_core::SeedableRng;

#[derive(Clone)]
struct Fixed(Vec<f32>);

impl Policy for Fixed {
    fn act(&mut self, _features: &[f32], out: &mut [f32]) {
        out.copy_from_slice(&self.0);
    }
}

#[derive(Clone)]
struct Wander(u32);

impl Policy for Wander {
    fn act(&mut self, features: &[f32], out: &mut [f32]) {
        self.0 = self.0.wrapping_add(1);
        let swing = (self.0 % 7) as f32 / 7.0 - 0.5;
        out[0] = features[0] / 1.2 + swing;
        out[1] = features[1] / 0.6 - swing;
    }
}

type HockeyPilot<Q> = Pilot<AirHockey, HockeyPlaybook, Q>;

fn pilot<Q: Policy>(policy: Q, reaction_ms: u32) -> HockeyPilot<Q> {
    let game = AirHockey::new(Params::default(), 0);
    Pilot::new(
        HockeyPlaybook::new(Params::default()),
        policy,
        &game.spec(),
        reaction_ms,
        0.8,
    )
    .expect("a realtime spec")
}

fn header(game: &AirHockey, seed: u64) -> RunHeader {
    let spec = game.spec();
    let body = Difficulty::preset(5).expect("level 5").body(&spec);
    RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: vec![body.clone(), body],
        levels: [0, 0],
        opponent: feint_ai::Opponent::HandMade,
    }
}

struct Tap<'a, Q: Policy> {
    pilot: &'a mut HockeyPilot<Q>,
    seen: Vec<ObservationT>,
    features: Vec<(u64, Vec<f32>)>,
    actions: Vec<ActionT>,
    puck: Vec<(u64, [f64; 2])>,
}

impl<'a, Q: Policy> Tap<'a, Q> {
    fn new(pilot: &'a mut HockeyPilot<Q>) -> Self {
        Self {
            pilot,
            seen: Vec::new(),
            features: Vec::new(),
            actions: Vec::new(),
            puck: Vec::new(),
        }
    }
}

impl<Q: Policy> Controller<AirHockey> for Tap<'_, Q> {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.pilot.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push(obs.clone());
        let puck = game.live().world.puck.pos;
        self.puck.push((obs.tick, [puck.x, puck.y]));
        let action = self.pilot.on_tick(game, obs);
        if let Some(action) = &action {
            self.features
                .push((action.tick, self.pilot.features().to_vec()));
            self.actions.push(action.clone());
        }
        action
    }
}

struct Played {
    game: AirHockey,
    seen: Vec<ObservationT>,
    features: Vec<(u64, Vec<f32>)>,
    actions: Vec<ActionT>,
}

fn play<Q: Policy>(pilot: &mut HockeyPilot<Q>, seat: usize, ticks: u64) -> Played {
    let mut game = AirHockey::new(Params::default(), 0);
    let mut other = Reader::new(Params::default(), true);
    let header = header(&game, 7);
    let mut tap = Tap::new(pilot);
    if seat == 0 {
        run_match(&mut game, [&mut tap, &mut other], header, ticks)
    } else {
        run_match(&mut game, [&mut other, &mut tap], header, ticks)
    }
    .expect("the match runs");
    Played {
        game,
        seen: tap.seen,
        features: tap.features,
        actions: tap.actions,
    }
}

#[test]
fn features_are_the_anticipated_view_plus_reaction_and_body() {
    for seat in [0, 1] {
        let reaction_ms = 170;
        let delay = delay_ticks(reaction_ms, 240) as u64;
        let mut pilot = pilot(Wander(0), reaction_ms);
        let Played {
            game,
            seen,
            features,
            actions,
        } = play(&mut pilot, seat, 900);
        assert!(features.len() > 150, "seat {seat}: {}", features.len());
        for (tick, got) in features.iter().skip(100).step_by(17) {
            let delayed = &seen[(tick - delay) as usize];
            assert_eq!(delayed.tick, tick - delay);
            let mut own = Anticipation::new(delay);
            for action in actions.iter().filter(|a| a.tick < *tick) {
                own.remember(action, action.tick);
            }
            let playbook = HockeyPlaybook::new(Params::default());
            let state = game.state_from_observation(delayed, &mut ChaCha8Rng::seed_from_u64(0));
            let them = 1 - seat as PlayerId;
            let state = own.roll_forward(&game, &playbook, them, state, delayed.tick, *tick);
            let mut expected = ObservationT::default();
            game.observe(&state, seat as PlayerId, &mut expected);
            let mut expected = expected.state.expect("an observation");
            expected.extend([0.17, 0.8]);
            assert_eq!(got, &expected, "seat {seat} tick {tick}");
        }
    }
}

#[test]
fn seat_one_sees_the_board_turned_through_observe() {
    for seat in [0, 1] {
        let mut game = AirHockey::new(Params::default(), 0);
        let mut other = Reader::new(Params::default(), false);
        let mut pilot = pilot(Wander(0), 0);
        let mut tap = Tap::new(&mut pilot);
        let header = header(&game, 3);
        if seat == 0 {
            run_match(&mut game, [&mut tap, &mut other], header, 600)
        } else {
            run_match(&mut game, [&mut other, &mut tap], header, 600)
        }
        .expect("the match runs");
        let sign = if seat == 0 { 1.0 } else { -1.0 };
        let mut moving = 0;
        for (tick, features) in tap.features.iter().skip(20) {
            let (_, puck) = tap.puck.iter().find(|(t, _)| t == tick).expect("seen");
            assert_eq!(
                features[0],
                (sign * puck[0]) as f32,
                "seat {seat} tick {tick}"
            );
            assert_eq!(
                features[1],
                (sign * puck[1]) as f32,
                "seat {seat} tick {tick}"
            );
            assert!(features[4] < 0.0, "seat {seat} own half: {}", features[4]);
            moving += usize::from(puck[0] != 0.0);
        }
        assert!(
            moving > 50,
            "seat {seat}: the puck moved in {moving} samples"
        );
    }
}

#[test]
fn outputs_map_onto_the_table_and_are_clamped_as_the_bot_clamps() {
    let spec = AirHockey::new(Params::default(), 0).spec();
    let dims = spec
        .action_space
        .as_ref()
        .unwrap()
        .continuous
        .clone()
        .unwrap();
    let (l, w) = (dims[0].max, dims[1].max);
    assert!(l > 0.0 && w > 0.0 && dims[0].min == -l && dims[1].min == -w);
    for (out, expected) in [
        ([2.0, -5.0], [l, -w]),
        ([1.0, -1.0], [l, -w]),
        ([0.5, -0.25], [0.5 * l, -0.25 * w]),
        ([f32::NAN, f32::INFINITY], [0.0, w]),
    ] {
        let mut pilot = pilot(Fixed(out.to_vec()), 150);
        let actions = play(&mut pilot, 0, 60).actions;
        let first = actions.first().expect("an action within 60 ticks");
        assert_eq!(first.values.as_deref(), Some(&expected[..]), "{out:?}");
        assert_eq!((first.player, first.intent, first.deceptive), (0, 0, false));
    }
}

#[test]
fn a_zero_output_is_a_positive_zero() {
    let mut pilot = pilot(Fixed(vec![-0.0, -0.0]), 150);
    let actions = play(&mut pilot, 1, 60).actions;
    let values = actions[0].values.clone().expect("values");
    assert_eq!(
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [0, 0]
    );
}

#[test]
fn the_pilot_acts_on_decision_ticks_once_its_reaction_has_passed() {
    let mut pilot = pilot(Fixed(vec![0.1, 0.1]), 150);
    let actions = play(&mut pilot, 0, 400).actions;
    assert_eq!(actions[0].tick, 36);
    assert!(actions.iter().all(|a| a.tick % 4 == 0));
    assert_eq!(actions.len(), (400 - 36) / 4);
}

#[test]
fn sensing_ahead_of_the_tick_does_not_change_the_play() {
    let reference = {
        let mut pilot = pilot(Wander(0), 200);
        play(&mut pilot, 1, 1200).actions
    };
    let mut game = AirHockey::new(Params::default(), 0);
    let mut other = Reader::new(Params::default(), true);
    let mut pilot = pilot(Wander(0), 200);
    let header = header(&game, 7);
    let mut runner = Runner::start(&mut game, [&mut other, &mut pilot], header).expect("starts");
    let mut sensed = 0;
    while runner.ticks() < 1200 {
        let mut obs = ObservationT::default();
        game.observe(game.live(), 1, &mut obs);
        if let Some(first) = pilot.sense(&game, &obs).map(<[f32]>::to_vec) {
            assert_eq!(pilot.sense(&game, &obs), Some(&first[..]));
            sensed += 1;
        }
        if !runner.step(&mut game, [&mut other, &mut pilot]) {
            break;
        }
    }
    assert_eq!(sensed, reference.len());
    assert_eq!(
        runner
            .actions()
            .iter()
            .filter(|a| a.player == 1)
            .cloned()
            .collect::<Vec<_>>(),
        reference
    );
}

#[test]
fn a_turn_based_game_has_no_pilot() {
    let mut spec = AirHockey::new(Params::default(), 0).spec();
    spec.time_mode = feint_protocol::TimeMode::TurnBased;
    let made = Pilot::<AirHockey, _, _>::new(
        HockeyPlaybook::new(Params::default()),
        Fixed(vec![0.0, 0.0]),
        &spec,
        150,
        1.0,
    );
    assert!(matches!(made, Err(BotError::BadCadence)));
}
