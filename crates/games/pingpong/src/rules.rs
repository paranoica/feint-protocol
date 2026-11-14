#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub half_length: f64,
    pub half_width: f64,
    pub net_height: f64,
    pub gravity: f64,
    pub bounce: f64,
    pub reach: f64,
    pub hit_top: f64,
    pub back_room: f64,
    pub side_room: f64,
    pub over_table: f64,
    pub min_depth: f64,
    pub out_room: f64,
    pub max_returns: u32,
    pub min_flight: f64,
    pub max_flight: f64,
    pub disguise_flight: f64,
    pub serve_height: f64,
    pub serve_ticks: u32,
    pub settle_ticks: u32,
    pub points_to_win: u32,
    pub win_by: u32,
    pub max_points: u32,
    pub serves_each: u32,
    pub tick_hz: u32,
    pub decision_hz: u32,
    pub max_speed: f64,
    pub max_accel: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            half_length: 1.37,
            half_width: 0.7625,
            net_height: 0.1525,
            gravity: 9.81,
            bounce: 0.85,
            reach: 0.32,
            hit_top: 0.6,
            back_room: 2.0,
            side_room: 1.0,
            over_table: 0.3,
            min_depth: 0.2,
            out_room: 0.5,
            max_returns: 13,
            min_flight: 0.35,
            max_flight: 1.0,
            disguise_flight: 0.12,
            serve_height: 0.25,
            serve_ticks: 240 * 3,
            settle_ticks: 240,
            points_to_win: 11,
            win_by: 2,
            max_points: 40,
            serves_each: 2,
            tick_hz: 240,
            decision_hz: 60,
            max_speed: 4.0,
            max_accel: 16.0,
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
            self.half_length,
            self.half_width,
            self.net_height,
            self.gravity,
            self.reach,
            self.hit_top,
            self.back_room,
            self.side_room,
            self.min_flight,
            self.max_flight,
            self.serve_height,
            self.max_speed,
            self.max_accel,
        ];
        if !positive.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("sizes, times and speeds must be positive and finite");
        }
        if !(self.bounce > 0.0 && self.bounce < 1.0) {
            return Err("the bounce keeps part of the vertical speed");
        }
        if !(self.out_room >= 0.0 && self.out_room.is_finite()) || self.max_returns == 0 {
            return Err("aims may miss the table by a bounded room and rallies end");
        }
        if !(self.min_depth > 0.0 && self.min_depth < self.half_length) {
            return Err("a shot lands past the net and on the table");
        }
        if !(self.over_table >= 0.0 && self.over_table < self.half_length) {
            return Err("a player reaches over its own half only");
        }
        if self.min_flight >= self.max_flight
            || !self.disguise_flight.is_finite()
            || self.disguise_flight < 0.0
        {
            return Err("flight times must form a range");
        }
        if self.points_to_win == 0 || self.win_by == 0 || self.serves_each == 0 {
            return Err("a match needs points, a margin and serves");
        }
        if self.max_points < 2 * self.points_to_win {
            return Err("the point cap must allow a full game");
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
    pub accel: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub pos: [f64; 3],
    pub vel: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paddle {
    pub pos: [f64; 2],
    pub vel: [f64; 2],
    pub target: [f64; 2],
    pub facing: f64,
    pub swing: Option<Swing>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    pub aim: [f64; 2],
    pub flight: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Serve,
    Rally,
    Settle,
    Over,
}

impl Phase {
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Serve => 0.0,
            Self::Rally => 1.0,
            Self::Settle => 2.0,
            Self::Over => 3.0,
        }
    }

    #[must_use]
    pub fn from_code(code: f64) -> Self {
        match code.round() as i64 {
            1 => Self::Rally,
            2 => Self::Settle,
            3 => Self::Over,
            _ => Self::Serve,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointEvent {
    Point { winner: u8 },
    Over,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub params: Params,
    pub bodies: [Body; 2],
    pub seed: u64,
    pub tick: u64,
    pub score: [u32; 2],
    pub server: u8,
    pub phase: Phase,
    pub t: u32,
    pub ball: Ball,
    pub paddles: [Paddle; 2],
    pub hitter: Option<u8>,
    pub bounces: u8,
    pub hits: u32,
    pub rally: u32,
    pub facing_at_hit: f64,
}

fn side_x(seat: u8) -> f64 {
    if seat == 0 { -1.0 } else { 1.0 }
}

impl Table {
    #[must_use]
    pub fn new(params: Params, bodies: [Body; 2], seed: u64) -> Self {
        let ready = |seat: u8| {
            let x = side_x(seat) * (params.half_length + 0.4);
            Paddle {
                pos: [x, 0.0],
                vel: [0.0, 0.0],
                target: [x, 0.0],
                facing: 0.0,
                swing: None,
            }
        };
        let server = (seed % 2) as u8;
        let paddles = [ready(0), ready(1)];
        let mut t = Self {
            ball: Ball {
                pos: [0.0, 0.0, 0.0],
                vel: [0.0; 3],
            },
            params,
            bodies,
            seed,
            tick: 0,
            score: [0, 0],
            server,
            phase: Phase::Serve,
            t: 0,
            paddles,
            hitter: None,
            bounces: 0,
            hits: 0,
            rally: 0,
            facing_at_hit: 0.0,
        };
        t.hold_ball();
        t
    }

    fn hold_ball(&mut self) {
        let p = self.paddles[usize::from(self.server)].pos;
        self.ball = Ball {
            pos: [p[0], p[1], self.params.serve_height],
            vel: [0.0; 3],
        };
    }

    #[must_use]
    pub fn over(&self) -> bool {
        self.phase == Phase::Over
    }

    #[must_use]
    pub fn to_hit(&self) -> Option<u8> {
        match (self.phase, self.hitter) {
            (Phase::Serve, _) => Some(self.server),
            (Phase::Rally, Some(h)) if self.bounces == 1 => Some(1 - h),
            _ => None,
        }
    }

    pub fn steer(&mut self, seat: u8, target: [f64; 2], swing: Option<Swing>, facing: f64) {
        let p = &self.params;
        let s = side_x(seat);
        let near = p.half_length - p.over_table;
        let far = p.half_length + p.back_room;
        let (lo, hi) = if s < 0.0 { (-far, -near) } else { (near, far) };
        let w = p.half_width + p.side_room;
        let paddle = &mut self.paddles[usize::from(seat)];
        paddle.target = [target[0].clamp(lo, hi), target[1].clamp(-w, w)];
        paddle.swing = swing;
        paddle.facing = facing.clamp(-1.0, 1.0);
    }

    fn move_paddle(&mut self, seat: usize) {
        let dt = self.params.dt();
        let body = self.bodies[seat];
        let pad = &mut self.paddles[seat];
        let to = [pad.target[0] - pad.pos[0], pad.target[1] - pad.pos[1]];
        let dist = (to[0] * to[0] + to[1] * to[1]).sqrt();
        let stop = (2.0 * body.accel * dist).sqrt().min(body.speed);
        let want = if dist > 1e-9 {
            [to[0] / dist * stop, to[1] / dist * stop]
        } else {
            [0.0, 0.0]
        };
        let dv = [want[0] - pad.vel[0], want[1] - pad.vel[1]];
        let dvn = (dv[0] * dv[0] + dv[1] * dv[1]).sqrt();
        let most = body.accel * dt;
        let scale = if dvn > most { most / dvn } else { 1.0 };
        pad.vel = [pad.vel[0] + dv[0] * scale, pad.vel[1] + dv[1] * scale];
        pad.pos = [pad.pos[0] + pad.vel[0] * dt, pad.pos[1] + pad.vel[1] * dt];
    }

    #[must_use]
    pub fn in_reach(&self, seat: u8) -> bool {
        let pad = self.paddles[usize::from(seat)];
        let b = self.ball.pos;
        let d = [b[0] - pad.pos[0], b[1] - pad.pos[1]];
        d[0] * d[0] + d[1] * d[1] <= self.params.reach * self.params.reach
            && b[2] >= 0.0
            && b[2] <= self.params.hit_top
    }

    fn try_hit(&mut self, seat: u8, events: &mut Vec<PointEvent>) -> bool {
        if self.to_hit() != Some(seat) || !self.in_reach(seat) {
            return false;
        }
        if seat != self.server && self.rally >= 2 * self.params.max_returns {
            let receiver = 1 - self.server;
            self.point(receiver, events);
            return true;
        }
        let pad = self.paddles[usize::from(seat)];
        let Some(swing) = pad.swing else {
            return false;
        };
        let p = &self.params;
        let s = side_x(seat);
        let aim = [
            -s * swing.aim[0]
                .abs()
                .clamp(p.min_depth, p.half_length + p.out_room),
            swing.aim[1].clamp(-p.half_width - p.out_room, p.half_width + p.out_room),
        ];
        let toward = if aim[1] > 0.0 { 1.0 } else { -1.0 };
        let disguised = pad.facing * toward < -0.3;
        let floor = p.min_flight + if disguised { p.disguise_flight } else { 0.0 };
        let flight = swing.flight.clamp(floor, p.max_flight);
        let b = self.ball.pos;
        self.ball.vel = [
            (aim[0] - b[0]) / flight,
            (aim[1] - b[1]) / flight,
            (0.5 * p.gravity * flight * flight - b[2]) / flight,
        ];
        self.facing_at_hit = pad.facing;
        self.hitter = Some(seat);
        self.bounces = 0;
        self.hits += 1;
        self.rally += 1;
        self.phase = Phase::Rally;
        self.t = 0;
        true
    }

    fn point(&mut self, winner: u8, events: &mut Vec<PointEvent>) {
        self.score[usize::from(winner)] += 1;
        events.push(PointEvent::Point { winner });
        let total = self.score[0] + self.score[1];
        let p = &self.params;
        let [a, b] = self.score;
        let lead = a.abs_diff(b);
        if (a.max(b) >= p.points_to_win && lead >= p.win_by) || total >= p.max_points {
            self.phase = Phase::Over;
            events.push(PointEvent::Over);
            return;
        }
        if total.is_multiple_of(p.serves_each) {
            self.server = 1 - self.server;
        }
        self.phase = Phase::Settle;
        self.t = 0;
        self.hitter = None;
        self.bounces = 0;
        self.rally = 0;
    }

    fn fly(&mut self, events: &mut Vec<PointEvent>) {
        let dt = self.params.dt();
        let g = self.params.gravity;
        let before = self.ball.pos;
        let v = &mut self.ball.vel;
        v[2] -= g * dt;
        let after = [
            before[0] + v[0] * dt,
            before[1] + v[1] * dt,
            before[2] + v[2] * dt,
        ];
        self.ball.pos = after;
        let Some(hitter) = self.hitter else {
            return;
        };
        let p = self.params.clone();
        if before[0] * after[0] < 0.0 || (before[0] != 0.0 && after[0] == 0.0) {
            let k = before[0] / (before[0] - after[0]);
            let z = before[2] + (after[2] - before[2]) * k;
            let y = before[1] + (after[1] - before[1]) * k;
            if z < p.net_height && z >= 0.0 && y.abs() <= p.half_width + p.net_height {
                self.point(1 - hitter, events);
                return;
            }
        }
        if before[2] > 0.0 && after[2] <= 0.0 {
            let k = before[2] / (before[2] - after[2]);
            let x = before[0] + (after[0] - before[0]) * k;
            let y = before[1] + (after[1] - before[1]) * k;
            let on_table = x.abs() <= p.half_length && y.abs() <= p.half_width;
            let receiver_side = side_x(1 - hitter) * x > 0.0;
            if on_table && receiver_side && self.bounces == 0 {
                self.bounces = 1;
                self.ball.pos = [x, y, 0.0];
                self.ball.vel[2] = -self.ball.vel[2] * p.bounce;
            } else if self.bounces == 1 {
                self.point(hitter, events);
            } else {
                self.point(1 - hitter, events);
            }
        }
    }

    pub fn step(&mut self, events: &mut Vec<PointEvent>) {
        if self.phase == Phase::Over {
            return;
        }
        self.tick += 1;
        self.t = self.t.saturating_add(1);
        self.move_paddle(0);
        self.move_paddle(1);
        match self.phase {
            Phase::Serve => {
                self.hold_ball();
                if !self.try_hit(self.server, events) && self.t >= self.params.serve_ticks {
                    let server = self.server;
                    self.point(1 - server, events);
                }
            }
            Phase::Rally => {
                for seat in 0..2u8 {
                    if self.try_hit(seat, events) {
                        return;
                    }
                }
                self.fly(events);
            }
            Phase::Settle => {
                if self.t >= self.params.settle_ticks {
                    self.phase = Phase::Serve;
                    self.t = 0;
                    self.hold_ball();
                }
            }
            Phase::Over => {}
        }
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        let mut words = vec![
            self.tick,
            u64::from(self.score[0]),
            u64::from(self.score[1]),
            u64::from(self.server),
            self.phase.code().to_bits(),
            u64::from(self.t),
            self.hitter.map_or(u64::MAX, u64::from),
            u64::from(self.bounces),
            u64::from(self.hits),
            u64::from(self.rally),
            self.facing_at_hit.to_bits(),
        ];
        words.extend(
            self.ball
                .pos
                .iter()
                .chain(&self.ball.vel)
                .map(|v| v.to_bits()),
        );
        for p in &self.paddles {
            words.extend(
                p.pos
                    .iter()
                    .chain(&p.vel)
                    .chain(&p.target)
                    .chain([&p.facing])
                    .map(|v| v.to_bits()),
            );
            words.push(p.swing.map_or(u64::MAX, |s| {
                s.aim[0].to_bits()
                    ^ s.aim[1].to_bits().rotate_left(21)
                    ^ s.flight.to_bits().rotate_left(42)
            }));
        }
        words.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, w| {
            w.to_le_bytes()
                .iter()
                .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
        })
    }
}
