use feint_protocol::{ActionT, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub id: u16,
    pub intent: u8,
    pub deceptive: bool,
    pub horizon_ticks: u32,
    pub param: [f32; 2],
    pub shown: Option<u8>,
    pub real: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub pace: f32,
    pub twitch: Option<u8>,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            pace: 1.0,
            twitch: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Still,
    Naive,
    Cover(u8),
    Shoot(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub covered: Option<u8>,
    pub shot: Option<u8>,
}

pub trait Playbook<A: GameAdapter> {
    fn options(
        &self,
        state: &A::State,
        me: PlayerId,
        repertoire: u8,
        cue: &Cue,
        out: &mut Vec<Choice>,
    );

    fn act(
        &self,
        state: &A::State,
        me: PlayerId,
        choice: &Choice,
        elapsed: u64,
        aim: [f64; 2],
    ) -> ActionT;

    fn opponent_move(&self, state: &A::State, them: PlayerId, stance: Stance) -> ActionT;

    fn evaluate(&self, state: &A::State, me: PlayerId) -> f64;

    fn margin(&self) -> f64;

    fn read(&self, before: &A::State, after: &A::State, them: PlayerId) -> Read {
        let _ = (before, after, them);
        Read::default()
    }

    fn importance(&self, state: &A::State, me: PlayerId) -> f64 {
        let _ = (state, me);
        0.0
    }

    fn possession(&self, state: &A::State, me: PlayerId) -> Option<u64> {
        let _ = (state, me);
        None
    }

    fn signal_classes(&self) -> u8 {
        3
    }
}

pub trait Controller<A: GameAdapter> {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64);

    fn on_tick(&mut self, adapter: &A, obs: &ObservationT) -> Option<ActionT>;

    fn on_event(&mut self, event: &EventT) {
        let _ = event;
    }

    fn end_match(&mut self, adapter: &A, obs: &ObservationT) {
        let _ = (adapter, obs);
    }
}
