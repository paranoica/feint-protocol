#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub half_goal: f64,
    pub keeper_reach: f64,
    pub run_up: f64,
    pub run_speed: f64,
    pub kick_range: f64,
    pub flight_ticks: u32,
    pub windup_ticks: u32,
    pub dive_ticks: u32,
    pub settle_ticks: u32,
    pub ball_wait_ticks: u32,
    pub kicks_each: u32,
    pub max_kicks: u32,
    pub tick_hz: u32,
    pub decision_hz: u32,
    pub max_dive_speed: f64,
    pub max_lean_rate: f64,
    pub walk_share: f64,
    pub wide: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            half_goal: 3.66,
            keeper_reach: 0.9,
            run_up: 3.0,
            run_speed: 3.0,
            kick_range: 0.65,
            flight_ticks: 106,
            windup_ticks: 48,
            dive_ticks: 108,
            settle_ticks: 168,
            ball_wait_ticks: 360,
            kicks_each: 5,
            max_kicks: 20,
            tick_hz: 240,
            decision_hz: 60,
            max_dive_speed: 9.0,
            max_lean_rate: 4.0,
            walk_share: 0.3,
            wide: 0.8,
        }
    }
}

impl Params {
    #[must_use]
    pub fn dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let positive = [
            self.half_goal,
            self.keeper_reach,
            self.run_up,
            self.run_speed,
            self.max_dive_speed,
            self.max_lean_rate,
            self.walk_share,
        ];
        if !positive.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("sizes and speeds must be positive and finite");
        }
        if self.flight_ticks == 0
            || self.windup_ticks == 0
            || self.dive_ticks == 0
            || self.tick_hz == 0
        {
            return Err("phases need ticks");
        }
        if self.decision_hz == 0 || self.decision_hz > self.tick_hz {
            return Err("decisions must be positive and no faster than ticks");
        }
        if self.kicks_each == 0 || self.max_kicks < 2 * self.kicks_each {
            return Err("a shootout needs kicks for both sides");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub dive_speed: f64,
    pub lean_rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    RunUp,
    AtBall,
    Strike,
    Flight,
    Settle,
    Over,
}

