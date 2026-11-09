#![allow(dead_code)]

use feint_ai::{Choice, Playbook};
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventT, GameAdapter, GameSpecT,
    MatchResultT, ObservationT, PlayerId, TimeMode,
};

pub const LEGAL: [u8; 3] = [1, 0, 1];

#[derive(Clone, Debug, PartialEq)]
pub struct PickState {
    pub tick: u64,
    pub picks: Vec<i32>,
}

impl PickState {
    pub fn to_act(&self) -> PlayerId {
        (self.picks.len() % 2) as PlayerId
    }
}

pub struct Pick {
    pub state: PickState,
}

pub fn pick_spec() -> GameSpecT {
    let mut spec = GameSpecT::default();
    spec.game_id = Some("pick".into());
    spec.time_mode = TimeMode::TurnBased;
    let mut space = ActionSpaceT::default();
    space.discrete = 3;
    spec.action_space = Some(Box::new(space));
    spec
}

impl GameAdapter for Pick {
    type State = PickState;

    fn spec(&self) -> GameSpecT {
        pick_spec()
    }

    fn reset(&mut self, _seed: u64, _bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        self.state = PickState {
            tick: 0,
            picks: vec![],
        };
        Ok(())
    }

    fn live(&self) -> &PickState {
        &self.state
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.player != self.state.to_act() {
            return Err(AdapterError::BadAction("not this player's turn"));
        }
        let legal = usize::try_from(action.discrete)
            .ok()
            .and_then(|i| LEGAL.get(i))
            .is_some_and(|&ok| ok == 1);
        if !legal {
            return Err(AdapterError::BadAction("illegal pick"));
        }
        self.state.picks.push(action.discrete);
        self.state.tick += 1;
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {}

    fn observe(&self, state: &PickState, player: PlayerId, out: &mut ObservationT) {
        out.tick = state.tick;
        out.player = player;
        out.to_act = state.to_act() as i8;
        let mine = state.to_act() == player;
        out.legal = Some(LEGAL.map(|ok| if mine { ok } else { 0 }).to_vec());
        out.state = Some(state.picks.iter().map(|&p| p as f32).collect());
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> PickState {
        PickState {
            tick: obs.tick,
            picks: obs.state.iter().flatten().map(|&p| p as i32).collect(),
        }
    }

    fn simulate(&self, state: &PickState, actions: &[ActionT], _ticks: u32) -> PickState {
        let mut next = state.clone();
        for action in actions {
            if action.player == next.to_act() && action.discrete >= 0 {
                next.picks.push(action.discrete);
                next.tick += 1;
            }
        }
        next
    }

    fn signals(&self, _state: &PickState, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        out.push(action.discrete as f32);
    }

    fn digest(&self, state: &PickState) -> u64 {
        state.picks.iter().fold(state.tick, |h, &p| {
            h.wrapping_mul(31).wrapping_add(p as u64)
        })
    }

    fn result(&self) -> Option<MatchResultT> {
        None
    }
}

pub struct PickPlaybook;

impl Playbook<Pick> for PickPlaybook {
    fn options(&self, _s: &PickState, _me: PlayerId, _rep: u8, out: &mut Vec<Choice>) {
        out.clear();
        for (i, &ok) in LEGAL.iter().enumerate() {
            if ok == 1 {
                out.push(Choice {
                    id: i as u16,
                    intent: 0,
                    deceptive: false,
                    horizon_ticks: 1,
                    param: [i as f32, 0.0],
                });
            }
        }
    }

    fn act(&self, _s: &PickState, me: PlayerId, c: &Choice, _e: u64, _aim: [f64; 2]) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.discrete = c.param[0] as i32;
        a
    }

    fn reply(&self, _s: &PickState, them: PlayerId) -> ActionT {
        let mut a = ActionT::default();
        a.player = them;
        a.discrete = 0;
        a
    }

    fn evaluate(&self, s: &PickState, me: PlayerId) -> f64 {
        let mine = s.picks.iter().skip(usize::from(me)).step_by(2);
        mine.map(|&p| f64::from(p)).sum()
    }

    fn margin(&self) -> f64 {
        0.0
    }
}
