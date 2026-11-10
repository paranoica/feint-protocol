use std::sync::Mutex;

use airhockey::human::{HUMAN_NOISE, Human, Reader};
use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, OBSERVATION_LEN, Params};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, Runner};
use feint_protocol::{ChaCha8Rng, EventKind, GameAdapter, ObservationT, PlayerId};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::pilot::{EXTRA_FEATURES, Pilot, Policy};

pub const FEATURES: usize = OBSERVATION_LEN + EXTRA_FEATURES;

pub const OUTPUTS: usize = 2;

const SEED_FLOOR: u64 = 1 << 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    Learner,
    Level(u8),
    Human,
    Reader { anticipates: bool },
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchError {
    Empty,
    NoLearner(usize),
    BadLevel(u8),
    ZeroThreads,
    BadCap(u64),
    BadRange,
    BadActions { expected: usize, got: usize },
    Game(String),
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "a batch needs at least one env"),
            Self::NoLearner(env) => write!(f, "env {env} has no learner seat"),
            Self::BadLevel(level) => write!(f, "no hand-made level {level}"),
            Self::ZeroThreads => write!(f, "a batch needs at least one thread"),
            Self::BadCap(cap) => write!(f, "cap {cap} is not a positive multiple of 4 ticks"),
            Self::BadRange => write!(f, "reaction and physique ranges must be low <= high"),
            Self::BadActions { expected, got } => {
                write!(f, "{got} action values, expected {expected}")
            }
            Self::Game(why) => write!(f, "the game refused: {why}"),
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

type Learner = Pilot<AirHockey, HockeyPlaybook, Fed>;

enum Player {
    Learner(Box<Learner>),
    Other(Box<dyn Controller<AirHockey> + Send>),
}

