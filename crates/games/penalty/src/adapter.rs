use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventKind, EventT,
    GameAdapter, GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::rules::{Body, Dive, KickEvent, Params, Phase, Shootout};

pub const GAME_VERSION: u32 = 2;
pub const OBSERVATION_LEN: usize = 14;

pub struct Penalty {
    game: Shootout,
    level: u8,
    scratch: Vec<KickEvent>,
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

impl Penalty {
    #[must_use]
    pub fn new(params: Params, level: u8) -> Self {
        if let Err(why) = params.validate() {
            panic!("invalid Params: {why}");
        }
        let body = Body {
            dive_speed: params.max_dive_speed,
            lean_rate: params.max_lean_rate,
        };
        Self {
            game: Shootout::new(params, [body; 2], 0),
            level,
            scratch: Vec::with_capacity(4),
        }
    }

    pub fn game_mut(&mut self) -> &mut Shootout {
        &mut self.game
    }
}

fn input(action: &ActionT) -> Result<[f64; 4], AdapterError> {
    let Some(&[a, b, c, d]) = action.values.as_deref() else {
        return Err(AdapterError::BadAction("four values"));
    };
    if ![a, b, c, d].iter().all(|v| v.is_finite()) {
        return Err(AdapterError::BadAction("not finite"));
    }
    Ok([a, b, c, d].map(f64::from))
}

pub fn apply_to(game: &mut Shootout, action: &ActionT) -> Result<(), AdapterError> {
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
    let [a, b, c, d] = input(action)?;
    if action.player == game.shooter() {
        game.steer(a, b >= 0.5);
        if c >= 0.5 {
            game.kick(d);
        }
    } else {
        game.guard(a);
        if c >= 0.5 {
            game.dive_to(d);
        }
    }
    Ok(())
}

impl GameAdapter for Penalty {
    type State = Shootout;

    fn spec(&self) -> GameSpecT {
        let p = &self.game.params;
        let h = p.half_goal as f32;
        let wide = (p.half_goal + p.wide) as f32;
        let mut spec = GameSpecT::default();
        spec.game_id = Some("penalty".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::Realtime;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.decision_hz;
        let mut space = ActionSpaceT::default();
        space.continuous = Some(vec![
            dim("lean_or_guard", -h, h, "m"),
            dim("run", 0.0, 1.0, "flag"),
            dim("strike", 0.0, 1.0, "flag"),
            dim("aim_or_dive", -wide, wide, "m"),
        ]);
        spec.action_space = Some(Box::new(space));
        spec.observation = Some(vec![
            dim("role", 0.0, 1.0, "keeper"),
            dim("phase", 0.0, 5.0, "code"),
            dim("phase_time", 0.0, 2.0, "s"),
            dim("distance", 0.0, p.run_up as f32, "m"),
            dim("lean", -1.0, 1.0, "share"),
            dim("running", 0.0, 1.0, "flag"),
            dim("ball_y", -wide, wide, "m"),
            dim("keeper_y", -h, h, "m"),
            dim("keeper_target", -h, h, "m"),
            dim("diving", 0.0, 1.0, "flag"),
            dim("dive_target", -h, h, "m"),
            dim("dive_left", 0.0, 1.0, "s"),
            dim("dived", 0.0, 1.0, "flag"),
            dim("kicks", 0.0, p.max_kicks as f32, "count"),
        ]);
        spec.body_params = Some(vec![
            dim("dive_speed", 0.0, p.max_dive_speed as f32, "m/s"),
            dim("lean_rate", 0.0, p.max_lean_rate as f32, "1/s"),
        ]);
        spec.signals = Some(vec![
            dim("lean", -1.0, 1.0, "share"),
            dim("distance", 0.0, p.run_up as f32, "m"),
            dim("keeper_y", -h, h, "m"),
            dim("dive_target", -h, h, "m"),
        ]);
        spec.intent_labels = Some(
            ["none", "strike", "defend", "fake"]
                .map(String::from)
                .to_vec(),
        );
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        const WHY: &str = "two positive finite values per player within the declared maxima";
        let p = &self.game.params;
        let parse = |b: &BodyParamsT| match b.values.as_deref() {
            Some(&[dive, lean])
                if dive.is_finite()
                    && lean.is_finite()
                    && dive > 0.0
                    && lean > 0.0
                    && f64::from(dive) <= p.max_dive_speed
                    && f64::from(lean) <= p.max_lean_rate =>
            {
                Ok(Body {
                    dive_speed: f64::from(dive),
                    lean_rate: f64::from(lean),
                })
            }
            _ => Err(AdapterError::BadBody(WHY)),
        };
        let [a, b] = bodies else {
            return Err(AdapterError::BadBody(WHY));
        };
        let bodies = [parse(a)?, parse(b)?];
        self.game = Shootout::new(self.game.params.clone(), bodies, seed);
        Ok(())
    }

    fn live(&self) -> &Shootout {
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
            let mut out = EventT::default();
            out.tick = tick;
            match *e {
                KickEvent::Goal { shooter } => {
                    out.kind = EventKind::Score;
                    out.player = shooter as i8;
                }
                KickEvent::Saved { shooter } => {
                    out.kind = EventKind::RoundEnd;
                    out.player = (1 - shooter) as i8;
                }
                KickEvent::Over => continue,
            }
            events.push(out);
        }
    }

