use feint_ai::Controller;
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventT, GameAdapter,
    GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};
use feint_turn::{TurnBatch, TurnError, TurnGame};

#[derive(Clone, Debug, PartialEq)]
struct Picks {
    taken: Vec<i32>,
    first: u8,
    tick: u64,
}

impl Picks {
    fn to_act(&self) -> Option<u8> {
        (self.taken.len() < 4).then(|| (self.first + self.taken.len() as u8) % 2)
    }

    fn score(&self) -> [i64; 2] {
        let mut sums = [0i64; 2];
        for (i, &pick) in self.taken.iter().enumerate() {
            sums[usize::from((self.first + i as u8) % 2)] += i64::from(pick);
        }
        [sums[0] - sums[1], sums[1] - sums[0]]
    }
}

struct Pick {
    state: Picks,
}

fn dim(min: f32, max: f32) -> DimT {
    let mut d = DimT::default();
    d.min = min;
    d.max = max;
    d
}

impl GameAdapter for Pick {
    type State = Picks;

    fn spec(&self) -> GameSpecT {
        let mut spec = GameSpecT::default();
        spec.game_id = Some("pick".into());
        spec.time_mode = TimeMode::TurnBased;
        let mut space = ActionSpaceT::default();
        space.discrete = 3;
        spec.action_space = Some(Box::new(space));
        spec.observation = Some(vec![dim(0.0, 4.0), dim(-3.0, 2.0), dim(0.0, 0.0)]);
        spec
    }

    fn reset(&mut self, seed: u64, _bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        self.state = Picks {
            taken: vec![],
            first: (seed % 2) as u8,
            tick: 0,
        };
        Ok(())
    }

    fn live(&self) -> &Picks {
        &self.state
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.tick != self.state.tick {
            return Err(AdapterError::WrongTick {
                expected: self.state.tick,
                got: action.tick,
            });
        }
        if self.state.to_act() != Some(action.player) {
            return Err(AdapterError::BadAction("not this player's turn"));
        }
        if !(0..3).contains(&action.discrete)
            || (action.discrete == 2 && self.state.taken.is_empty())
        {
            return Err(AdapterError::BadAction("illegal pick"));
        }
        self.state.taken.push(action.discrete);
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {
        self.state.tick += 1;
    }

    fn observe(&self, state: &Picks, player: PlayerId, out: &mut ObservationT) {
        out.tick = state.tick;
        out.player = player;
        out.to_act = state.to_act().map_or(-1, |p| p as i8);
        out.state = Some(vec![
            state.taken.len() as f32,
            state.taken.last().map_or(-1.0, |&p| p as f32),
            f32::from(player),
        ]);
        let mine = state.to_act() == Some(player);
        let opening = state.taken.is_empty();
        out.legal = Some(vec![
            u8::from(mine),
            u8::from(mine),
            u8::from(mine && !opening),
        ]);
        out.score = Some(state.score().to_vec());
    }

    fn state_from_observation(&self, _obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Picks {
        self.state.clone()
    }

    fn simulate(&self, state: &Picks, _actions: &[ActionT], _ticks: u32) -> Picks {
        state.clone()
    }

    fn signals(&self, _state: &Picks, _action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
    }

    fn digest(&self, _state: &Picks) -> u64 {
        0
    }

    fn result(&self) -> Option<MatchResultT> {
        self.state.to_act().is_none().then(|| {
            let mut r = MatchResultT::default();
            r.score = Some(self.state.score().to_vec());
            r
        })
    }
}

struct Zero {
    me: PlayerId,
}

impl Controller<Pick> for Zero {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, _adapter: &Pick, obs: &ObservationT) -> Option<ActionT> {
        (obs.to_act == self.me as i8).then(|| {
            let mut a = ActionT::default();
            a.tick = obs.tick;
            a.player = self.me;
            a.discrete = 0;
            a
        })
    }
}

impl TurnGame for Pick {
    fn fresh() -> Self {
        Pick {
            state: Picks {
                taken: vec![],
                first: 0,
                tick: 0,
            },
        }
    }

    fn reward(score: &[i64], me: usize) -> f32 {
        score.get(me).map_or(0.0, |&s| s as f32)
    }

