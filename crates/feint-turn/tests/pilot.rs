use feint_ai::Controller;
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventT, GameAdapter, GameSpecT,
    MatchResultT, ObservationT, PlayerId, TimeMode,
};
use feint_turn::{TurnPilot, TurnPolicy, sample_index, turn_layout, unit_draw};
use rand_chacha::rand_core::SeedableRng;

#[test]
fn sample_at_a_boundary_takes_the_next_action() {
    assert_eq!(sample_index(&[0.25, 0.75], &[1, 1], 0.25), Some(1));
    assert_eq!(sample_index(&[0.25, 0.75], &[1, 1], 0.249_999_98), Some(0));
    assert_eq!(sample_index(&[0.0, 0.5, 0.5], &[1, 1, 1], 0.0), Some(1));
}

#[test]
fn sampling_never_picks_an_illegal_action() {
    let p = [0.0, 0.3, 0.0, 0.7, 0.0];
    let legal = [0, 1, 0, 1, 0];
    for u in [0.0, 0.299_999_98, 0.3, 0.5, 0.999_999_94] {
        let a = sample_index(&p, &legal, u).unwrap();
        assert_eq!(legal[a], 1, "u = {u} gave {a}");
    }
    assert_eq!(sample_index(&[0.4, 0.4], &[1, 1], 0.999_999_94), Some(1));
    assert_eq!(sample_index(&[0.4, 0.4, 0.2], &[1, 1, 0], 0.9), Some(1));
}

#[test]
fn no_legal_action_samples_nothing() {
    assert_eq!(sample_index(&[0.5, 0.5], &[0, 0], 0.1), None);
    assert_eq!(sample_index(&[], &[], 0.1), None);
}

#[test]
fn sampling_follows_the_probabilities() {
    let p = [0.1, 0.2, 0.0, 0.3, 0.4];
    let legal = [1, 1, 0, 1, 1];
    let mut rng = ChaCha8Rng::seed_from_u64(2026);
    let mut counts = [0u32; 5];
    let n = 10_000;
    for _ in 0..n {
        counts[sample_index(&p, &legal, unit_draw(&mut rng)).unwrap()] += 1;
    }
    assert_eq!(counts[2], 0);
    let chi: f64 = [0, 1, 3, 4]
        .iter()
        .map(|&i| {
            let expected = f64::from(p[i]) * f64::from(n);
            let d = f64::from(counts[i]) - expected;
            d * d / expected
        })
        .sum();
    const CHI_SQUARE_3_DF_0_999: f64 = 16.266;
    assert!(chi < CHI_SQUARE_3_DF_0_999, "chi-square {chi}: {counts:?}");
}

#[test]
fn unit_draws_lie_in_zero_to_one() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    for _ in 0..10_000 {
        let u = unit_draw(&mut rng);
        assert!((0.0..1.0).contains(&u), "{u}");
    }
}

#[derive(Clone, Debug)]
struct Count {
    made: Vec<i32>,
    tick: u64,
}

struct Turns(Count);

const LAST: usize = 6;

impl GameAdapter for Turns {
    type State = Count;

    fn spec(&self) -> GameSpecT {
        let mut spec = GameSpecT::default();
        spec.game_id = Some("turns".into());
        spec.num_players = 2;
        spec.time_mode = TimeMode::TurnBased;
        let mut space = ActionSpaceT::default();
        space.discrete = 3;
        spec.action_space = Some(Box::new(space));
        spec
    }

    fn reset(&mut self, _seed: u64, _bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        self.0 = Count {
            made: vec![],
            tick: 0,
        };
        Ok(())
    }

    fn live(&self) -> &Count {
        &self.0
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.tick != self.0.tick || usize::from(action.player) != self.0.made.len() % 2 {
            return Err(AdapterError::BadAction("not now"));
        }
        self.0.made.push(action.discrete);
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {
        self.0.tick += 1;
    }

    fn observe(&self, state: &Count, player: PlayerId, out: &mut ObservationT) {
        out.tick = state.tick;
        out.player = player;
        let done = state.made.len() >= LAST;
        out.to_act = if done {
            -1
        } else {
            (state.made.len() % 2) as i8
        };
        out.state = Some(vec![state.made.len() as f32]);
        let mine = !done && usize::from(player) == state.made.len() % 2;
        out.legal = Some(vec![u8::from(mine); 3]);
    }

    fn state_from_observation(&self, _obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Count {
        self.0.clone()
    }

    fn simulate(&self, state: &Count, _actions: &[ActionT], _ticks: u32) -> Count {
        state.clone()
    }

    fn signals(&self, _state: &Count, _action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
    }

    fn digest(&self, _state: &Count) -> u64 {
        0
    }

    fn result(&self) -> Option<MatchResultT> {
        None
    }
}

fn safetensors(tensors: &[(&str, Vec<usize>, Vec<f32>)], metadata: &[(&str, String)]) -> Vec<u8> {
    let mut header = String::from("{");
    let mut data = Vec::new();
    for (name, shape, values) in tensors {
        let start = data.len();
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let dims: Vec<String> = shape.iter().map(ToString::to_string).collect();
        header += &format!(
            "\"{name}\":{{\"dtype\":\"F32\",\"shape\":[{}],\"data_offsets\":[{start},{}]}},",
            dims.join(","),
            data.len()
        );
    }
    let meta: Vec<String> = metadata
        .iter()
        .map(|(k, v)| format!("\"{k}\":\"{v}\""))
        .collect();
    header += &format!("\"__metadata__\":{{{}}}}}", meta.join(","));
    while (8 + header.len()) % 8 != 0 {
        header.push(' ');
    }
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&data);
    out
}

fn policy() -> TurnPolicy {
    let layout = turn_layout("turns", 1, 3);
    let bytes = safetensors(
        &[
            ("layer0.weight", vec![2, 1], vec![1.0, -1.0]),
            ("layer0.bias", vec![2], vec![0.5, 0.5]),
            ("layer1.weight", vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]),
            ("layer1.bias", vec![2], vec![0.0, 0.0]),
            (
                "layer2.weight",
                vec![3, 2],
                vec![0.3, -0.2, -0.1, 0.4, 0.0, 0.0],
            ),
            ("layer2.bias", vec![3], vec![0.0, 0.1, 0.2]),
        ],
        &[
            ("format", "feint-mlp-v1".into()),
            ("layout", layout.clone()),
            ("features", "1".into()),
            ("outputs", "3".into()),
            ("hidden", "2,2".into()),
            ("activation", "relu".into()),
            ("inv_scales", "1".into()),
        ],
    );
    TurnPolicy::from_safetensors(&bytes, &layout).unwrap()
}

