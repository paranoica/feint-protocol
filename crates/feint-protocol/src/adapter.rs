use rand_chacha::ChaCha8Rng;

use crate::{ActionT, BodyParamsT, EventT, GameSpecT, MatchResultT, ObservationT};

pub type PlayerId = u8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    WrongTick { expected: u64, got: u64 },
    BadPlayer(PlayerId),
    BadAction(&'static str),
    BadBody(&'static str),
    MatchOver,
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongTick { expected, got } => {
                write!(f, "action for tick {got}, the game is at tick {expected}")
            }
            Self::BadPlayer(p) => write!(f, "no player {p}"),
            Self::BadAction(why) => write!(f, "bad action: {why}"),
            Self::BadBody(why) => write!(f, "bad body parameters: {why}"),
            Self::MatchOver => write!(f, "the match is over"),
        }
    }
}

impl std::error::Error for AdapterError {}

pub trait GameAdapter: Send {
    type State: Clone + Send;

    fn spec(&self) -> GameSpecT;

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError>;

    fn live(&self) -> &Self::State;

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError>;

    fn advance(&mut self, events: &mut Vec<EventT>);

    fn observe(&self, state: &Self::State, player: PlayerId, out: &mut ObservationT);

    fn state_from_observation(&self, obs: &ObservationT, rng: &mut ChaCha8Rng) -> Self::State;

    fn simulate(&self, state: &Self::State, actions: &[ActionT], ticks: u32) -> Self::State;

    fn signals(&self, state: &Self::State, action: &ActionT, out: &mut Vec<f32>);

    fn digest(&self, state: &Self::State) -> u64;

    fn result(&self) -> Option<MatchResultT>;
}
