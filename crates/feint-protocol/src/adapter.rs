use rand_chacha::ChaCha8Rng;

use crate::{ActionT, BodyParamsT, EventT, GameSpecT, MatchResultT, ObservationT};

/// A player's index, `0..num_players`.
pub type PlayerId = u8;

/// Why an adapter refused a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    /// An action stamped for a tick other than the one about to be stepped.
    WrongTick { expected: u64, got: u64 },
    /// No such player in this game.
    BadPlayer(PlayerId),
    /// The action does not fit the game's action space.
    BadAction(&'static str),
    /// Body parameters do not fit the game's `body_params`.
    BadBody(&'static str),
    /// The match is over.
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

/// Everything game-specific, behind one interface, so the core never names a game (FNT-D3).
///
/// The bot side of the core only ever receives observations and plans from
/// [`GameAdapter::state_from_observation`]; [`GameAdapter::live`] is for the match runner,
/// replays and digests. Observations, actions and signals are in the acting player's own
/// frame.
pub trait GameAdapter: Send {
    /// A full world state that `simulate` can run from.
    type State: Clone + Send;

    /// Static description of the game.
    fn spec(&self) -> GameSpecT;

    /// Start a new match.
    ///
    /// # Errors
    /// [`AdapterError::BadBody`] when `bodies` does not hold one valid entry per player.
    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError>;

    /// The true current state. Never hand it to a bot.
    fn live(&self) -> &Self::State;

    /// Feed one player's action for the tick about to be stepped. A realtime action is held
    /// until replaced; a turn-based game advances on it.
    ///
    /// # Errors
    /// Wrong tick, unknown player, an action outside the action space, or a finished match.
    /// A refused action leaves the game unchanged.
    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError>;

    /// Step one tick, appending what happened. A no-op for turn-based games.
    fn advance(&mut self, events: &mut Vec<EventT>);

    /// What `player` may see of `state`, written into `out` (its vectors are reused).
    fn observe(&self, state: &Self::State, player: PlayerId, out: &mut ObservationT);

    /// A full state consistent with an observation; hidden information is sampled.
    fn state_from_observation(&self, obs: &ObservationT, rng: &mut ChaCha8Rng) -> Self::State;

    /// Run `ticks` steps from `state` with each listed action held throughout.
    fn simulate(&self, state: &Self::State, actions: &[ActionT], ticks: u32) -> Self::State;

    /// What the opponent will see of `action` taken in `state`, written into `out`.
    fn signals(&self, state: &Self::State, action: &ActionT, out: &mut Vec<f32>);

    /// A hash of the state, equal for bit-identical states.
    fn digest(&self, state: &Self::State) -> u64;

    /// The result once the match is over.
    fn result(&self) -> Option<MatchResultT>;
}