impl Player {
    fn controller(&mut self) -> &mut dyn Controller<AirHockey> {
        match self {
            Self::Learner(pilot) => pilot.as_mut(),
            Self::Other(other) => other.as_mut(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Episode {
    pub seed: u64,
    pub reaction_ms: [u32; 2],
    pub physique: [f64; 2],
}

struct Env {
    seats: [Seat; 2],
    game: AirHockey,
    players: [Player; 2],
    runner: Runner,
    rng: ChaCha8Rng,
    episode: Episode,
    obs: [[f32; FEATURES]; 2],
    final_obs: [[f32; FEATURES]; 2],
    ready: [bool; 2],
    reward: [f32; 2],
    terminated: bool,
    truncated: bool,
    score: [i64; 2],
    error: Option<String>,
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

fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn other(seat: Seat, params: &Params) -> Result<Box<dyn Controller<AirHockey> + Send>, String> {
    match seat {
        Seat::Learner => Err("a learner is not an opponent".into()),
        Seat::Level(level) => {
            let d = Difficulty::preset(level).ok_or_else(|| format!("level {level}"))?;
            let spec = AirHockey::new(params.clone(), level).spec();
            let bot = Bot::new(d, HockeyPlaybook::new(params.clone()), &spec)
                .map_err(|e| format!("{e:?}"))?;
            Ok(Box::new(bot))
        }
        Seat::Human => Ok(Box::new(Human::new(params.clone(), HUMAN_NOISE))),
        Seat::Reader { anticipates } => Ok(Box::new(Reader::new(params.clone(), anticipates))),
    }
}

struct Ranges {
    reaction_ms: [u32; 2],
    physique: [f64; 2],
    cap: u64,
}

struct Dealt {
    players: [Player; 2],
    episode: Episode,
    header: RunHeader,
}

fn deal(
    rng: &mut ChaCha8Rng,
    seats: [Seat; 2],
    ranges: &Ranges,
    params: &Params,
    game: &AirHockey,
) -> Result<Dealt, String> {
    let seed = SEED_FLOOR | rng.next_u64();
    let mut episode = Episode {
        seed,
        reaction_ms: [0; 2],
        physique: [1.0; 2],
    };
    let spec = game.spec();
    let mut bodies = Vec::with_capacity(2);
    let mut players = Vec::with_capacity(2);
    for (index, seat) in seats.into_iter().enumerate() {
        let (reaction, strength) = match seat {
            Seat::Learner => {
                let [low, high] = ranges.reaction_ms;
                let span = f64::from(high - low) + 1.0;
                let reaction = (low + (unit(rng) * span) as u32).min(high);
                let [weak, strong] = ranges.physique;
                (reaction, weak + unit(rng) * (strong - weak))
            }
            Seat::Level(level) => Difficulty::preset(level)
                .map(|d| (d.reaction_ms, d.physique))
                .ok_or_else(|| format!("level {level}"))?,
            Seat::Human | Seat::Reader { .. } => (0, 1.0),
        };
        episode.reaction_ms[index] = reaction;
        episode.physique[index] = strength;
        bodies.push(body(game, strength));
        players.push(match seat {
            Seat::Learner => Pilot::new(
                HockeyPlaybook::new(params.clone()),
                Fed::default(),
                &spec,
                reaction,
                strength,
            )
            .map(|pilot| Player::Learner(Box::new(pilot)))
            .map_err(|e| format!("{e:?}"))?,
            seat => Player::Other(other(seat, params)?),
        });
    }
    let [low, high] = <[Player; 2]>::try_from(players).map_err(|_| String::from("two players"))?;
    Ok(Dealt {
        players: [low, high],
        episode,
        header: RunHeader {
            game_id: "airhockey".into(),
            game_version: GAME_VERSION,
            seed,
            bodies,
            levels: [0, 0],
        },
    })
}

fn begin(game: &mut AirHockey, dealt: Dealt) -> Result<(Runner, [Player; 2], Episode), String> {
    let Dealt {
        mut players,
        episode,
        header,
    } = dealt;
    let [a, b] = &mut players;
    let runner =
        Runner::start(game, [a.controller(), b.controller()], header).map_err(|e| e.to_string())?;
    Ok((runner, players, episode))
}

impl Env {
    fn new(
        seats: [Seat; 2],
        seed: u64,
        index: usize,
        ranges: &Ranges,
        params: &Params,
    ) -> Result<Self, String> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(index as u64);
        let mut game = AirHockey::new(params.clone(), 0);
        let dealt = deal(&mut rng, seats, ranges, params, &game)?;
        let (runner, players, episode) = begin(&mut game, dealt)?;
        Ok(Self {
            seats,
            game,
            players,
            runner,
            rng,
            episode,
            obs: [[0.0; FEATURES]; 2],
            final_obs: [[0.0; FEATURES]; 2],
            ready: [false; 2],
            reward: [0.0; 2],
            terminated: false,
            truncated: false,
            score: [0; 2],
            error: None,
        })
    }

    fn start(&mut self, ranges: &Ranges, params: &Params) {
        let started = deal(&mut self.rng, self.seats, ranges, params, &self.game)
            .and_then(|dealt| begin(&mut self.game, dealt));
        match started {
            Ok((runner, players, episode)) => {
                self.runner = runner;
                self.players = players;
                self.episode = episode;
            }
            Err(why) => self.error = Some(why),
        }
    }

    fn sense(&mut self) -> bool {
        let mut any = false;
        let mut seen = ObservationT::default();
        self.ready = [false; 2];
        self.obs = [[0.0; FEATURES]; 2];
        for index in 0..2 {
            if let Player::Learner(pilot) = &mut self.players[index] {
                self.game
                    .observe(self.game.live(), index as PlayerId, &mut seen);
                if let Some(features) = pilot.sense(&self.game, &seen) {
                    self.obs[index].copy_from_slice(features);
                    self.ready[index] = true;
                    any = true;
                }
            }
        }
        any
    }

    fn tick(&mut self) {
        let [a, b] = &mut self.players;
        self.runner
            .step(&mut self.game, [a.controller(), b.controller()]);
        self.runner.forget_actions();
        for event in self.runner.events() {
            if event.kind == EventKind::Score {
                let scorer = usize::from(event.player == 1);
                self.reward[scorer] += 1.0;
                self.reward[1 - scorer] -= 1.0;
            }
        }
    }

    fn end(&mut self, ranges: &Ranges, params: &Params) {
        let mut seen = ObservationT::default();
        self.game.observe(self.game.live(), 0, &mut seen);
        let score = seen.score.unwrap_or_default();
        self.score = [
            score.first().copied().unwrap_or(0),
            score.get(1).copied().unwrap_or(0),
        ];
        self.start(ranges, params);
    }

    fn advance(&mut self, ranges: &Ranges, params: &Params) {
        while self.error.is_none() {
            if self.game.result().is_some() {
                self.terminated = true;
                self.end(ranges, params);
                continue;
            }
            let any = self.sense();
            if self.runner.ticks() >= ranges.cap {
                self.truncated = true;
                self.final_obs = self.obs;
                self.end(ranges, params);
                continue;
            }
            if any {
                return;
            }
            self.tick();
        }
    }

    fn play(&mut self, actions: &[f32]) {
        for index in 0..2 {
            if let (Player::Learner(pilot), true) = (&mut self.players[index], self.ready[index]) {
                let at = index * OUTPUTS;
                pilot
                    .policy_mut()
                    .0
                    .copy_from_slice(&actions[at..at + OUTPUTS]);
            }
        }
        self.tick();
    }

    fn clear_step(&mut self) {
        self.reward = [0.0; 2];
        self.terminated = false;
        self.truncated = false;
        self.final_obs = [[0.0; FEATURES]; 2];
        self.score = [0; 2];
    }
}

fn body(game: &AirHockey, physique: f64) -> feint_protocol::BodyParamsT {
    let mut d = Difficulty::preset(5).unwrap_or_else(|| unreachable!("level 5 exists"));
    d.physique = physique;
    d.body(&game.spec())
}

impl HockeyBatch {
    pub fn new(spec: BatchSpec) -> Result<Self, BatchError> {
        if spec.seats.is_empty() {
            return Err(BatchError::Empty);
        }
        if spec.threads == 0 {
            return Err(BatchError::ZeroThreads);
        }
        if spec.cap_ticks == 0 || !spec.cap_ticks.is_multiple_of(4) {
            return Err(BatchError::BadCap(spec.cap_ticks));
        }
        let [low, high] = spec.physique;
        if spec.reaction_ms[0] > spec.reaction_ms[1] || !(0.0 < low && low <= high && high <= 1.0) {
            return Err(BatchError::BadRange);
        }
        let ranges = Ranges {
            reaction_ms: spec.reaction_ms,
            physique: spec.physique,
            cap: spec.cap_ticks,
        };
        let mut envs = Vec::with_capacity(spec.seats.len());
        for (index, seats) in spec.seats.iter().enumerate() {
            if !seats.contains(&Seat::Learner) {
                return Err(BatchError::NoLearner(index));
            }
            for seat in seats {
                if let Seat::Level(level) = *seat
                    && Difficulty::preset(level).is_none()
                {
                    return Err(BatchError::BadLevel(level));
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