    fn scripted(name: &str) -> Option<Box<dyn Controller<Self>>> {
        (name == "zero").then(|| Box::new(Zero { me: 0 }) as Box<dyn Controller<Self>>)
    }
}

#[test]
fn shapes_follow_the_spec() {
    let batch = TurnBatch::<Pick>::new(&[["learner", "zero"], ["learner", "frozen:3"]]).unwrap();
    assert_eq!(batch.envs(), 2);
    assert_eq!(batch.features(), 3);
    assert_eq!(batch.actions(), 3);
    assert_eq!(batch.scales(), vec![4.0, 3.0, 1.0]);
    assert_eq!(
        batch.seat_names()[1],
        ["learner".to_string(), "frozen:3".to_string()]
    );
}

#[test]
fn unknown_and_doubly_scripted_seats_are_refused() {
    assert!(matches!(
        TurnBatch::<Pick>::new(&[["learner", "nobody"]]),
        Err(TurnError::Seat(name)) if name == "nobody"
    ));
    assert!(matches!(
        TurnBatch::<Pick>::new(&[["zero", "zero"]]),
        Err(TurnError::BothScripted(0))
    ));
}

#[test]
fn the_batch_plays_scripted_seats_until_a_python_slot_is_ready() {
    let mut batch = TurnBatch::<Pick>::new(&[["zero", "learner"]]).unwrap();
    let out = batch.reset(&[0]).unwrap();
    assert_eq!(
        out.ready,
        vec![0, 1],
        "player 0 is scripted and moved first"
    );
    assert_eq!(&out.obs[3..6], &[1.0, 0.0, 1.0]);
    assert_eq!(&out.legal[3..6], &[1, 1, 1]);
    assert_eq!(&out.legal[0..3], &[0, 0, 0]);
    let out = batch.step(&[-1, 2]).unwrap();
    assert_eq!(
        out.ready,
        vec![0, 1],
        "the scripted reply came back at once"
    );
    assert_eq!(out.obs[3], 3.0);
    assert_eq!(out.terminated, vec![0]);
}

#[test]
fn rewards_arrive_once_at_the_end() {
    let mut batch = TurnBatch::<Pick>::new(&[["learner", "learner"], ["zero", "learner"]]).unwrap();
    let out = batch.reset(&[1, 0]).unwrap();
    assert_eq!(
        out.ready,
        vec![0, 1, 0, 1],
        "seed 1 lets player 1 open in env 0"
    );
    let mut rewards = [0.0f32; 4];
    let mut ends = [0u8; 2];
    let mut out = out;
    for _ in 0..6 {
        let actions: Vec<i32> = out
            .ready
            .iter()
            .enumerate()
            .map(|(slot, &r)| {
                if r == 1 {
                    if out.legal[slot * 3 + 2] == 1 { 2 } else { 1 }
                } else {
                    -1
                }
            })
            .collect();
        out = batch.step(&actions).unwrap();
        for (total, r) in rewards.iter_mut().zip(&out.reward) {
            *total += r;
        }
        for (end, t) in ends.iter_mut().zip(&out.terminated) {
            *end += t;
        }
    }
    assert_eq!(ends, [1, 1], "each match ends exactly once");
    assert_eq!(rewards, [1.0, -1.0, -4.0, 4.0]);
    assert_eq!(out.ready, vec![0, 0, 0, 0]);
    assert_eq!(out.score[2..4], [-4, 4]);
}

#[test]
fn an_illegal_python_action_is_refused_before_anything_moves() {
    let mut batch =
        TurnBatch::<Pick>::new(&[["learner", "learner"], ["learner", "learner"]]).unwrap();
    let before = batch.reset(&[0, 0]).unwrap();
    assert_eq!(
        batch.step(&[0, -1, 2, -1]),
        Err(TurnError::Illegal {
            env: 1,
            slot: 0,
            action: 2
        })
    );
    let after = batch.step(&[-1, -1, -1, -1]).unwrap();
    assert_eq!(after.obs, before.obs, "the refused step moved a game");
    assert_eq!(
        batch.step(&[0]),
        Err(TurnError::Actions {
            expected: 4,
            got: 1
        })
    );
    assert_eq!(
        batch.reset(&[1]),
        Err(TurnError::Seeds {
            expected: 2,
            got: 1
        })
    );
}

#[test]
fn a_ready_python_slot_must_act() {
    let mut batch = TurnBatch::<Pick>::new(&[["learner", "learner"]]).unwrap();
    batch.reset(&[0]).unwrap();
    let out = batch.step(&[-1, -1]).unwrap();
    assert_eq!(out.ready, vec![1, 0], "skipping keeps the slot ready");
    assert_eq!(out.obs[0], 0.0);
}
