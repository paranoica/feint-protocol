#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub half_width: f64,
    pub hit_radius: f64,
    pub roll_dist: f64,
    pub reload_ticks: u32,
    pub commit_at: f64,
    pub flight_ticks: u32,
    pub ready_ticks: u32,
    pub settle_ticks: u32,
    pub round_ticks: u32,
    pub wins: u32,
    pub max_rounds: u32,
    pub tick_hz: u32,
    pub decision_hz: u32,
    pub max_draw_rate: f64,
    pub max_step_rate: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            half_width: 2.5,
            hit_radius: 0.3,
            roll_dist: 1.0,
            reload_ticks: 240,
            commit_at: 0.7,
            flight_ticks: 44,
            ready_ticks: 240,
            settle_ticks: 240,
            round_ticks: 240 * 15,
            wins: 5,
            max_rounds: 30,
            tick_hz: 240,
            decision_hz: 60,
            max_draw_rate: 2.86,
            max_step_rate: 1.25,
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
            self.half_width,
            self.hit_radius,
            self.roll_dist,
            self.max_draw_rate,
            self.max_step_rate,
        ];
        if !positive.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("sizes and speeds must be positive and finite");
        }
        if !(self.commit_at > 0.0 && self.commit_at < 1.0) {
            return Err("the commit point lies inside the draw");
        }
        if self.roll_dist >= 2.0 * self.half_width {
            return Err("a roll must fit the lane");
        }
        if self.flight_ticks == 0 || self.round_ticks == 0 || self.tick_hz == 0 {
            return Err("phases need ticks");
        }
        if self.reload_ticks < self.flight_ticks {
            return Err("a gun reloads only after its bullet lands");
        }
        if self.decision_hz == 0 || self.decision_hz > self.tick_hz {
            return Err("decisions must be positive and no faster than ticks");
        }
        if self.wins == 0 || self.max_rounds < 2 * self.wins - 1 {
            return Err("a match needs rounds to be won");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub draw_rate: f64,
    pub step_rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Live,
    Settle,
    Over,
}

