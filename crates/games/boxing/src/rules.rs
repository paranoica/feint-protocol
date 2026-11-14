#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub ring: f64,
    pub start_gap: f64,
    pub min_gap: f64,
    pub jab_windup: u32,
    pub hook_windup: u32,
    pub active: u32,
    pub jab_recover: u32,
    pub hook_recover: u32,
    pub blocked_recover: u32,
    pub cancel_recover: u32,
    pub guard_switch: u32,
    pub break_ticks: u32,
    pub jab_reach: f64,
    pub hook_reach: f64,
    pub jab_points: u32,
    pub hook_points: u32,
    pub points_to_win: u32,
    pub round_ticks: u32,
    pub rounds: u32,
    pub tick_hz: u32,
    pub decision_hz: u32,
    pub max_speed: f64,
    pub min_hand: f64,
    pub max_hand: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ring: 3.0,
            start_gap: 1.6,
            min_gap: 0.4,
            jab_windup: 100,
            hook_windup: 180,
            active: 12,
            jab_recover: 36,
            hook_recover: 72,
            blocked_recover: 96,
            cancel_recover: 36,
            guard_switch: 24,
            break_ticks: 120,
            jab_reach: 0.85,
            hook_reach: 0.7,
            jab_points: 1,
            hook_points: 2,
            points_to_win: 30,
            round_ticks: 240 * 30,
            rounds: 3,
            tick_hz: 240,
            decision_hz: 60,
            max_speed: 1.5,
            min_hand: 0.5,
            max_hand: 1.0,
        }
    }
}

