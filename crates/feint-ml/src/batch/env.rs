use airhockey::human::{HUMAN_NOISE, Human, Reader};
use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, Runner};
use feint_protocol::{ChaCha8Rng, EventKind, GameAdapter, ObservationT, PlayerId};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use super::{Episode, FEATURES, Fed, OUTPUTS, SEED_FLOOR, Seat};
use crate::pilot::Pilot;

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

pub(super) struct Env {
    pub(super) seats: [Seat; 2],
    pub(super) game: AirHockey,
    players: [Player; 2],
    pub(super) runner: Runner,
    rng: ChaCha8Rng,
    pub(super) episode: Episode,
    pub(super) obs: [[f32; FEATURES]; 2],
    pub(super) final_obs: [[f32; FEATURES]; 2],
    pub(super) ready: [bool; 2],
    pub(super) reward: [f32; 2],
    pub(super) terminated: bool,
    pub(super) truncated: bool,
    pub(super) score: [i64; 2],
    pub(super) error: Option<String>,
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

pub(super) struct Ranges {
    pub(super) reaction_ms: [u32; 2],
    pub(super) physique: [f64; 2],
    pub(super) cap: u64,
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
    pub(super) fn new(
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

    pub(super) fn start(&mut self, ranges: &Ranges, params: &Params) {
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

    pub(super) fn advance(&mut self, ranges: &Ranges, params: &Params) {
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

    pub(super) fn play(&mut self, actions: &[f32]) {
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

    pub(super) fn clear_step(&mut self) {
        self.reward = [0.0; 2];
        self.terminated = false;
        self.truncated = false;
        self.final_obs = [[0.0; FEATURES]; 2];
        self.score = [0; 2];
    }
}

pub(super) fn body(game: &AirHockey, physique: f64) -> feint_protocol::BodyParamsT {
    let mut d = Difficulty::preset(5).unwrap_or_else(|| unreachable!("level 5 exists"));
    d.physique = physique;
    d.body(&game.spec())
}
