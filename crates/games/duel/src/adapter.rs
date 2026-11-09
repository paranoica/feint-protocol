use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventKind, EventT,
    GameAdapter, GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::rules::{Body, Bullet, Duel, Gunner, Params, Phase, RoundEvent};

pub const GAME_VERSION: u32 = 1;
pub const OBSERVATION_LEN: usize = 29;

pub struct QuickDraw {
    game: Duel,
    level: u8,
    scratch: Vec<RoundEvent>,
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

impl QuickDraw {
    #[must_use]
    pub fn new(params: Params, level: u8) -> Self {
        if let Err(why) = params.validate() {
            panic!("invalid Params: {why}");
        }
        let body = Body {
            draw_rate: params.max_draw_rate,
            step_rate: params.max_step_rate,
        };
        Self {
            game: Duel::new(params, [body; 2], 0),
            level,
            scratch: Vec::with_capacity(4),
        }
    }

    pub fn game_mut(&mut self) -> &mut Duel {
        &mut self.game
    }
}

fn input(action: &ActionT) -> Result<[f64; 3], AdapterError> {
    let Some(&[a, b, c]) = action.values.as_deref() else {
        return Err(AdapterError::BadAction("three values"));
    };
    if ![a, b, c].iter().all(|v| v.is_finite()) {
        return Err(AdapterError::BadAction("not finite"));
    }
    Ok([a, b, c].map(f64::from))
}

pub fn apply_to(game: &mut Duel, action: &ActionT) -> Result<(), AdapterError> {
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
    let [aim, hold, roll] = input(action)?;
    game.steer(action.player, aim, hold >= 0.5);
    if roll <= -0.5 {
        game.roll(action.player, -1);
    } else if roll >= 0.5 {
        game.roll(action.player, 1);
    }
    Ok(())
}

fn gunner_values(g: &Gunner, own: bool, hz: f64) -> [f64; 9] {
    [
        g.x,
        g.hand,
        f64::from(u8::from(g.committed)),
        f64::from(u8::from(g.loaded)),
        f64::from(g.recovery) / hz,
        if own {
            g.aim.unwrap_or(g.aim_target)
        } else {
            0.0
        },
        f64::from(g.reload) / hz,
        f64::from(g.shots),
        f64::from(g.last_response),
    ]
}

fn bullet_values(b: Option<Bullet>, hz: f64) -> [f64; 4] {
    b.map_or([0.0, 0.0, 0.0, 0.0], |b| {
        [1.0, b.x, f64::from(b.t) / hz, b.lead]
    })
}

impl GameAdapter for QuickDraw {
    type State = Duel;

    fn spec(&self) -> GameSpecT {
        let p = &self.game.params;
        let w = p.half_width as f32;
        let mut spec = GameSpecT::default();
        spec.game_id = Some("duel".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::Realtime;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.decision_hz;
        let mut space = ActionSpaceT::default();
        space.continuous = Some(vec![
            dim("aim", -w, w, "m"),
            dim("draw", 0.0, 1.0, "flag"),
            dim("roll", -1.0, 1.0, "dir"),
        ]);
        spec.action_space = Some(Box::new(space));
        let mut observation = Vec::new();
        for who in ["my", "their"] {
            observation.extend([
                dim(&format!("{who}_x"), -w, w, "m"),
                dim(&format!("{who}_hand"), 0.0, 1.0, "share"),
                dim(&format!("{who}_committed"), 0.0, 1.0, "flag"),
                dim(&format!("{who}_loaded"), 0.0, 1.0, "flag"),
                dim(&format!("{who}_recovery"), 0.0, 1.0, "s"),
                dim(&format!("{who}_aim"), -w, w, "m"),
                dim(&format!("{who}_reload"), 0.0, 2.0, "s"),
                dim(&format!("{who}_shots"), 0.0, 1000.0, "count"),
                dim(&format!("{who}_last_response"), -2.0, 2.0, "code"),
            ]);
        }
        for who in ["my", "their"] {
            observation.extend([
                dim(&format!("{who}_bullet"), 0.0, 1.0, "flag"),
                dim(&format!("{who}_bullet_x"), -w, w, "m"),
                dim(&format!("{who}_bullet_time"), 0.0, 1.0, "s"),
                dim(&format!("{who}_bullet_lead"), -2.0 * w, 2.0 * w, "m"),
            ]);
        }
        observation.extend([
            dim("phase", 0.0, 3.0, "code"),
            dim("phase_time", 0.0, 20.0, "s"),
            dim("round", 0.0, p.max_rounds as f32, "count"),
        ]);
        spec.observation = Some(observation);
        spec.body_params = Some(vec![
            dim("draw_rate", 0.0, p.max_draw_rate as f32, "1/s"),
            dim("step_rate", 0.0, p.max_step_rate as f32, "1/s"),
        ]);
        spec.signals = Some(vec![
            dim("hand", 0.0, 1.0, "share"),
            dim("x", -w, w, "m"),
            dim("recovery", 0.0, 1.0, "s"),
        ]);
        spec.intent_labels = Some(
            ["none", "shoot", "dodge", "fake"]
                .map(String::from)
                .to_vec(),
        );
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        const WHY: &str = "two positive finite values per player within the declared maxima";
        let p = &self.game.params;
        let parse = |b: &BodyParamsT| match b.values.as_deref() {
            Some(&[draw, roll])
                if draw.is_finite()
                    && roll.is_finite()
                    && draw > 0.0
                    && roll > 0.0
                    && f64::from(draw) <= p.max_draw_rate
                    && f64::from(roll) <= p.max_step_rate =>
            {
                Ok(Body {
                    draw_rate: f64::from(draw),
                    step_rate: f64::from(roll),
                })
            }
            _ => Err(AdapterError::BadBody(WHY)),
        };
        let [a, b] = bodies else {
            return Err(AdapterError::BadBody(WHY));
        };
        let bodies = [parse(a)?, parse(b)?];
        self.game = Duel::new(self.game.params.clone(), bodies, seed);
        Ok(())
    }

    fn live(&self) -> &Duel {
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
                RoundEvent::Hit { shooter } => {
                    out.kind = EventKind::Score;
                    out.player = shooter as i8;
                }
                RoundEvent::Void => {
                    out.kind = EventKind::RoundEnd;
                    out.player = -1;
                }
                RoundEvent::Over => continue,
            }
            events.push(out);
        }
    }