impl Params {
    #[must_use]
    pub fn dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz)
    }

    #[must_use]
    pub fn match_ticks(&self) -> u64 {
        u64::from(self.round_ticks) * u64::from(self.rounds)
    }

    #[must_use]
    pub fn reach(&self, kind: Kind) -> f64 {
        match kind {
            Kind::Jab => self.jab_reach,
            Kind::Hook => self.hook_reach,
        }
    }

    #[must_use]
    pub fn points(&self, kind: Kind) -> u32 {
        match kind {
            Kind::Jab => self.jab_points,
            Kind::Hook => self.hook_points,
        }
    }

    #[must_use]
    pub fn recover(&self, kind: Kind) -> u32 {
        match kind {
            Kind::Jab => self.jab_recover,
            Kind::Hook => self.hook_recover,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let positive = [
            self.ring,
            self.start_gap,
            self.min_gap,
            self.jab_reach,
            self.hook_reach,
            self.max_speed,
            self.min_hand,
            self.max_hand,
        ];
        if !positive.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("sizes and speeds must be positive and finite");
        }
        if self.min_gap >= self.start_gap || self.start_gap >= 2.0 * self.ring {
            return Err("the fighters start apart inside the ring");
        }
        if self.min_hand > self.max_hand {
            return Err("the hand range is empty");
        }
        if self.jab_windup == 0 || self.hook_windup <= self.jab_windup || self.active == 0 {
            return Err("a hook winds up longer than a jab");
        }
        if self.jab_recover == 0
            || self.hook_recover == 0
            || self.blocked_recover == 0
            || self.cancel_recover == 0
            || self.break_ticks == 0
        {
            return Err("recoveries and breaks take time");
        }
        let timers = [
            self.jab_windup,
            self.hook_windup,
            self.active,
            self.jab_recover,
            self.hook_recover,
            self.blocked_recover,
            self.cancel_recover,
            self.guard_switch,
            self.break_ticks,
        ];
        if timers.iter().any(|&t| t > 10 * self.tick_hz) {
            return Err("a move lasts at most ten seconds");
        }
        if self.jab_points == 0
            || self.hook_points == 0
            || self.jab_points.max(self.hook_points) > self.points_to_win
        {
            return Err("a clean hit scores at least one point and at most the match");
        }
        if self.points_to_win == 0 || self.round_ticks == 0 || self.rounds == 0 {
            return Err("a match needs points and time");
        }
        if self.points_to_win > 1000 || self.match_ticks() > 240 * 3600 {
            return Err("a match ends within an hour and a thousand points");
        }
        if self.tick_hz == 0 || self.decision_hz == 0 || self.decision_hz > self.tick_hz {
            return Err("ticks and decisions need rates");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub speed: f64,
    pub hand: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jab,
    Hook,
}

impl Kind {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Jab => 1.0,
            Self::Hook => 2.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Option<Self> {
        if code >= 1.5 {
            Some(Self::Hook)
        } else if code >= 0.5 {
            Some(Self::Jab)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Height {
    Low,
    High,
}

impl Height {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Low => -1.0,
            Self::High => 1.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Self {
        if code < 0.0 { Self::Low } else { Self::High }
    }

    #[must_use]
    pub fn class(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }

    #[must_use]
    pub fn of_class(class: u8) -> Self {
        if class == 0 { Self::Low } else { Self::High }
    }

    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High => Self::Low,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    WindUp {
        kind: Kind,
        height: Height,
        left: u32,
    },
    Strike {
        kind: Kind,
        height: Height,
        left: u32,
        blocked: bool,
    },
    Recover {
        left: u32,
    },
}

pub const STAGE_LEN: usize = 5;

impl Stage {
    #[must_use]
    pub fn code(self) -> [f64; STAGE_LEN] {
        match self {
            Self::Idle => [0.0; STAGE_LEN],
            Self::WindUp { kind, height, left } => {
                [1.0, kind.code(), height.code(), f64::from(left), 0.0]
            }
            Self::Strike {
                kind,
                height,
                left,
                blocked,
            } => [
                2.0,
                kind.code(),
                height.code(),
                f64::from(left),
                f64::from(u8::from(blocked)),
            ],
            Self::Recover { left } => [3.0, 0.0, 0.0, f64::from(left), 0.0],
        }
    }

    #[must_use]
    pub fn from_code(v: [f64; STAGE_LEN], cap: u32) -> Self {
        let left = (v[3].round().max(1.0).min(f64::from(cap.max(1)))) as u32;
        let kind = Kind::from_code(v[1]).unwrap_or(Kind::Jab);
        let height = Height::from_code(v[2]);
        match v[0].round() as i64 {
            1 => Self::WindUp { kind, height, left },
            2 => Self::Strike {
                kind,
                height,
                left,
                blocked: v[4] >= 0.5,
            },
            3 => Self::Recover { left },
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fighter {
    pub x: f64,
    pub target: f64,
    pub guard: Height,
    pub switching: u32,
    pub stage: Stage,
    pub queued: Option<(Kind, Height)>,
    pub cancel: bool,
    pub strikes: u32,
    pub last: Option<Height>,
    pub feints: u32,
}

impl Fighter {
    fn at(x: f64) -> Self {
        Self {
            x,
            target: x,
            guard: Height::High,
            switching: 0,
            stage: Stage::Idle,
            queued: None,
            cancel: false,
            strikes: 0,
            last: None,
            feints: 0,
        }
    }

    #[must_use]
    pub fn covering(&self) -> Option<Height> {
        (matches!(self.stage, Stage::Idle | Stage::WindUp { .. }) && self.switching == 0)
            .then_some(self.guard)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Fight,
    Break,
    Over,
}

impl Phase {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Fight => 0.0,
            Self::Break => 1.0,
            Self::Over => 2.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Self {
        match code.round() as i64 {
            1 => Self::Break,
            2 => Self::Over,
            _ => Self::Fight,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Hit { by: u8, points: u32 },
    Blocked { by: u8 },
    Whiff { by: u8 },
    Feint { by: u8 },
    RoundEnd,
    Over,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bout {
    pub params: Params,
    pub bodies: [Body; 2],
    pub seed: u64,
    pub tick: u64,
    pub phase: Phase,
    pub phase_left: u32,
    pub score: [u32; 2],
    pub fighters: [Fighter; 2],
    pub landed: [u32; 2],
    pub blocked: [u32; 2],
}

#[must_use]
pub fn facing(seat: u8) -> f64 {
    if seat == 0 { 1.0 } else { -1.0 }
}

impl Bout {
    #[must_use]
    pub fn new(params: Params, bodies: [Body; 2], seed: u64) -> Self {
        let half = params.start_gap / 2.0;
        Self {
            fighters: [Fighter::at(-half), Fighter::at(half)],
            params,
            bodies,
            seed,
            tick: 0,
            phase: Phase::Fight,
            phase_left: 0,
            score: [0, 0],
            landed: [0, 0],
            blocked: [0, 0],
        }
    }

    #[must_use]
    pub fn over(&self) -> bool {
        self.phase == Phase::Over
    }

    #[must_use]
    pub fn gap(&self) -> f64 {
        self.fighters[1].x - self.fighters[0].x
    }

    #[must_use]
    pub fn windup(&self, seat: u8, kind: Kind) -> u32 {
        let base = match kind {
            Kind::Jab => self.params.jab_windup,
            Kind::Hook => self.params.hook_windup,
        };
        let hand = self.bodies[usize::from(seat.min(1))].hand;
        ((f64::from(base) / hand).round() as u32).max(1)
    }

    pub fn order(
        &mut self,
        seat: u8,
        target: f64,
        guard: Height,
        attack: Option<(Kind, Height)>,
        cancel: bool,
    ) {
        let ring = self.params.ring;
        let switch = self.params.guard_switch;
        let f = &mut self.fighters[usize::from(seat.min(1))];
        f.target = target.clamp(-ring, ring);
        if guard != f.guard {
            f.guard = guard;
            f.switching = switch;
        }
        f.queued = attack;
        f.cancel = cancel;
    }

    fn part(&mut self) {
        let half = self.params.start_gap / 2.0;
        for (f, x) in self.fighters.iter_mut().zip([-half, half]) {
            f.x = x;
            f.target = x;
            f.switching = 0;
            f.stage = Stage::Idle;
            f.queued = None;
            f.cancel = false;
        }
    }

    fn rest(&mut self) {
        self.phase = Phase::Break;
        self.phase_left = self.params.break_ticks;
        self.part();
    }

    fn walk(&mut self) {
        let p = &self.params;
        let dt = p.dt();
        let mut xs = [0.0; 2];
        for (seat, f) in self.fighters.iter().enumerate() {
            let free = !matches!(f.stage, Stage::Strike { .. });
            let step = self.bodies[seat].speed * dt;
            xs[seat] = if free {
                f.x + (f.target - f.x).clamp(-step, step)
            } else {
                f.x
            };
        }
        if xs[1] - xs[0] < p.min_gap {
            let mid = (xs[0] + xs[1]) / 2.0;
            xs = [mid - p.min_gap / 2.0, mid + p.min_gap / 2.0];
        }
        if xs[0] < -p.ring {
            xs = [-p.ring, -p.ring + p.min_gap];
        }
        if xs[1] > p.ring {
            xs = [p.ring - p.min_gap, p.ring];
        }
        self.fighters[0].x = xs[0];
        self.fighters[1].x = xs[1];
    }

    fn fight(&mut self, events: &mut Vec<Event>) {
        for f in &mut self.fighters {
            f.switching = f.switching.saturating_sub(1);
        }
        self.walk();
        let covers = [self.fighters[0].covering(), self.fighters[1].covering()];
        let gap = self.gap();
        let windups = [
            [self.windup(0, Kind::Jab), self.windup(0, Kind::Hook)],
            [self.windup(1, Kind::Jab), self.windup(1, Kind::Hook)],
        ];
        let p = self.params.clone();
        let mut hit = false;
        for seat in 0..2u8 {
            let i = usize::from(seat);
            let f = &mut self.fighters[i];
            let cancel = std::mem::take(&mut f.cancel);
            f.stage = match f.stage {
                Stage::Idle => match f.queued.take() {
                    Some((kind, height)) => {
                        let left = windups[i][usize::from(kind == Kind::Hook)];
                        Stage::WindUp { kind, height, left }
                    }
                    None => Stage::Idle,
                },
                Stage::WindUp { .. } if cancel => {
                    f.feints += 1;
                    events.push(Event::Feint { by: seat });
                    Stage::Recover {
                        left: p.cancel_recover,
                    }
                }
                Stage::WindUp { kind, height, left } if left <= 1 => {
                    f.strikes += 1;
                    f.last = Some(height);
                    let reached = gap <= p.reach(kind);
                    let blocked = reached && covers[1 - i] == Some(height);
                    if !reached {
                        events.push(Event::Whiff { by: seat });
                    } else if blocked {
                        self.blocked[i] += 1;
                        events.push(Event::Blocked { by: seat });
                    } else {
                        let points = p.points(kind);
                        self.score[i] += points;
                        self.landed[i] += 1;
                        events.push(Event::Hit { by: seat, points });
                        hit = true;
                    }
                    Stage::Strike {
                        kind,
                        height,
                        left: p.active,
                        blocked,
                    }
                }
                Stage::WindUp { kind, height, left } => Stage::WindUp {
                    kind,
                    height,
                    left: left - 1,
                },
                Stage::Strike {
                    kind,
                    blocked,
                    left,
                    ..
                } if left <= 1 => Stage::Recover {
                    left: if blocked {
                        p.blocked_recover
                    } else {
                        p.recover(kind)
                    },
                },
                Stage::Strike {
                    kind,
                    height,
                    left,
                    blocked,
                } => Stage::Strike {
                    kind,
                    height,
                    left: left - 1,
                    blocked,
                },
                Stage::Recover { left } if left <= 1 => Stage::Idle,
                Stage::Recover { left } => Stage::Recover { left: left - 1 },
            };
        }
        if hit {
            self.rest();
        }
    }

    pub fn step(&mut self, events: &mut Vec<Event>) {
        if self.over() {
            return;
        }
        self.tick += 1;
        match self.phase {
            Phase::Fight => self.fight(events),
            Phase::Break => {
                for f in &mut self.fighters {
                    f.switching = f.switching.saturating_sub(1);
                }
                self.phase_left = self.phase_left.saturating_sub(1);
                if self.phase_left == 0 {
                    self.phase = Phase::Fight;
                }
            }
            Phase::Over => {}
        }
        let p = &self.params;
        if self.score[0].max(self.score[1]) >= p.points_to_win || self.tick >= p.match_ticks() {
            self.phase = Phase::Over;
            self.phase_left = 0;
            events.push(Event::Over);
        } else if self.tick.is_multiple_of(u64::from(p.round_ticks)) {
            self.rest();
            events.push(Event::RoundEnd);
        }
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        let mut words = vec![
            self.tick,
            self.phase.code().to_bits(),
            u64::from(self.phase_left),
            u64::from(self.score[0]),
            u64::from(self.score[1]),
            u64::from(self.landed[0]),
            u64::from(self.landed[1]),
            u64::from(self.blocked[0]),
            u64::from(self.blocked[1]),
        ];
        for f in &self.fighters {
            words.extend([
                f.x.to_bits(),
                f.target.to_bits(),
                f.guard.code().to_bits(),
                u64::from(f.switching),
                u64::from(f.cancel),
                u64::from(f.strikes),
                f.last.map_or(0, |h| h.code().to_bits()),
                u64::from(f.feints),
            ]);
            words.extend(f.stage.code().map(f64::to_bits));
            words.push(f.queued.map_or(0, |(k, h)| {
                1 + 2 * u64::from(k == Kind::Hook) + u64::from(h == Height::High)
            }));
        }
        words.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, w| {
            w.to_le_bytes()
                .iter()
                .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
        })
    }
}