impl Phase {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::RunUp => 0.0,
            Self::AtBall => 1.0,
            Self::Flight => 2.0,
            Self::Settle => 3.0,
            Self::Over => 4.0,
            Self::Strike => 5.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Self {
        match code.round() as i64 {
            1 => Self::AtBall,
            2 => Self::Flight,
            3 => Self::Settle,
            4 => Self::Over,
            5 => Self::Strike,
            _ => Self::RunUp,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dive {
    pub target: f64,
    pub left: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shootout {
    pub params: Params,
    pub bodies: [Body; 2],
    pub seed: u64,
    pub tick: u64,
    pub kicks: u32,
    pub score: [u32; 2],
    pub phase: Phase,
    pub t: u32,
    pub dist: f64,
    pub lean: f64,
    pub lean_target: f64,
    pub running: bool,
    pub aim: Option<f64>,
    pub ball_y: f64,
    pub keeper_y: f64,
    pub keeper_target: f64,
    pub dive: Option<Dive>,
    pub dived: bool,
    pub last_goal: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KickEvent {
    Goal { shooter: u8 },
    Saved { shooter: u8 },
    Over,
}

impl Shootout {
    #[must_use]
    pub fn new(params: Params, bodies: [Body; 2], seed: u64) -> Self {
        let run_up = params.run_up;
        Self {
            params,
            bodies,
            seed,
            tick: 0,
            kicks: 0,
            score: [0, 0],
            phase: Phase::RunUp,
            t: 0,
            dist: run_up,
            lean: 0.0,
            lean_target: 0.0,
            running: true,
            aim: None,
            ball_y: 0.0,
            keeper_y: 0.0,
            keeper_target: 0.0,
            dive: None,
            dived: false,
            last_goal: None,
        }
    }

    #[must_use]
    pub fn shooter(&self) -> u8 {
        (self.kicks % 2) as u8
    }

    #[must_use]
    pub fn keeper(&self) -> u8 {
        1 - self.shooter()
    }

    #[must_use]
    pub fn over(&self) -> bool {
        self.phase == Phase::Over
    }

    #[must_use]
    pub fn limit(&self) -> f64 {
        self.params.half_goal + self.params.wide
    }

    pub fn steer(&mut self, lean: f64, run: bool) {
        self.lean_target = if lean.is_finite() {
            lean.clamp(-1.0, 1.0)
        } else {
            self.lean_target
        };
        self.running = run;
    }

    pub fn kick(&mut self, aim: f64) -> bool {
        let ready = matches!(self.phase, Phase::AtBall)
            || (self.phase == Phase::RunUp && self.dist <= self.params.kick_range);
        if !ready || !aim.is_finite() {
            return false;
        }
        let limit = self.limit();
        self.aim = Some(aim.clamp(-limit, limit));
        self.phase = Phase::Strike;
        self.t = 0;
        true
    }

    pub fn guard(&mut self, target: f64) {
        if target.is_finite() {
            let h = self.params.half_goal;
            self.keeper_target = target.clamp(-h, h);
        }
    }

    pub fn dive_to(&mut self, target: f64) -> bool {
        let live = matches!(
            self.phase,
            Phase::RunUp | Phase::AtBall | Phase::Strike | Phase::Flight
        );
        if self.dived || !live || !target.is_finite() {
            return false;
        }
        let h = self.params.half_goal;
        self.dive = Some(Dive {
            target: target.clamp(-h, h),
            left: self.params.dive_ticks,
        });
        self.dived = true;
        true
    }

    fn move_keeper(&mut self) {
        let dt = self.params.dt();
        let body = self.bodies[usize::from(self.keeper())];
        let (target, speed) = match self.dive {
            Some(d) if d.left > 0 => (d.target, body.dive_speed),
            Some(_) => (self.keeper_y, 0.0),
            None => (self.keeper_target, body.dive_speed * self.params.walk_share),
        };
        let step = speed * dt;
        let gap = target - self.keeper_y;
        self.keeper_y = if gap.abs() <= step {
            target
        } else {
            self.keeper_y + step * gap.signum()
        };
        if let Some(d) = self.dive.as_mut() {
            d.left = d.left.saturating_sub(1);
        }
    }

    fn lean_towards(&mut self) {
        let body = self.bodies[usize::from(self.shooter())];
        let step = body.lean_rate * self.params.dt();
        let gap = self.lean_target - self.lean;
        self.lean = if gap.abs() <= step {
            self.lean_target
        } else {
            self.lean + step * gap.signum()
        };
    }

    fn decided(&self) -> bool {
        let taken = [self.kicks.div_ceil(2), self.kicks / 2];
        let each = self.params.kicks_each;
        let [a, b] = self.score;
        if self.kicks <= 2 * each {
            let left = [each - taken[0].min(each), each - taken[1].min(each)];
            if a > b + left[1] || b > a + left[0] {
                return true;
            }
        }
        if self.kicks >= 2 * each && self.kicks.is_multiple_of(2) && a != b {
            return true;
        }
        self.kicks >= self.params.max_kicks
    }

    fn next_kick(&mut self) {
        self.kicks += 1;
        if self.decided() {
            self.phase = Phase::Over;
            return;
        }
        self.phase = Phase::RunUp;
        self.t = 0;
        self.dist = self.params.run_up;
        self.lean = 0.0;
        self.lean_target = 0.0;
        self.running = true;
        self.aim = None;
        self.ball_y = 0.0;
        self.keeper_y = 0.0;
        self.keeper_target = 0.0;
        self.dive = None;
        self.dived = false;
    }

    pub fn step(&mut self, events: &mut Vec<KickEvent>) {
        if self.over() {
            return;
        }
        self.tick = self.tick.saturating_add(1);
        self.t = self.t.saturating_add(1);
        match self.phase {
            Phase::RunUp => {
                self.lean_towards();
                if self.running {
                    self.dist -= self.params.run_speed * self.params.dt();
                }
                if self.dist <= 0.0 {
                    self.dist = 0.0;
                    self.phase = Phase::AtBall;
                    self.t = 0;
                }
                self.move_keeper();
            }
            Phase::AtBall => {
                self.lean_towards();
                self.move_keeper();
                if self.t >= self.params.ball_wait_ticks {
                    self.kick(0.0);
                }
            }
            Phase::Strike => {
                self.dist = (self.dist - self.params.run_speed * self.params.dt()).max(0.0);
                self.move_keeper();
                if self.t >= self.params.windup_ticks {
                    self.phase = Phase::Flight;
                    self.dist = 0.0;
                    self.t = 0;
                }
            }
            Phase::Flight => {
                self.move_keeper();
                let aim = self.aim.unwrap_or(0.0);
                let f = f64::from(self.params.flight_ticks);
                self.ball_y = aim * (f64::from(self.t.min(self.params.flight_ticks)) / f);
                if self.t >= self.params.flight_ticks {
                    let inside = aim.abs() < self.params.half_goal;
                    let reached = (self.keeper_y - aim).abs() <= self.params.keeper_reach;
                    let goal = inside && !reached;
                    let shooter = self.shooter();
                    if goal {
                        self.score[usize::from(shooter)] += 1;
                        events.push(KickEvent::Goal { shooter });
                    } else {
                        events.push(KickEvent::Saved { shooter });
                    }
                    self.last_goal = Some(goal);
                    self.phase = Phase::Settle;
                    self.t = 0;
                }
            }
            Phase::Settle => {
                if self.t >= self.params.settle_ticks {
                    self.next_kick();
                    if self.over() {
                        events.push(KickEvent::Over);
                    }
                }
            }
            Phase::Over => {}
        }
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        let words = [
            self.tick,
            u64::from(self.kicks),
            u64::from(self.score[0]),
            u64::from(self.score[1]),
            self.phase.code().to_bits(),
            u64::from(self.t),
            self.dist.to_bits(),
            self.lean.to_bits(),
            self.lean_target.to_bits(),
            u64::from(self.running),
            self.aim.map_or(u64::MAX, f64::to_bits),
            self.ball_y.to_bits(),
            self.keeper_y.to_bits(),
            self.keeper_target.to_bits(),
            self.dive.map_or(u64::MAX, |d| d.target.to_bits()),
            self.dive.map_or(u64::MAX, |d| u64::from(d.left)),
            u64::from(self.dived),
        ];
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for word in words {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        hash
    }
}
