use feint_ai::Controller;
use feint_protocol::{ActionT, AdapterError, BodyParamsT, GameAdapter, ObservationT, PlayerId};

pub trait TurnGame: GameAdapter + Sized + 'static {
    fn fresh() -> Self;

    fn reward(score: &[i64], me: usize) -> f32;

    fn scripted(name: &str) -> Option<Box<dyn Controller<Self>>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurnError {
    Seat(String),
    BothScripted(usize),
    Seeds {
        expected: usize,
        got: usize,
    },
    Actions {
        expected: usize,
        got: usize,
    },
    Illegal {
        env: usize,
        slot: usize,
        action: i32,
    },
    Stalled {
        env: usize,
        slot: usize,
    },
    Missing {
        env: usize,
        slot: usize,
    },
    Game(AdapterError),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Seat(name) => write!(f, "unknown seat {name:?}"),
            Self::BothScripted(env) => write!(f, "env {env} has no python seat"),
            Self::Seeds { expected, got } => write!(f, "{got} seeds for {expected} envs"),
            Self::Actions { expected, got } => write!(f, "{got} actions for {expected} slots"),
            Self::Illegal { env, slot, action } => {
                write!(f, "action {action} is illegal for env {env} slot {slot}")
            }
            Self::Stalled { env, slot } => {
                write!(f, "scripted seat {slot} of env {env} did not move")
            }
            Self::Missing { env, slot } => {
                write!(f, "env {env} slot {slot} is to act and got no action")
            }
            Self::Game(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TurnError {}

enum Seat<A> {
    Python,
    Scripted(Box<dyn Controller<A>>),
}

impl<A: TurnGame> Seat<A> {
    fn named(name: &str) -> Result<Self, TurnError> {
        if name == "learner" || name.starts_with("frozen:") {
            return Ok(Self::Python);
        }
        A::scripted(name)
            .map(Self::Scripted)
            .ok_or_else(|| TurnError::Seat(name.to_string()))
    }
}

struct Env<A> {
    game: A,
    seats: [Seat<A>; 2],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnOutputs {
    pub obs: Vec<f32>,
    pub legal: Vec<u8>,
    pub ready: Vec<u8>,
    pub reward: Vec<f32>,
    pub terminated: Vec<u8>,
    pub score: Vec<i64>,
}

pub struct TurnBatch<A: TurnGame> {
    envs: Vec<Env<A>>,
    names: Vec<[String; 2]>,
    features: usize,
    actions: usize,
    scales: Vec<f32>,
    seen: ObservationT,
}

const SCRIPTED_MOVES_PER_STEP: usize = 10_000;

impl<A: TurnGame> TurnBatch<A> {
    pub fn new<S: AsRef<str>>(seats: &[[S; 2]]) -> Result<Self, TurnError> {
        let mut envs = Vec::with_capacity(seats.len());
        for (index, [first, second]) in seats.iter().enumerate() {
            let pair = [Seat::named(first.as_ref())?, Seat::named(second.as_ref())?];
            if pair.iter().all(|s| matches!(s, Seat::Scripted(_))) {
                return Err(TurnError::BothScripted(index));
            }
            envs.push(Env {
                game: A::fresh(),
                seats: pair,
            });
        }
        let spec = A::fresh().spec();
        let dims = spec.observation.unwrap_or_default();
        let scales = dims
            .iter()
            .map(|d| {
                let reach = d.min.abs().max(d.max.abs());
                if reach > 0.0 { reach } else { 1.0 }
            })
            .collect();
        Ok(Self {
            names: seats
                .iter()
                .map(|[a, b]| [a.as_ref().to_string(), b.as_ref().to_string()])
                .collect(),
            envs,
            features: dims.len(),
            actions: spec.action_space.map_or(0, |s| s.discrete as usize),
            scales,
            seen: ObservationT::default(),
        })
    }

    #[must_use]
    pub fn envs(&self) -> usize {
        self.envs.len()
    }

    #[must_use]
    pub fn features(&self) -> usize {
        self.features
    }

    #[must_use]
    pub fn actions(&self) -> usize {
        self.actions
    }

    #[must_use]
    pub fn scales(&self) -> Vec<f32> {
        self.scales.clone()
    }

    #[must_use]
    pub fn seat_names(&self) -> &[[String; 2]] {
        &self.names
    }

    fn to_act(game: &A, seen: &mut ObservationT) -> Option<usize> {
        game.observe(game.live(), 0, seen);
        usize::try_from(seen.to_act).ok().filter(|&p| p < 2)
    }

    fn run_scripted(
        env: &mut Env<A>,
        index: usize,
        seen: &mut ObservationT,
    ) -> Result<(), TurnError> {
        let mut events = Vec::new();
        for _ in 0..SCRIPTED_MOVES_PER_STEP {
            if env.game.result().is_some() {
                return Ok(());
            }
            let Some(slot) = Self::to_act(&env.game, seen) else {
                return Ok(());
            };
            let Seat::Scripted(controller) = &mut env.seats[slot] else {
                return Ok(());
            };
            env.game.observe(env.game.live(), slot as PlayerId, seen);
            let action = controller
                .on_tick(&env.game, seen)
                .ok_or(TurnError::Stalled { env: index, slot })?;
            env.game.apply(&action).map_err(TurnError::Game)?;
            events.clear();
            env.game.advance(&mut events);
        }
        Err(TurnError::Stalled {
            env: index,
            slot: 2,
        })
    }

    pub fn reset(&mut self, seeds: &[u64]) -> Result<TurnOutputs, TurnError> {
        if seeds.len() != self.envs.len() {
            return Err(TurnError::Seeds {
                expected: self.envs.len(),
                got: seeds.len(),
            });
        }
        let bodies = [BodyParamsT::default(), BodyParamsT::default()];
        let mut ended = vec![false; self.envs.len()];
        for (index, (env, &seed)) in self.envs.iter_mut().zip(seeds).enumerate() {
            env.game.reset(seed, &bodies).map_err(TurnError::Game)?;
            let spec = env.game.spec();
            for (me, seat) in env.seats.iter_mut().enumerate() {
                if let Seat::Scripted(controller) = seat {
                    controller.new_match(&spec, me as PlayerId, seed);
                }
            }
            Self::run_scripted(env, index, &mut self.seen)?;
            ended[index] = env.game.result().is_some();
        }
        Ok(self.outputs(&ended))
    }

    pub fn step(&mut self, actions: &[i32]) -> Result<TurnOutputs, TurnError> {
        let slots = 2 * self.envs.len();
        if actions.len() != slots {
            return Err(TurnError::Actions {
                expected: slots,
                got: actions.len(),
            });
        }
        let mut moves = Vec::new();
        for (index, env) in self.envs.iter().enumerate() {
            let Some(slot) = Self::to_act(&env.game, &mut self.seen) else {
                continue;
            };
            let action = actions[2 * index + slot];
            if !matches!(env.seats[slot], Seat::Python) {
                continue;
            }
            if action < 0 {
                return Err(TurnError::Missing { env: index, slot });
            }
            env.game
                .observe(env.game.live(), slot as PlayerId, &mut self.seen);
            let legal = self.seen.legal.as_deref().unwrap_or_default();
            if usize::try_from(action).ok().and_then(|a| legal.get(a)) != Some(&1) {
                return Err(TurnError::Illegal {
                    env: index,
                    slot,
                    action,
                });
            }
            moves.push((index, slot, action, self.seen.tick));
        }
        let mut ended = vec![false; self.envs.len()];
        let mut events = Vec::new();
        for (index, slot, discrete, tick) in moves {
            let env = &mut self.envs[index];
            let mut action = ActionT::default();
            action.tick = tick;
            action.player = slot as PlayerId;
            action.discrete = discrete;
            env.game.apply(&action).map_err(TurnError::Game)?;
            events.clear();
            env.game.advance(&mut events);
            Self::run_scripted(env, index, &mut self.seen)?;
            ended[index] = env.game.result().is_some();
        }
        Ok(self.outputs(&ended))
    }

    fn outputs(&mut self, ended: &[bool]) -> TurnOutputs {
        let mut out = TurnOutputs::default();
        let (features, actions) = (self.features, self.actions);
        for (env, &end) in self.envs.iter_mut().zip(ended) {
            let score = env
                .game
                .result()
                .and_then(|r| r.score)
                .unwrap_or_else(|| vec![0, 0]);
            let to_act = Self::to_act(&env.game, &mut self.seen);
            for slot in 0..2 {
                env.game
                    .observe(env.game.live(), slot as PlayerId, &mut self.seen);
                let state = self.seen.state.as_deref().unwrap_or_default();
                out.obs
                    .extend((0..features).map(|i| state.get(i).copied().unwrap_or(0.0)));
                let ready = to_act == Some(slot);
                out.ready.push(u8::from(ready));
                let legal = self.seen.legal.as_deref().unwrap_or_default();
                out.legal.extend((0..actions).map(|i| {
                    if ready {
                        legal.get(i).copied().unwrap_or(0)
                    } else {
                        0
                    }
                }));
                out.reward
                    .push(if end { A::reward(&score, slot) } else { 0.0 });
                out.score.push(score.get(slot).copied().unwrap_or(0));
            }
            out.terminated.push(u8::from(end));
        }
        out
    }
}