impl Phase {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Ready => 0.0,
            Self::Live => 1.0,
            Self::Settle => 2.0,
            Self::Over => 3.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Self {
        match code.round() as i64 {
            1 => Self::Live,
            2 => Self::Settle,
            3 => Self::Over,
            _ => Self::Ready,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roll {
    pub dir: i8,
    pub left: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gunner {
    pub x: f64,
    pub hand: f64,
    pub holding: bool,
    pub committed: bool,
    pub aim_target: f64,
    pub aim: Option<f64>,
    pub lead: f64,
    pub loaded: bool,
    pub roll: Option<Roll>,
    pub recovery: u32,
    pub rolls: u32,
    pub last_roll: i8,
    pub reload: u32,
    pub shots: u32,
    pub response: i8,
    pub last_response: i8,
}

impl Gunner {
    fn fresh() -> Self {
        Self {
            x: 0.0,
            hand: 0.0,
            holding: false,
            committed: false,
            aim_target: 0.0,
            aim: None,
            lead: 0.0,
            loaded: true,
            roll: None,
            recovery: 0,
            rolls: 0,
            last_roll: 0,
            reload: 0,
            shots: 0,
            response: 0,
            last_response: 0,
        }
    }

    #[must_use]
    pub fn can_roll(&self) -> bool {
        self.roll.is_none() && self.recovery == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bullet {
    pub x: f64,
    pub lead: f64,
    pub t: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundEvent {
    Hit { shooter: u8 },
    Void,
    Over,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Duel {
    pub params: Params,
    pub bodies: [Body; 2],
    pub seed: u64,
    pub tick: u64,
    pub round: u32,
    pub score: [u32; 2],
    pub phase: Phase,
    pub t: u32,
    pub gunners: [Gunner; 2],
    pub bullets: [Option<Bullet>; 2],
    pub last: Option<RoundEvent>,
}

impl Duel {
    #[must_use]
    pub fn new(params: Params, bodies: [Body; 2], seed: u64) -> Self {
        Self {
            params,
            bodies,
            seed,
            tick: 0,
            round: 0,
            score: [0, 0],
            phase: Phase::Ready,
            t: 0,
            gunners: [Gunner::fresh(); 2],
            bullets: [None; 2],
            last: None,
        }
    }

    #[must_use]
    pub fn over(&self) -> bool {
        self.phase == Phase::Over
    }

    pub fn steer(&mut self, seat: u8, aim: f64, hold: bool) {
        let w = self.params.half_width;
        let g = &mut self.gunners[usize::from(seat.min(1))];
        if aim.is_finite() {
            g.aim_target = aim.clamp(-w, w);
        }
        g.holding = hold;
    }

    pub fn roll(&mut self, seat: u8, dir: i8) -> bool {
        let g = &mut self.gunners[usize::from(seat.min(1))];
        if self.phase != Phase::Live || dir == 0 || !g.can_roll() {
            return false;
        }
        g.roll = Some(Roll {
            dir: dir.signum(),
            left: 1,
        });
        g.rolls += 1;
        g.last_roll = dir.signum();
        let shooter = usize::from(1 - seat.min(1));
        let late = self.gunners[shooter].committed || self.bullets[shooter].is_some();
        let drawing = self.gunners[shooter].loaded && self.gunners[shooter].hand > 0.0;
        if late {
            self.gunners[shooter].response = 2 * dir.signum();
        } else if drawing {
            self.gunners[shooter].response = dir.signum();
        }
        true
    }

    #[must_use]
    pub fn recovery_ticks(&self, seat: u8) -> u32 {
        let body = self.bodies[usize::from(seat.min(1))];
        (f64::from(self.params.tick_hz) / body.step_rate)
            .round()
            .max(1.0) as u32
    }

    fn move_gunner(&mut self, seat: usize) {
        let w = self.params.half_width;
        let recovery = self.recovery_ticks(seat as u8);
        let g = &mut self.gunners[seat];
        if let Some(roll) = g.roll.take() {
            g.x = (g.x + f64::from(roll.dir) * self.params.roll_dist).clamp(-w, w);
            g.recovery = recovery;
        } else {
            g.recovery = g.recovery.saturating_sub(1);
        }
        if !g.loaded {
            g.reload = g.reload.saturating_sub(1);
            if g.reload == 0 && self.bullets[seat].is_none() {
                g.loaded = true;
                g.response = 0;
            }
        }
    }

    fn move_hand(&mut self, seat: usize) {
        let step = self.bodies[seat].draw_rate * self.params.dt();
        let target_x = self.gunners[1 - seat].x;
        let commit_at = self.params.commit_at;
        let w = self.params.half_width;
        let g = &mut self.gunners[seat];
        if g.committed {
            g.hand = (g.hand + step).min(1.0);
            if g.hand >= 1.0 {
                let x = g.aim.unwrap_or(target_x);
                self.bullets[seat] = Some(Bullet {
                    x,
                    lead: g.lead,
                    t: 0,
                });
                g.committed = false;
                g.loaded = false;
                g.aim = None;
                g.reload = self.params.reload_ticks;
            }
        } else if g.holding {
            g.hand = if g.loaded {
                (g.hand + step).min(1.0)
            } else if g.hand > commit_at {
                (g.hand - step).max(commit_at)
            } else {
                (g.hand + step).min(commit_at)
            };
            if g.loaded && g.hand >= commit_at {
                let aim = g.aim_target.clamp(-w, w);
                g.committed = true;
                g.aim = Some(aim);
                g.lead = aim - target_x;
            }
        } else {
            g.hand = (g.hand - step).max(0.0);
        }
    }

    fn fly(&mut self) -> [bool; 2] {
        let flight = self.params.flight_ticks;
        let radius = self.params.hit_radius;
        let targets = [self.gunners[1].x, self.gunners[0].x];
        let mut hits = [false; 2];
        for (seat, hit) in hits.iter_mut().enumerate() {
            let Some(b) = self.bullets[seat].as_mut() else {
                continue;
            };
            b.t += 1;
            if b.t < flight {
                continue;
            }
            *hit = (targets[seat] - b.x).abs() < radius;
            self.bullets[seat] = None;
            let g = &mut self.gunners[seat];
            g.shots += 1;
            g.last_response = g.response;
            g.response = 0;
        }
        hits
    }

    fn end_round(&mut self, event: RoundEvent, events: &mut Vec<RoundEvent>) {
        if let RoundEvent::Hit { shooter } = event {
            self.score[usize::from(shooter)] += 1;
        }
        events.push(event);
        self.last = Some(event);
        self.phase = Phase::Settle;
        self.t = 0;
    }

    fn next_round(&mut self, events: &mut Vec<RoundEvent>) {
        self.round += 1;
        let won = self.score.iter().any(|&s| s >= self.params.wins);
        if won || self.round >= self.params.max_rounds {
            self.phase = Phase::Over;
            events.push(RoundEvent::Over);
            return;
        }
        let aims = self.gunners.map(|g| (g.aim_target, g.holding));
        self.gunners = [Gunner::fresh(); 2];
        for (g, (aim, hold)) in self.gunners.iter_mut().zip(aims) {
            g.aim_target = aim;
            g.holding = hold;
        }
        self.bullets = [None; 2];
        self.phase = Phase::Ready;
        self.t = 0;
    }

    pub fn step(&mut self, events: &mut Vec<RoundEvent>) {
        if self.over() {
            return;
        }
        self.tick = self.tick.saturating_add(1);
        self.t = self.t.saturating_add(1);
        match self.phase {
            Phase::Ready => {
                if self.t >= self.params.ready_ticks {
                    self.phase = Phase::Live;
                    self.t = 0;
                }
            }
            Phase::Live => {
                self.move_gunner(0);
                self.move_gunner(1);
                self.move_hand(0);
                self.move_hand(1);
                let hits = self.fly();
                match hits {
                    [true, true] => self.end_round(RoundEvent::Void, events),
                    [true, false] => self.end_round(RoundEvent::Hit { shooter: 0 }, events),
                    [false, true] => self.end_round(RoundEvent::Hit { shooter: 1 }, events),
                    [false, false] => {
                        if self.t >= self.params.round_ticks {
                            self.end_round(RoundEvent::Void, events);
                        }
                    }
                }
            }
            Phase::Settle => {
                if self.t >= self.params.settle_ticks {
                    self.next_round(events);
                }
            }
            Phase::Over => {}
        }
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        let mut words = vec![
            self.tick,
            u64::from(self.round),
            u64::from(self.score[0]),
            u64::from(self.score[1]),
            self.phase.code().to_bits(),
            u64::from(self.t),
        ];
        for g in &self.gunners {
            words.extend([
                g.x.to_bits(),
                g.hand.to_bits(),
                u64::from(g.holding),
                u64::from(g.committed),
                g.aim_target.to_bits(),
                g.aim.map_or(u64::MAX, f64::to_bits),
                g.lead.to_bits(),
                u64::from(g.loaded),
                g.roll.map_or(u64::MAX, |r| {
                    u64::from(r.left) << 8 | u64::from(r.dir as u8)
                }),
                u64::from(g.recovery),
                u64::from(g.rolls),
                u64::from(g.reload),
                u64::from(g.shots),
                u64::from(g.response as u8),
                u64::from(g.last_response as u8),
            ]);
        }
        for b in &self.bullets {
            words.extend(b.map_or([u64::MAX; 3], |b| {
                [b.x.to_bits(), b.lead.to_bits(), u64::from(b.t)]
            }));
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for word in words {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        hash
    }
}
