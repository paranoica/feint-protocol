use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventKind, EventT,
    GameAdapter, GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::rules::{Ball, Body, Params, Phase, PointEvent, Swing, Table};

pub const GAME_VERSION: u32 = 1;
pub const OBSERVATION_LEN: usize = 23;
pub const ACTION_LEN: usize = 7;

pub struct PingPong {
    game: Table,
    level: u8,
    scratch: Vec<PointEvent>,
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

#[must_use]
pub fn mirror(seat: u8) -> f64 {
    if seat == 0 { 1.0 } else { -1.0 }
}

impl PingPong {
    #[must_use]
    pub fn new(params: Params, level: u8) -> Self {
        if let Err(why) = params.validate() {
            panic!("invalid Params: {why}");
        }
        let body = Body {
            speed: params.max_speed,
            accel: params.max_accel,
        };
        Self {
            game: Table::new(params, [body; 2], 0),
            level,
            scratch: Vec::with_capacity(4),
        }
    }
}

fn values(action: &ActionT, p: &Params) -> Result<[f64; ACTION_LEN], AdapterError> {
    let Some(v) = action.values.as_deref() else {
        return Err(AdapterError::BadAction("seven values"));
    };
    let Ok(v) = <[f32; ACTION_LEN]>::try_from(v) else {
        return Err(AdapterError::BadAction("seven values"));
    };
    if !v.iter().all(|x| x.is_finite()) {
        return Err(AdapterError::BadAction("not finite"));
    }
    let v = v.map(f64::from);
    let inside = v[0].abs() <= p.half_length + p.back_room
        && v[1].abs() <= p.half_width + p.side_room
        && (0.0..=1.0).contains(&v[2])
        && v[3].abs() <= p.half_length + p.out_room
        && v[4].abs() <= p.half_width + p.out_room
        && (0.0..=2.0 * p.max_flight).contains(&v[5])
        && v[6].abs() <= 1.0;
    if inside {
        Ok(v)
    } else {
        Err(AdapterError::BadAction("outside the action space"))
    }
}

pub fn apply_to(game: &mut Table, action: &ActionT) -> Result<(), AdapterError> {
    if action.player > 1 {
        return Err(AdapterError::BadPlayer(action.player));
    }
    if game.over() {
        return Err(AdapterError::MatchOver);
    }
    if action.tick != game.tick {
        return Err(AdapterError::WrongTick {
            expected: game.tick,
            got: action.tick,
        });
    }
    let v = values(action, &game.params)?;
    let m = mirror(action.player);
    let swing = (v[2] >= 0.5).then_some(Swing {
        aim: [v[3], m * v[4]],
        flight: v[5],
    });
    game.steer(action.player, [m * v[0], m * v[1]], swing, m * v[6]);
    Ok(())
}

impl GameAdapter for PingPong {
    type State = Table;

