use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventKind, EventT,
    GameAdapter, GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::rules::{Body, Bout, Event, Height, Kind, Params, Phase, STAGE_LEN, Stage, facing};

pub const GAME_VERSION: u32 = 1;
pub const OBSERVATION_LEN: usize = 26;
pub const ACTION_LEN: usize = 5;
const LEFT_CAP: u32 = 1 << 16;
const COUNT_CAP: f64 = 1e6;

pub struct Boxing {
    game: Bout,
    level: u8,
    scratch: Vec<Event>,
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

impl Boxing {
    #[must_use]
    pub fn new(params: Params, level: u8) -> Self {
        if let Err(why) = params.validate() {
            panic!("invalid Params: {why}");
        }
        let body = Body {
            speed: params.max_speed,
            hand: params.max_hand,
        };
        Self {
            game: Bout::new(params, [body; 2], 0),
            level,
            scratch: Vec::with_capacity(4),
        }
    }
}

fn values(action: &ActionT, p: &Params) -> Result<[f64; ACTION_LEN], AdapterError> {
    const WHY: &str = "five values";
    let Some(v) = action.values.as_deref() else {
        return Err(AdapterError::BadAction(WHY));
    };
    let Ok(v) = <[f32; ACTION_LEN]>::try_from(v) else {
        return Err(AdapterError::BadAction(WHY));
    };
    if !v.iter().all(|x| x.is_finite()) {
        return Err(AdapterError::BadAction("not finite"));
    }
    let v = v.map(f64::from);
    let inside = v[0].abs() <= p.ring
        && v[1].abs() <= 1.0
        && (0.0..=2.0).contains(&v[2])
        && v[3].abs() <= 1.0
        && (0.0..=1.0).contains(&v[4]);
    if inside {
        Ok(v)
    } else {
        Err(AdapterError::BadAction("outside the action space"))
    }
}

pub fn apply_to(game: &mut Bout, action: &ActionT) -> Result<(), AdapterError> {
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
    let m = facing(action.player);
    let attack = Kind::from_code(v[2]).map(|k| (k, Height::from_code(v[3])));
    game.order(
        action.player,
        m * v[0],
        Height::from_code(v[1]),
        attack,
        v[4] >= 0.5,
    );
    Ok(())
}

fn last_code(h: Option<Height>) -> f64 {
    h.map_or(0.0, Height::code)
}

impl GameAdapter for Boxing {
    type State = Bout;

    fn spec(&self) -> GameSpecT {
        let p = &self.game.params;
        let ring = p.ring as f32;
        let mut spec = GameSpecT::default();
        spec.game_id = Some("boxing".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::Realtime;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.decision_hz;
        let mut space = ActionSpaceT::default();
        space.continuous = Some(vec![
            dim("move_x", -ring, ring, "m"),
            dim("guard", -1.0, 1.0, "height"),
            dim("attack", 0.0, 2.0, "kind"),
            dim("height", -1.0, 1.0, "height"),
            dim("cancel", 0.0, 1.0, "flag"),
        ]);
        spec.action_space = Some(Box::new(space));
        let mut observation = Vec::new();
        observation.extend([
            dim("my_x", -ring, ring, "m"),
            dim("their_x", -ring, ring, "m"),
        ]);
        for who in ["my", "their"] {
            observation.extend([
                dim(&format!("{who}_stage"), 0.0, 3.0, "code"),
                dim(&format!("{who}_kind"), 0.0, 2.0, "kind"),
                dim(&format!("{who}_height"), -1.0, 1.0, "height"),
                dim(&format!("{who}_left"), 0.0, LEFT_CAP as f32, "ticks"),
                dim(&format!("{who}_blocked"), 0.0, 1.0, "flag"),
                dim(&format!("{who}_guard"), -1.0, 1.0, "height"),
                dim(&format!("{who}_switching"), 0.0, LEFT_CAP as f32, "ticks"),
            ]);
        }
        observation.extend([
            dim("phase", 0.0, 2.0, "code"),
            dim("phase_left", 0.0, LEFT_CAP as f32, "ticks"),
        ]);
        for who in ["my", "their"] {
            observation.extend([
                dim(&format!("{who}_strikes"), 0.0, COUNT_CAP as f32, "count"),
                dim(&format!("{who}_last"), -1.0, 1.0, "height"),
            ]);
        }
        observation.extend([
            dim("my_feints", 0.0, COUNT_CAP as f32, "count"),
            dim("their_feints", 0.0, COUNT_CAP as f32, "count"),
            dim("my_cancelled", 0.0, 1.0, "flag"),
            dim("their_cancelled", 0.0, 1.0, "flag"),
        ]);
        spec.observation = Some(observation);
        spec.body_params = Some(vec![
            dim("speed", 0.0, p.max_speed as f32, "m/s"),
            dim("hand", p.min_hand as f32, p.max_hand as f32, "rate"),
        ]);
        spec.signals = Some(vec![
            dim("windup_height", -1.0, 1.0, "height"),
            dim("windup_kind", 0.0, 2.0, "kind"),
            dim("x", -ring, ring, "m"),
        ]);
        spec.intent_labels = Some(
            ["none", "attack", "guard", "feint"]
                .map(String::from)
                .to_vec(),
        );
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        const WHY: &str = "a speed and a hand rate per player within the declared range";
        let p = &self.game.params;
        let parse = |b: &BodyParamsT| match b.values.as_deref() {
            Some(&[speed, hand])
                if speed.is_finite()
                    && hand.is_finite()
                    && speed > 0.0
                    && f64::from(speed) <= p.max_speed
                    && f64::from(hand) >= p.min_hand
                    && f64::from(hand) <= p.max_hand =>
            {
                Ok(Body {
                    speed: f64::from(speed),
                    hand: f64::from(hand),
                })
            }
            _ => Err(AdapterError::BadBody(WHY)),
        };
        let [a, b] = bodies else {
            return Err(AdapterError::BadBody(WHY));
        };
        let bodies = [parse(a)?, parse(b)?];
        self.game = Bout::new(self.game.params.clone(), bodies, seed);
        Ok(())
    }

    fn live(&self) -> &Bout {
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
            let (kind, player) = match *e {
                Event::Hit { by, .. } => (EventKind::Score, by as i8),
                Event::RoundEnd => (EventKind::RoundEnd, -1),
                _ => continue,
            };
            let mut out = EventT::default();
            out.tick = tick;
            out.kind = kind;
            out.player = player;
            events.push(out);
        }
    }