    fn observe(&self, s: &Shootout, player: PlayerId, out: &mut ObservationT) {
        out.tick = s.tick;
        out.player = player;
        out.to_act = -1;
        let hz = f64::from(s.params.tick_hz);
        let dive = s.dive.unwrap_or(Dive {
            target: s.keeper_y,
            left: 0,
        });
        let state = out.state.get_or_insert_with(Vec::new);
        state.clear();
        state.extend(
            [
                f64::from(u8::from(player != s.shooter())),
                s.phase.code(),
                f64::from(s.t) / hz,
                s.dist,
                s.lean,
                f64::from(u8::from(s.running)),
                s.ball_y,
                s.keeper_y,
                s.keeper_target,
                f64::from(u8::from(dive.left > 0)),
                dive.target,
                f64::from(dive.left) / hz,
                f64::from(u8::from(s.dived)),
                f64::from(s.kicks),
            ]
            .map(|v| v as f32),
        );
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        signals.extend([s.lean, s.dist, s.keeper_y, dive.target].map(|v| v as f32));
        out.legal.get_or_insert_with(Vec::new).clear();
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(s.score.map(i64::from));
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Shootout {
        let p = &self.game.params;
        let v = obs.state.as_deref().unwrap_or_default();
        let value = |i: usize| match v.get(i) {
            Some(x) if x.is_finite() => f64::from(*x),
            _ => 0.0,
        };
        let hz = f64::from(p.tick_hz);
        let h = p.half_goal;
        let limit = p.half_goal + p.wide;
        let mut s = Shootout::new(p.clone(), self.game.bodies, 0);
        s.tick = obs.tick;
        s.kicks = (value(13).round().max(0.0) as u32).min(p.max_kicks);
        s.phase = Phase::from_code(value(1));
        s.t = ((value(2) * hz).round().max(0.0) as u32).min(10 * p.tick_hz);
        s.dist = value(3).clamp(0.0, p.run_up);
        s.lean = value(4).clamp(-1.0, 1.0);
        s.lean_target = s.lean;
        s.running = value(5) >= 0.5;
        s.ball_y = value(6).clamp(-limit, limit);
        s.keeper_y = value(7).clamp(-h, h);
        s.keeper_target = value(8).clamp(-h, h);
        let left = ((value(11) * hz).round().max(0.0) as u32).min(p.dive_ticks);
        s.dive = (value(9) >= 0.5).then_some(Dive {
            target: value(10).clamp(-h, h),
            left,
        });
        s.dived = value(12) >= 0.5 || s.dive.is_some();
        if s.dived && s.dive.is_none() {
            s.dive = Some(Dive {
                target: s.keeper_y,
                left: 0,
            });
        }
        if s.phase == Phase::Flight && s.t > 0 {
            let f = f64::from(p.flight_ticks);
            s.aim = Some((s.ball_y * f / f64::from(s.t.min(p.flight_ticks))).clamp(-limit, limit));
        }
        let score = obs.score.as_deref().unwrap_or_default();
        let goals = |i: usize| {
            score
                .get(i)
                .map_or(0, |&g| u32::try_from(g).unwrap_or(0))
                .min(p.max_kicks)
        };
        s.score = [goals(0), goals(1)];
        s
    }

    fn simulate(&self, state: &Shootout, actions: &[ActionT], ticks: u32) -> Shootout {
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

    fn signals(&self, state: &Shootout, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        let after = self.simulate(state, std::slice::from_ref(action), 1);
        let dive = after.dive.map_or(after.keeper_y, |d| d.target);
        out.extend([after.lean, after.dist, after.keeper_y, dive].map(|v| v as f32));
    }

    fn digest(&self, state: &Shootout) -> u64 {
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