    fn spec(&self) -> GameSpecT {
        let p = &self.game.params;
        let reach_x = (p.half_length + p.back_room) as f32;
        let reach_y = (p.half_width + p.side_room) as f32;
        let (l, w) = (
            (p.half_length + p.out_room) as f32,
            (p.half_width + p.out_room) as f32,
        );
        let mut spec = GameSpecT::default();
        spec.game_id = Some("pingpong".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::Realtime;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.decision_hz;
        let mut space = ActionSpaceT::default();
        space.continuous = Some(vec![
            dim("move_x", -reach_x, reach_x, "m"),
            dim("move_y", -reach_y, reach_y, "m"),
            dim("swing", 0.0, 1.0, "flag"),
            dim("aim_depth", -l, l, "m"),
            dim("aim_y", -w, w, "m"),
            dim("flight", 0.0, 2.0 * p.max_flight as f32, "s"),
            dim("facing", -1.0, 1.0, "dir"),
        ]);
        spec.action_space = Some(Box::new(space));
        let mut observation = Vec::new();
        for who in ["my", "their"] {
            observation.extend([
                dim(&format!("{who}_x"), -reach_x, reach_x, "m"),
                dim(&format!("{who}_y"), -reach_y, reach_y, "m"),
                dim(&format!("{who}_vx"), -10.0, 10.0, "m/s"),
                dim(&format!("{who}_vy"), -10.0, 10.0, "m/s"),
            ]);
        }
        observation.extend([
            dim("ball_x", -10.0, 10.0, "m"),
            dim("ball_y", -10.0, 10.0, "m"),
            dim("ball_z", -1.0, 5.0, "m"),
            dim("ball_vx", -20.0, 20.0, "m/s"),
            dim("ball_vy", -20.0, 20.0, "m/s"),
            dim("ball_vz", -20.0, 20.0, "m/s"),
            dim("my_facing", -1.0, 1.0, "dir"),
            dim("their_facing", -1.0, 1.0, "dir"),
            dim("phase", 0.0, 3.0, "code"),
            dim("phase_time", 0.0, 10.0, "s"),
            dim("my_serve", 0.0, 1.0, "flag"),
            dim("my_turn", 0.0, 1.0, "flag"),
            dim("bounces", 0.0, 1.0, "count"),
            dim("hits", 0.0, 1e6, "count"),
            dim("rally", 0.0, 100.0, "count"),
        ]);
        spec.observation = Some(observation);
        spec.body_params = Some(vec![
            dim("speed", 0.0, p.max_speed as f32, "m/s"),
            dim("accel", 0.0, p.max_accel as f32, "m/s2"),
        ]);
        spec.signals = Some(vec![
            dim("facing", -1.0, 1.0, "dir"),
            dim("x", -reach_x, reach_x, "m"),
            dim("y", -reach_y, reach_y, "m"),
        ]);
        spec.intent_labels = Some(
            ["none", "shot", "cover", "disguise"]
                .map(String::from)
                .to_vec(),
        );
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        const WHY: &str = "two positive finite values per player within the declared maxima";
        let p = &self.game.params;
        let parse = |b: &BodyParamsT| match b.values.as_deref() {
            Some(&[speed, accel])
                if speed.is_finite()
                    && accel.is_finite()
                    && speed > 0.0
                    && accel > 0.0
                    && f64::from(speed) <= p.max_speed
                    && f64::from(accel) <= p.max_accel =>
            {
                Ok(Body {
                    speed: f64::from(speed),
                    accel: f64::from(accel),
                })
            }
            _ => Err(AdapterError::BadBody(WHY)),
        };
        let [a, b] = bodies else {
            return Err(AdapterError::BadBody(WHY));
        };
        let bodies = [parse(a)?, parse(b)?];
        self.game = Table::new(self.game.params.clone(), bodies, seed);
        Ok(())
    }

    fn live(&self) -> &Table {
        &self.game
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        apply_to(&mut self.game, action)
    }

    fn advance(&mut self, events: &mut Vec<EventT>) {
        self.scratch.clear();
        let tick = self.game.tick;
        self.game.step(&mut self.scratch);
        for e in &self.scratch {
            if let PointEvent::Point { winner } = *e {
                let mut out = EventT::default();
                out.tick = tick;
                out.kind = EventKind::Score;
                out.player = winner as i8;
                events.push(out);
            }
        }
    }

    fn observe(&self, s: &Table, player: PlayerId, out: &mut ObservationT) {
        out.tick = s.tick;
        out.player = player;
        out.to_act = -1;
        let me = player.min(1);
        let m = mirror(me);
        let hz = f64::from(s.params.tick_hz);
        let mine = s.paddles[usize::from(me)];
        let theirs = s.paddles[usize::from(1 - me)];
        let b = s.ball;
        let state = out.state.get_or_insert_with(Vec::new);
        state.clear();
        let values = [
            m * mine.pos[0],
            m * mine.pos[1],
            m * mine.vel[0],
            m * mine.vel[1],
            m * theirs.pos[0],
            m * theirs.pos[1],
            m * theirs.vel[0],
            m * theirs.vel[1],
            m * b.pos[0],
            m * b.pos[1],
            b.pos[2],
            m * b.vel[0],
            m * b.vel[1],
            b.vel[2],
            m * mine.facing,
            m * theirs.facing,
            s.phase.code(),
            f64::from(s.t) / hz,
            f64::from(u8::from(s.server == me)),
            f64::from(u8::from(s.to_hit() == Some(me))),
            f64::from(s.bounces),
            f64::from(s.hits),
            f64::from(s.rally),
        ];
        state.extend(values.map(|v| v as f32));
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        signals.extend([m * theirs.facing, m * theirs.pos[0], m * theirs.pos[1]].map(|v| v as f32));
        out.legal.get_or_insert_with(Vec::new).clear();
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(s.score.map(i64::from));
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Table {
        let p = &self.game.params;
        let v = obs.state.as_deref().unwrap_or_default();
        let value = |i: usize| match v.get(i) {
            Some(x) if x.is_finite() => f64::from(*x),
            _ => 0.0,
        };
        let me = obs.player.min(1);
        let them = 1 - me;
        let m = mirror(me);
        let hz = f64::from(p.tick_hz);
        let mut s = Table::new(p.clone(), self.game.bodies, 0);
        s.tick = obs.tick;
        let template = s.paddles[0];
        let paddle = |base: usize, facing: f64| {
            let mut pad = template;
            pad.pos = [m * value(base), m * value(base + 1)];
            pad.vel = [m * value(base + 2), m * value(base + 3)];
            pad.target = pad.pos;
            pad.facing = (m * facing).clamp(-1.0, 1.0);
            pad.swing = None;
            pad
        };
        s.paddles[usize::from(me)] = paddle(0, value(14));
        s.paddles[usize::from(them)] = paddle(4, value(15));
        s.ball = Ball {
            pos: [m * value(8), m * value(9), value(10)],
            vel: [m * value(11), m * value(12), value(13)],
        };
        s.phase = Phase::from_code(value(16));
        s.t = ((value(17) * hz).round().max(0.0) as u32).min(p.serve_ticks.max(p.settle_ticks));
        s.server = if value(18) >= 0.5 { me } else { them };
        s.bounces = u8::from(value(20) >= 0.5);
        s.hits = value(21).round().clamp(0.0, 1e6) as u32;
        s.rally = value(22).round().clamp(0.0, 100.0) as u32;
        s.hitter = (s.phase == Phase::Rally).then(|| {
            if value(19) >= 0.5 || value(11) < 0.0 {
                them
            } else {
                me
            }
        });
        let score = obs.score.as_deref().unwrap_or_default();
        let points = |i: usize| {
            score
                .get(i)
                .map_or(0, |&g| u32::try_from(g).unwrap_or(0))
                .min(p.max_points)
        };
        s.score = [points(0), points(1)];
        s
    }

    fn simulate(&self, state: &Table, actions: &[ActionT], ticks: u32) -> Table {
        let mut next = state.clone();
        let mut events = Vec::new();
        for _ in 0..ticks {
            for action in actions {
                let mut stamped = action.clone();
                stamped.tick = next.tick;
                let _ = apply_to(&mut next, &stamped);
            }
            events.clear();
            next.step(&mut events);
        }
        next
    }

    fn signals(&self, state: &Table, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        let after = self.simulate(state, std::slice::from_ref(action), 1);
        let seat = action.player.min(1);
        let pad = after.paddles[usize::from(seat)];
        let m = mirror(1 - seat);
        out.extend([m * pad.facing, m * pad.pos[0], m * pad.pos[1]].map(|v| v as f32));
    }

    fn digest(&self, state: &Table) -> u64 {
        state.digest()
    }

    fn result(&self) -> Option<MatchResultT> {
        if !self.game.over() {
            return None;
        }
        let mut result = MatchResultT::default();
        result.score = Some(self.game.score.map(i64::from).to_vec());
        result.duration_ticks = self.game.tick;
        result.level = self.level;
        result.seed = self.game.seed;
        Some(result)
    }
}