    fn observe(&self, s: &Duel, player: PlayerId, out: &mut ObservationT) {
        out.tick = s.tick;
        out.player = player;
        out.to_act = -1;
        let hz = f64::from(s.params.tick_hz);
        let me = usize::from(player.min(1));
        let them = 1 - me;
        let state = out.state.get_or_insert_with(Vec::new);
        state.clear();
        let values = gunner_values(&s.gunners[me], true, hz)
            .into_iter()
            .chain(gunner_values(&s.gunners[them], false, hz))
            .chain(bullet_values(s.bullets[me], hz))
            .chain(bullet_values(s.bullets[them], hz))
            .chain([s.phase.code(), f64::from(s.t) / hz, f64::from(s.round)]);
        state.extend(values.map(|v| v as f32));
        let opp = &s.gunners[them];
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        signals.extend([opp.hand, opp.x, f64::from(opp.recovery) / hz].map(|v| v as f32));
        out.legal.get_or_insert_with(Vec::new).clear();
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(s.score.map(i64::from));
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Duel {
        let p = &self.game.params;
        let v = obs.state.as_deref().unwrap_or_default();
        let value = |i: usize| match v.get(i) {
            Some(x) if x.is_finite() => f64::from(*x),
            _ => 0.0,
        };
        let hz = f64::from(p.tick_hz);
        let w = p.half_width;
        let me = usize::from(obs.player.min(1));
        let them = 1 - me;
        let ticks = |s: f64, cap: u32| ((s * hz).round().max(0.0) as u32).min(cap);
        let mut s = Duel::new(p.clone(), self.game.bodies, 0);
        s.tick = obs.tick;
        let caps = [s.recovery_ticks(0), s.recovery_ticks(1)];
        let gunner = |base: usize, seat: usize, own: bool| {
            let recovery_cap = caps[seat];
            let committed = value(base + 2) >= 0.5;
            let mut g = Gunner {
                x: value(base).clamp(-w, w),
                hand: value(base + 1).clamp(0.0, 1.0),
                holding: committed,
                committed,
                aim_target: 0.0,
                aim: None,
                lead: 0.0,
                loaded: value(base + 3) >= 0.5,
                roll: None,
                recovery: ticks(value(base + 4), recovery_cap),
                rolls: 0,
                last_roll: 0,
                reload: ticks(value(base + 6), p.reload_ticks),
                shots: value(base + 7).round().clamp(0.0, 1e6) as u32,
                response: 0,
                last_response: value(base + 8).round().clamp(-2.0, 2.0) as i8,
            };
            if own {
                g.aim_target = value(base + 5).clamp(-w, w);
                g.aim = committed.then_some(g.aim_target);
            }
            g
        };
        let mine = gunner(0, me, true);
        let theirs = gunner(9, them, false);
        s.gunners[me] = mine;
        s.gunners[them] = theirs;
        if s.gunners[them].committed {
            s.gunners[them].aim = Some(s.gunners[me].x);
        }
        let bullet = |base: usize| {
            (value(base) >= 0.5).then(|| Bullet {
                x: value(base + 1).clamp(-w, w),
                t: ticks(value(base + 2), p.flight_ticks),
                lead: value(base + 3).clamp(-2.0 * w, 2.0 * w),
            })
        };
        s.bullets[me] = bullet(18);
        s.bullets[them] = bullet(22);
        s.phase = Phase::from_code(value(26));
        s.t = ticks(
            value(27),
            p.round_ticks.max(p.ready_ticks).max(p.settle_ticks),
        );
        s.round = (value(28).round().max(0.0) as u32).min(p.max_rounds);
        let score = obs.score.as_deref().unwrap_or_default();
        let wins = |i: usize| {
            score
                .get(i)
                .map_or(0, |&g| u32::try_from(g).unwrap_or(0))
                .min(p.max_rounds)
        };
        s.score = [wins(0), wins(1)];
        s
    }

    fn simulate(&self, state: &Duel, actions: &[ActionT], ticks: u32) -> Duel {
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

    fn signals(&self, state: &Duel, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        let after = self.simulate(state, std::slice::from_ref(action), 1);
        let g = &after.gunners[usize::from(action.player.min(1))];
        let hz = f64::from(after.params.tick_hz);
        out.extend([g.hand, g.x, f64::from(g.recovery) / hz].map(|v| v as f32));
    }

    fn digest(&self, state: &Duel) -> u64 {
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
