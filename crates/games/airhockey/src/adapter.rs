use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventKind, EventT,
    GameAdapter, GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::game::{Input, InputError, Match, MatchEvent};
use crate::mallet::{Mallet, Side};
use crate::params::{Body, Params};
use crate::puck::Puck;
use crate::vec2::Vec2;
use crate::world::{World, regions};

pub const GAME_VERSION: u32 = 1;

pub const OBSERVATION_LEN: usize = 15;

pub struct AirHockey {
    game: Match,
    level: u8,
    scratch: Vec<MatchEvent>,
}

impl AirHockey {
    #[must_use]
    pub fn new(params: Params, level: u8) -> Self {
        let body = Body::new(params.max_body_speed, params.max_body_accel).unwrap_or_default();
        Self {
            game: Match::new(params, [body; 2], 0),
            level,
            scratch: Vec::with_capacity(4),
        }
    }

    pub fn game_mut(&mut self) -> &mut Match {
        &mut self.game
    }
}

impl GameAdapter for AirHockey {
    type State = Match;

    fn spec(&self) -> GameSpecT {
        let p = &self.game.params;
        let (l, w) = (p.half_length as f32, p.half_width as f32);
        let dims = |list: &[(&str, f32, f32, &str)]| {
            list.iter()
                .map(|&(name, min, max, unit)| dim(name, min, max, unit))
                .collect::<Vec<_>>()
        };
        let cap = p.puck_speed_cap as f32;
        let mallet = p.max_body_speed as f32;
        let mut spec = GameSpecT::default();
        spec.game_id = Some("airhockey".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::Realtime;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.decision_hz;
        let mut space = ActionSpaceT::default();
        space.continuous = Some(dims(&[("target_x", -l, l, "m"), ("target_y", -w, w, "m")]));
        spec.action_space = Some(Box::new(space));
        spec.observation = Some(dims(&[
            ("puck_x", -l, l, "m"),
            ("puck_y", -w, w, "m"),
            ("puck_vx", -cap, cap, "m/s"),
            ("puck_vy", -cap, cap, "m/s"),
            ("my_x", -l, l, "m"),
            ("my_y", -w, w, "m"),
            ("my_vx", -mallet, mallet, "m/s"),
            ("my_vy", -mallet, mallet, "m/s"),
            ("my_target_x", -l, l, "m"),
            ("my_target_y", -w, w, "m"),
            ("opponent_x", -l, l, "m"),
            ("opponent_y", -w, w, "m"),
            ("opponent_vx", -mallet, mallet, "m/s"),
            ("opponent_vy", -mallet, mallet, "m/s"),
            ("stall", 0.0, p.stall_seconds as f32, "s"),
        ]));
        spec.body_params = Some(dims(&[
            ("max_speed", 0.0, mallet, "m/s"),
            ("max_accel", 0.0, p.max_body_accel as f32, "m/s^2"),
        ]));
        spec.signals = Some(dims(&[
            ("opponent_x", -l, l, "m"),
            ("opponent_y", -w, w, "m"),
            ("opponent_vx", -mallet, mallet, "m/s"),
            ("opponent_vy", -mallet, mallet, "m/s"),
        ]));
        spec.intent_labels = Some(
            ["none", "strike", "defend", "fake"]
                .map(String::from)
                .to_vec(),
        );
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        const WHY: &str = "two positive finite values per player";
        let params = &self.game.params;
        let within = |value: f32, max: f64| {
            if value.is_finite() && value <= max as f32 {
                f64::from(value).min(max)
            } else {
                f64::INFINITY
            }
        };
        let parse = |b: &BodyParamsT| match b.values.as_deref() {
            Some(&[speed, accel]) => params
                .body(
                    within(speed, params.max_body_speed),
                    within(accel, params.max_body_accel),
                )
                .map_err(|_| AdapterError::BadBody(WHY)),
            _ => Err(AdapterError::BadBody(WHY)),
        };
        let [low, high] = bodies else {
            return Err(AdapterError::BadBody(WHY));
        };
        let bodies = [parse(low)?, parse(high)?];
        self.game = Match::new(self.game.params.clone(), bodies, seed);
        Ok(())
    }

    fn live(&self) -> &Match {
        &self.game
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        let input = to_input(action, &self.game.params)?;
        self.game.apply(input).map_err(|error| match error {
            InputError::WrongTick { expected, got } => AdapterError::WrongTick { expected, got },
            InputError::BadPlayer(p) => AdapterError::BadPlayer(p),
            InputError::NotFinite => AdapterError::BadAction("not finite"),
            InputError::MatchOver => AdapterError::MatchOver,
        })
    }

    fn advance(&mut self, events: &mut Vec<EventT>) {
        self.scratch.clear();
        self.game.step(&mut self.scratch);
        for event in &self.scratch {
            let (tick, kind, side) = match *event {
                MatchEvent::Goal { tick, scorer } => (tick, EventKind::Score, scorer),
                MatchEvent::RoundEnd { tick, to } => (tick, EventKind::RoundEnd, to),
                MatchEvent::Over { .. } => continue,
            };
            let mut out = EventT::default();
            out.tick = tick;
            out.kind = kind;
            out.player = side.index() as i8;
            events.push(out);
        }
    }

    fn observe(&self, state: &Match, player: PlayerId, out: &mut ObservationT) {
        let frame = Frame::of(player);
        let me = usize::from(player.min(1));
        let world = &state.world;
        let (mine, theirs) = (&world.mallets[me], &world.mallets[1 - me]);
        out.tick = state.tick();
        out.player = player;
        out.to_act = -1;

        let obs = out.state.get_or_insert_with(Vec::new);
        obs.clear();
        for v in [
            world.puck.pos,
            world.puck.vel,
            mine.pos,
            mine.vel,
            mine.target,
        ] {
            frame.push(obs, v);
        }
        for v in [theirs.pos, theirs.vel] {
            frame.push(obs, v);
        }
        obs.push((f64::from(state.stall_ticks()) / f64::from(state.params.tick_hz)) as f32);

        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        frame.push(signals, theirs.pos);
        frame.push(signals, theirs.vel);

        out.legal.get_or_insert_with(Vec::new).clear();
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(state.score().map(i64::from));
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Match {
        let params = &self.game.params;
        let frame = Frame::of(obs.player);
        let v = obs.state.as_deref().unwrap_or_default();
        let value = |i: usize| match v.get(i) {
            Some(x) if x.is_finite() => f64::from(*x),
            _ => 0.0,
        };
        let read = |i: usize| frame.to_world(Vec2::new(value(i), value(i + 1)));
        let regions = regions(params);
        let me = usize::from(obs.player.min(1));
        let bodies = &self.game.bodies;
        let mallet = |pos: Vec2, vel: Vec2, target: Vec2, side: usize| Mallet {
            pos: regions[side].clamp(pos),
            vel: vel.clamp_length(bodies[side].max_speed()),
            target,
        };
        let mine = mallet(read(4), read(6), read(8), me);
        let theirs = mallet(read(10), read(12), read(10), 1 - me);
        let theirs = Mallet {
            target: theirs.pos,
            ..theirs
        };
        let mallets = if me == 0 {
            [mine, theirs]
        } else {
            [theirs, mine]
        };
        let seen = read(0);
        let y_max = params.half_width - params.puck_radius;
        let y = seen.y.clamp(-y_max, y_max);
        let x_max = if y.abs() < params.mouth_half_width {
            params.half_length.next_down()
        } else {
            params.half_length - params.puck_radius
        };
        let world = World {
            puck: Puck {
                pos: Vec2::new(seen.x.clamp(-x_max, x_max), y),
                vel: read(2).clamp_length(params.puck_speed_cap),
            },
            mallets,
        };
        let limit = (params.stall_seconds * f64::from(params.tick_hz)).round() as u32;
        let stall_ticks = ((value(14) * f64::from(params.tick_hz)).round() as u32).min(limit);
        let score = obs.score.as_deref().unwrap_or_default();
        let goals = |i: usize| {
            score
                .get(i)
                .map_or(0, |&g| u32::try_from(g).unwrap_or(0))
                .min(params.goals_to_win)
        };
        Match::from_observed(
            params.clone(),
            self.game.bodies,
            world,
            obs.tick,
            [goals(0), goals(1)],
            stall_ticks,
        )
    }

    fn simulate(&self, state: &Match, actions: &[ActionT], ticks: u32) -> Match {
        let mut next = state.clone();
        let mut events = Vec::new();
        for _ in 0..ticks {
            for action in actions {
                if let Ok(mut input) = to_input(action, &next.params) {
                    input.tick = next.tick();
                    let _ = next.apply(input);
                }
            }
            events.clear();
            next.step(&mut events);
        }
        next
    }

    fn signals(&self, state: &Match, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        let player = usize::from(action.player.min(1));
        let after = self.simulate(state, std::slice::from_ref(action), 1);
        let mallet = after.world.mallets[player];
        let frame = Frame::of(1 - player as u8);
        frame.push(out, mallet.pos);
        frame.push(out, mallet.vel);
    }

    fn digest(&self, state: &Match) -> u64 {
        state.digest()
    }

    fn result(&self) -> Option<MatchResultT> {
        if !self.game.is_over() {
            return None;
        }
        let mut result = MatchResultT::default();
        result.score = Some(self.game.score().map(i64::from).to_vec());
        result.duration_ticks = self.game.tick();
        result.level = self.level;
        result.seed = self.game.seed();
        Some(result)
    }
}

#[derive(Clone, Copy)]
enum Frame {
    AsIs,
    Rotated,
}

impl Frame {
    fn of(player: PlayerId) -> Self {
        if player == 0 {
            Self::AsIs
        } else {
            Self::Rotated
        }
    }

    fn to_world(self, v: Vec2) -> Vec2 {
        match self {
            Self::AsIs => v,
            Self::Rotated => -v,
        }
    }

    fn push(self, out: &mut Vec<f32>, v: Vec2) {
        let seen = self.to_world(v);
        out.push(seen.x as f32);
        out.push(seen.y as f32);
    }
}

fn to_input(action: &ActionT, params: &Params) -> Result<Input, AdapterError> {
    if action.player > 1 {
        return Err(AdapterError::BadPlayer(action.player));
    }
    let Some(&[x, y]) = action.values.as_deref() else {
        return Err(AdapterError::BadAction("two values"));
    };
    if !(x.is_finite() && y.is_finite()) {
        return Err(AdapterError::BadAction("not finite"));
    }
    let (l, w) = (params.half_length as f32, params.half_width as f32);
    if x.abs() > l || y.abs() > w {
        return Err(AdapterError::BadAction("outside the action space"));
    }
    let side = if action.player == 0 {
        Side::Low
    } else {
        Side::High
    };
    let target = match side {
        Side::Low => [x, y],
        Side::High => [-x, -y],
    };
    Ok(Input {
        tick: action.tick,
        player: action.player,
        target,
    })
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}
