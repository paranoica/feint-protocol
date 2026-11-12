use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{BotError, Controller, Record, RecordError, Runner, decode_record, encode_record};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::batch::{FEATURES, OUTPUTS};
use crate::{Pilot, Policy};

pub const MAX_TICKS: u64 = 240 * 60 * 30;
const HUMAN: PlayerId = 0;
const PHYSIQUE: f64 = 1.0;
const OWN_TARGET: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum ImitationError {
    Decode(RecordError),
    Game,
    Ticks,
    Bodies,
    NoHuman,
    Reaction(BotError),
    Mismatch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Imitation {
    pub features: Vec<f32>,
    pub targets: Vec<f32>,
    pub decisions: usize,
    pub ticks: u64,
}

#[derive(Default)]
struct Teacher {
    label: [f32; OUTPUTS],
    features: Vec<f32>,
    targets: Vec<f32>,
}

impl Policy for Teacher {
    fn act(&mut self, features: &[f32], out: &mut [f32]) {
        self.features.extend_from_slice(features);
        self.targets.extend_from_slice(&self.label);
        out.copy_from_slice(&self.label);
    }
}

struct Replay {
    actions: Vec<ActionT>,
    next: usize,
}

impl Replay {
    fn at(&mut self, tick: u64) -> Option<ActionT> {
        let action = self.actions.get(self.next).filter(|a| a.tick == tick)?;
        self.next += 1;
        Some(action.clone())
    }
}

impl Controller<AirHockey> for Replay {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {
        self.next = 0;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.at(obs.tick)
    }
}

struct Shadow {
    human: Replay,
    pilot: Pilot<AirHockey, HockeyPlaybook, Teacher>,
    mid: [f32; OUTPUTS],
    half: [f32; OUTPUTS],
    stride: u64,
}

impl Shadow {
    fn label(&self, obs: &ObservationT) -> [f32; OUTPUTS] {
        let end = obs.tick + self.stride - 1;
        let held = self.human.actions.partition_point(|a| a.tick <= end);
        let target = match held.checked_sub(1) {
            Some(k) => self.human.actions[k].values.as_deref().unwrap_or_default(),
            None => obs
                .state
                .as_deref()
                .and_then(|s| s.get(OWN_TARGET..OWN_TARGET + OUTPUTS))
                .unwrap_or_default(),
        };
        let mut label = [0.0; OUTPUTS];
        for (k, (out, &value)) in label.iter_mut().zip(target).enumerate() {
            *out = ((value - self.mid[k]) / self.half[k]).clamp(-1.0, 1.0);
        }
        label
    }
}

impl Controller<AirHockey> for Shadow {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.human.next = 0;
        self.pilot.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.pilot.policy_mut().label = self.label(obs);
        let _ = self.pilot.on_tick(game, obs);
        self.human.at(obs.tick)
    }
}

fn human_body(spec: &GameSpecT) -> BodyParamsT {
    let mut body = BodyParamsT::default();
    body.values = Some(spec.body_params.iter().flatten().map(|d| d.max).collect());
    body
}

fn checked(record: &Record, spec: &GameSpecT) -> Result<(), ImitationError> {
    if spec.game_id.as_deref() != Some(record.header.game_id.as_str())
        || record.header.game_version != GAME_VERSION
    {
        return Err(ImitationError::Game);
    }
    if record.ticks == 0 || record.ticks > MAX_TICKS {
        return Err(ImitationError::Ticks);
    }
    if record.header.bodies.first() != Some(&human_body(spec)) {
        return Err(ImitationError::Bodies);
    }
    if !record.actions.iter().any(|a| a.player == HUMAN) {
        return Err(ImitationError::NoHuman);
    }
    Ok(())
}

fn seat(record: &Record, player: PlayerId) -> Replay {
    Replay {
        actions: record
            .actions
            .iter()
            .filter(|a| a.player == player)
            .cloned()
            .collect(),
        next: 0,
    }
}

pub fn imitation(bytes: &[u8], reaction_ms: u32) -> Result<Imitation, ImitationError> {
    let record = decode_record(bytes).map_err(ImitationError::Decode)?;
    let mut game = AirHockey::new(Params::default(), record.header.levels[1]);
    let spec = game.spec();
    checked(&record, &spec)?;
    let pilot = Pilot::new(
        HockeyPlaybook::new(Params::default()),
        Teacher::default(),
        &spec,
        reaction_ms,
        PHYSIQUE,
    )
    .map_err(ImitationError::Reaction)?;
    let dims = spec
        .action_space
        .as_deref()
        .and_then(|s| s.continuous.as_deref())
        .ok_or(ImitationError::Game)?;
    let [x, y] = dims else {
        return Err(ImitationError::Game);
    };
    let mut shadow = Shadow {
        human: seat(&record, HUMAN),
        pilot,
        mid: [(x.max + x.min) / 2.0, (y.max + y.min) / 2.0],
        half: [(x.max - x.min) / 2.0, (y.max - y.min) / 2.0],
        stride: u64::from(spec.tick_hz / spec.decision_hz),
    };
    let mut other = seat(&record, 1 - HUMAN);
    let mut runner = Runner::start(&mut game, [&mut shadow, &mut other], record.header.clone())
        .map_err(|_| ImitationError::Bodies)?;
    while runner.ticks() < record.ticks && runner.step(&mut game, [&mut shadow, &mut other]) {}
    let report = runner.finish(&mut game, [&mut shadow, &mut other]);
    if encode_record(&report.record()) != bytes {
        return Err(ImitationError::Mismatch);
    }
    let teacher = std::mem::take(shadow.pilot.policy_mut());
    let decisions = teacher.targets.len() / OUTPUTS;
    debug_assert_eq!(teacher.features.len(), decisions * FEATURES);
    Ok(Imitation {
        features: teacher.features,
        targets: teacher.targets,
        decisions,
        ticks: report.ticks,
    })
}
