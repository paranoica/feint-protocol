mod env;

use std::sync::Mutex;

use airhockey::{AirHockey, OBSERVATION_LEN, Params};
use feint_ai::{Difficulty, MAX_DELAY_TICKS, delay_ticks};
use feint_protocol::GameAdapter;

use crate::mlp::Mlp;
use crate::pilot::{EXTRA_FEATURES, Policy};
use env::{Env, Ranges, body};

pub const FEATURES: usize = OBSERVATION_LEN + EXTRA_FEATURES;

pub const OUTPUTS: usize = 2;

const SEED_FLOOR: u64 = 1 << 63;

#[must_use]
pub fn feature_scales(params: &Params) -> [f32; FEATURES] {
    let spec = AirHockey::new(params.clone(), 0).spec();
    let mut scales = [1.0; FEATURES];
    for (scale, dim) in scales.iter_mut().zip(spec.observation.iter().flatten()) {
        *scale = dim.max.max(-dim.min).max(f32::MIN_POSITIVE);
    }
    scales
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    Learner,
    Level(u8),
    Human,
    Reader { anticipates: bool },
    Frozen(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchSpec {
    pub seats: Vec<[Seat; 2]>,
    pub seed: u64,
    pub cap_ticks: u64,
    pub threads: usize,
    pub reaction_ms: [u32; 2],
    pub physique: [f64; 2],
    pub params: Params,
    pub frozen: Vec<Vec<u8>>,
}

impl BatchSpec {
    #[must_use]
    pub fn new(seats: Vec<[Seat; 2]>, seed: u64) -> Self {
        Self {
            seats,
            seed,
            cap_ticks: 240 * 600,
            threads: 1,
            reaction_ms: [150, 400],
            physique: [0.4, 1.0],
            params: Params::default(),
            frozen: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchError {
    Empty,
    NoLearner(usize),
    BadLevel(u8),
    ZeroThreads,
    BadParams(&'static str),
    BadCap { cap: u64, least: u64 },
    BadRange,
    BadActions { expected: usize, got: usize },
    Game(String),
    Frozen(u16),
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "a batch needs at least one env"),
            Self::NoLearner(env) => write!(f, "env {env} has no learner seat"),
            Self::BadLevel(level) => write!(f, "no hand-made level {level}"),
            Self::ZeroThreads => write!(f, "a batch needs at least one thread"),
            Self::BadParams(why) => write!(f, "bad game parameters: {why}"),
            Self::BadCap { cap, least } => write!(
                f,
                "cap {cap} must be a multiple of the decision stride and at least {least} ticks, past the slowest warm-up"
            ),
            Self::BadRange => write!(
                f,
                "reaction and physique ranges must be low <= high, physique in (0, 1], reaction within the delay limit"
            ),
            Self::BadActions { expected, got } => {
                write!(f, "{got} action values, expected {expected}")
            }
            Self::Game(why) => write!(f, "the game refused: {why}"),
            Self::Frozen(index) => write!(
                f,
                "frozen checkpoint {index} is missing or not a policy file"
            ),
        }
    }
}

impl std::error::Error for BatchError {}

#[derive(Clone, Debug, Default)]
pub struct Fed(pub [f32; OUTPUTS]);

impl Policy for Fed {
    fn act(&mut self, _features: &[f32], out: &mut [f32]) {
        out.copy_from_slice(&self.0);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Episode {
    pub seed: u64,
    pub reaction_ms: [u32; 2],
    pub physique: [f64; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outputs {
    pub obs: Vec<f32>,
    pub final_obs: Vec<f32>,
    pub ready: Vec<bool>,
    pub reward: Vec<f32>,
    pub terminated: Vec<bool>,
    pub truncated: Vec<bool>,
    pub score: Vec<i64>,
}

pub struct HockeyBatch {
    envs: Vec<Env>,
    ranges: Ranges,
    threads: usize,
    params: Params,
    out: Outputs,
}

impl HockeyBatch {
    pub fn new(spec: BatchSpec) -> Result<Self, BatchError> {
        if spec.seats.is_empty() {
            return Err(BatchError::Empty);
        }
        if spec.threads == 0 {
            return Err(BatchError::ZeroThreads);
        }
        let params = &spec.params;
        params.validate().map_err(BatchError::BadParams)?;
        if !params.tick_hz.is_multiple_of(params.decision_hz) {
            return Err(BatchError::BadParams(
                "the batch needs decisions on a whole number of ticks",
            ));
        }
        let [low, high] = spec.physique;
        let delay = delay_ticks(spec.reaction_ms[1], params.tick_hz);
        if spec.reaction_ms[0] > spec.reaction_ms[1]
            || delay > MAX_DELAY_TICKS
            || !(0.0 < low && low <= high && high <= 1.0)
        {
            return Err(BatchError::BadRange);
        }
        let mut probe = AirHockey::new(params.clone(), 0);
        let weakest = body(&probe, low);
        if probe.reset(0, &[weakest.clone(), weakest]).is_err() {
            return Err(BatchError::BadRange);
        }
        let stride = u64::from(params.tick_hz / params.decision_hz);
        let least = (delay as u64).div_ceil(stride) * stride + stride;
        if spec.cap_ticks < least || !spec.cap_ticks.is_multiple_of(stride) {
            return Err(BatchError::BadCap {
                cap: spec.cap_ticks,
                least,
            });
        }
        let mut frozen = Vec::with_capacity(spec.frozen.len());
        for (index, bytes) in spec.frozen.iter().enumerate() {
            let index = u16::try_from(index).map_err(|_| BatchError::Frozen(u16::MAX))?;
            frozen.push(Mlp::from_safetensors(bytes).map_err(|_| BatchError::Frozen(index))?);
        }
        let ranges = Ranges {
            reaction_ms: spec.reaction_ms,
            physique: spec.physique,
            cap: spec.cap_ticks,
            frozen,
        };
        let mut envs = Vec::with_capacity(spec.seats.len());
        for (index, seats) in spec.seats.iter().enumerate() {
            if !seats.contains(&Seat::Learner) {
                return Err(BatchError::NoLearner(index));
            }
            for seat in seats {
                match *seat {
                    Seat::Level(level) if Difficulty::preset(level).is_none() => {
                        return Err(BatchError::BadLevel(level));
                    }
                    Seat::Frozen(index) if usize::from(index) >= ranges.frozen.len() => {
                        return Err(BatchError::Frozen(index));
                    }
                    _ => {}
                }
            }
            envs.push(
                Env::new(*seats, spec.seed, index, &ranges, &spec.params)
                    .map_err(BatchError::Game)?,
            );
        }
        let n = envs.len();
        Ok(Self {
            envs,
            ranges,
            threads: spec.threads,
            params: spec.params,
            out: Outputs {
                obs: vec![0.0; n * 2 * FEATURES],
                final_obs: vec![0.0; n * 2 * FEATURES],
                ready: vec![false; n * 2],
                reward: vec![0.0; n * 2],
                terminated: vec![false; n],
                truncated: vec![false; n],
                score: vec![0; n * 2],
            },
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.envs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.envs.is_empty()
    }

    #[must_use]
    pub fn episode(&self, env: usize) -> Option<Episode> {
        self.envs.get(env).map(|e| e.episode)
    }

    #[must_use]
    pub fn seats(&self, env: usize) -> Option<[Seat; 2]> {
        self.envs.get(env).map(|e| e.seats)
    }

    #[must_use]
    pub fn ticks(&self, env: usize) -> Option<u64> {
        self.envs.get(env).map(|e| e.runner.ticks())
    }

    #[must_use]
    pub fn digest(&self, env: usize) -> Option<u64> {
        self.envs.get(env).map(|e| e.game.digest(e.game.live()))
    }

    pub fn reset(&mut self) -> Result<&Outputs, BatchError> {
        self.run(&[], 0, |env, _, ranges, params| {
            env.clear_step();
            env.start(ranges, params);
            env.advance(ranges, params);
        })
    }

    pub fn step(&mut self, actions: &[f32]) -> Result<&Outputs, BatchError> {
        let expected = self.envs.len() * 2 * OUTPUTS;
        if actions.len() != expected {
            return Err(BatchError::BadActions {
                expected,
                got: actions.len(),
            });
        }
        self.run(actions, 2 * OUTPUTS, |env, actions, ranges, params| {
            env.clear_step();
            env.play(actions);
            env.advance(ranges, params);
        })
    }

    fn run(
        &mut self,
        input: &[f32],
        per_env: usize,
        work: impl Fn(&mut Env, &[f32], &Ranges, &Params) + Sync,
    ) -> Result<&Outputs, BatchError> {
        let (ranges, params) = (&self.ranges, &self.params);
        let threads = self.threads.min(self.envs.len());
        let jobs = Mutex::new(self.envs.iter_mut().enumerate());
        let run = || {
            loop {
                let next = jobs.lock().map(|mut it| it.next());
                let Ok(Some((index, env))) = next else {
                    return;
                };
                let input = &input[index * per_env..(index + 1) * per_env];
                work(env, input, ranges, params);
            }
        };
        if threads <= 1 {
            run();
        } else {
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(run);
                }
            });
        }
        self.gather()
    }

    fn gather(&mut self) -> Result<&Outputs, BatchError> {
        for (index, env) in self.envs.iter().enumerate() {
            if let Some(why) = &env.error {
                return Err(BatchError::Game(format!("env {index}: {why}")));
            }
            let span = 2 * FEATURES;
            for seat in 0..2 {
                let at = index * span + seat * FEATURES;
                self.out.obs[at..at + FEATURES].copy_from_slice(&env.obs[seat]);
                self.out.final_obs[at..at + FEATURES].copy_from_slice(&env.final_obs[seat]);
                self.out.ready[index * 2 + seat] = env.ready[seat];
                self.out.reward[index * 2 + seat] = env.reward[seat];
                self.out.score[index * 2 + seat] = env.score[seat];
            }
            self.out.terminated[index] = env.terminated;
            self.out.truncated[index] = env.truncated;
        }
        Ok(&self.out)
    }
}