fn play(seed: u64, idle_asks: bool) -> Vec<ActionT> {
    let mut game = Turns(Count {
        made: vec![],
        tick: 0,
    });
    let spec = game.spec();
    game.reset(seed, &[]).unwrap();
    let mut pilots = [TurnPilot::new(policy()), TurnPilot::new(policy())];
    for (me, pilot) in pilots.iter_mut().enumerate() {
        Controller::<Turns>::new_match(pilot, &spec, me as PlayerId, seed);
    }
    let mut obs = ObservationT::default();
    let mut actions = Vec::new();
    let mut events = Vec::new();
    while game.live().made.len() < LAST {
        for (me, pilot) in pilots.iter_mut().enumerate() {
            game.observe(game.live(), me as PlayerId, &mut obs);
            let mine = obs.to_act == me as i8;
            if !mine && !idle_asks {
                continue;
            }
            match pilot.on_tick(&game, &obs) {
                Some(action) => {
                    assert!(mine, "seat {me} acted out of turn");
                    game.apply(&action).unwrap();
                    actions.push(action);
                    break;
                }
                None => assert!(!mine, "seat {me} did not act on its turn"),
            }
        }
        game.advance(&mut events);
    }
    actions
}

#[test]
fn the_pilot_acts_only_on_its_turn_with_its_tick_and_seat() {
    let actions = play(5, true);
    assert_eq!(actions.len(), LAST);
    for (i, action) in actions.iter().enumerate() {
        assert_eq!(usize::from(action.player), i % 2);
        assert_eq!(action.tick, i as u64);
        assert!((0..3).contains(&action.discrete));
    }
}

#[test]
fn one_draw_per_acting_turn_whatever_the_idle_ticks() {
    assert_eq!(play(7, true), play(7, false));
}

#[test]
fn the_same_seed_gives_the_same_match_and_seeds_differ() {
    let a: Vec<i32> = play(11, false).iter().map(|a| a.discrete).collect();
    assert_eq!(
        a,
        play(11, false)
            .iter()
            .map(|a| a.discrete)
            .collect::<Vec<_>>()
    );
    let others: Vec<Vec<i32>> = (12..20)
        .map(|s| play(s, false).iter().map(|a| a.discrete).collect())
        .collect();
    assert!(others.iter().any(|o| *o != a));
}

#[test]
fn seats_get_different_streams() {
    let mut pilot = TurnPilot::new(policy());
    let spec = Turns(Count {
        made: vec![],
        tick: 0,
    })
    .spec();
    Controller::<Turns>::new_match(&mut pilot, &spec, 0, 42);
    let first = pilot.stream_seed();
    Controller::<Turns>::new_match(&mut pilot, &spec, 1, 42);
    let second = pilot.stream_seed();
    assert_ne!(first, second);
    assert_ne!(first, 42);
}

#[test]
fn a_turn_without_a_legal_action_neither_acts_nor_draws() {
    let game = Turns(Count {
        made: vec![],
        tick: 0,
    });
    let spec = game.spec();
    let mut asked = TurnPilot::new(policy());
    let mut fresh = TurnPilot::new(policy());
    Controller::<Turns>::new_match(&mut asked, &spec, 0, 9);
    Controller::<Turns>::new_match(&mut fresh, &spec, 0, 9);
    let mut blocked = ObservationT::default();
    blocked.to_act = 0;
    blocked.state = Some(vec![0.0]);
    blocked.legal = Some(vec![0, 0, 0]);
    for _ in 0..5 {
        assert!(asked.on_tick(&game, &blocked).is_none());
    }
    let mut open = blocked.clone();
    open.legal = Some(vec![1, 1, 1]);
    let picks: Vec<i32> = (0..20)
        .map(|_| asked.on_tick(&game, &open).unwrap().discrete)
        .collect();
    let fresh_picks: Vec<i32> = (0..20)
        .map(|_| fresh.on_tick(&game, &open).unwrap().discrete)
        .collect();
    assert_eq!(picks, fresh_picks);
}