    fn observe(&self, s: &Bout, player: PlayerId, out: &mut ObservationT) {
        out.tick = s.tick;
        out.player = player;
        out.to_act = -1;
        let me = player.min(1);
        let m = facing(me);
        let mine = &s.fighters[usize::from(me)];
        let theirs = &s.fighters[usize::from(1 - me)];
        let state = out.state.get_or_insert_with(Vec::new);
        state.clear();
        state.extend([m * mine.x, m * theirs.x].map(|v| v as f32));
        for f in [mine, theirs] {
            state.extend(f.stage.code().map(|v| v as f32));
            state.extend([f.guard.code(), f64::from(f.switching)].map(|v| v as f32));
        }
        state.extend([s.phase.code(), f64::from(s.phase_left)].map(|v| v as f32));
        for f in [mine, theirs] {
            state.extend([f64::from(f.strikes), last_code(f.last)].map(|v| v as f32));
        }
        state.extend([f64::from(mine.feints), f64::from(theirs.feints)].map(|v| v as f32));
        state.extend([mine.cancelled, theirs.cancelled].map(|c| f32::from(u8::from(c))));
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        signals.extend(windup_signal(theirs.stage, m * theirs.x));
        out.legal.get_or_insert_with(Vec::new).clear();
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(s.score.map(i64::from));
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Bout {
        let p = &self.game.params;
        let v = obs.state.as_deref().unwrap_or_default();
        let value = |i: usize| match v.get(i) {
            Some(x) if x.is_finite() => f64::from(*x),
            _ => 0.0,
        };
        let count = |i: usize| value(i).round().clamp(0.0, COUNT_CAP) as u32;
        let me = obs.player.min(1);
        let them = 1 - me;
        let m = facing(me);
        let mut s = Bout::new(p.clone(), self.game.bodies, self.game.seed);
        s.tick = obs.tick.min(p.match_ticks());
        let template = s.fighters[0];
        let fighter = |x: usize, base: usize, strikes: usize, feints: usize| {
            let mut f = template;
            f.x = (m * value(x)).clamp(-p.ring, p.ring);
            f.target = f.x;
            let mut code = [0.0; STAGE_LEN];
            for (k, c) in code.iter_mut().enumerate() {
                *c = value(base + k);
            }
            f.stage = Stage::from_code(code, LEFT_CAP);
            f.guard = Height::from_code(value(base + STAGE_LEN));
            f.switching = (value(base + STAGE_LEN + 1)
                .round()
                .clamp(0.0, f64::from(p.guard_switch))) as u32;
            f.queued = None;
            f.cancel = false;
            f.strikes = count(strikes);
            f.last = match value(strikes + 1) {
                c if c > 0.5 => Some(Height::High),
                c if c < -0.5 => Some(Height::Low),
                _ => None,
            };
            f.feints = count(feints);
            f.cancelled = value(feints + 2) >= 0.5;
            f
        };
        s.fighters[usize::from(me)] = fighter(0, 2, 18, 22);
        s.fighters[usize::from(them)] = fighter(1, 9, 20, 23);
        if s.fighters[1].x - s.fighters[0].x < p.min_gap {
            s.fighters[0].x = s.fighters[0].x.min(p.ring - p.min_gap);
            s.fighters[1].x = s.fighters[0].x + p.min_gap;
        }
        s.phase = Phase::from_code(value(16));
        s.phase_left = (value(17).round().clamp(0.0, f64::from(p.break_ticks))) as u32;
        let score = obs.score.as_deref().unwrap_or_default();
        let points = |i: usize| {
            score
                .get(i)
                .map_or(0, |&g| u32::try_from(g).unwrap_or(0))
                .min(p.points_to_win)
        };
        s.score = [points(0), points(1)];
        s
    }

    fn simulate(&self, state: &Bout, actions: &[ActionT], ticks: u32) -> Bout {
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

    fn signals(&self, state: &Bout, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        let after = self.simulate(state, std::slice::from_ref(action), 1);
        let seat = action.player.min(1);
        let f = &after.fighters[usize::from(seat)];
        out.extend(windup_signal(f.stage, facing(1 - seat) * f.x));
    }

    fn digest(&self, state: &Bout) -> u64 {
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

fn windup_signal(stage: Stage, x: f64) -> [f32; 3] {
    match stage {
        Stage::WindUp { kind, height, .. } => [height.code(), kind.code(), x],
        _ => [0.0, 0.0, x],
    }
    .map(|v| v as f32)
}
