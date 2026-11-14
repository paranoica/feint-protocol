use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::{PingPong, mirror};
use crate::flight::{coming_to, intercept, landing};
use crate::rules::{Params, Phase, Table};

const SIDE_Y: f64 = 0.5;
const DEEP: f64 = 1.1;
const SHORT: f64 = 0.55;
const FAST: f64 = 0.42;
const SLOW: f64 = 0.7;
const SHOWN: f64 = 0.8;
const READ_FACING: f64 = 0.3;
const READY_BACK: f64 = 0.5;
const COVER_Y: f64 = 0.4;
const AIM_NOISE: f64 = 3.0;
const SERVE_AFTER: u32 = 120;

pub struct PingPongPlaybook {
    params: Params,
}

fn class_of_y(y: f64) -> u8 {
    if y < -0.2 {
        0
    } else if y > 0.2 {
        2
    } else {
        1
    }
}

fn class_y(class: u8) -> f64 {
    (f64::from(class) - 1.0) * SIDE_Y
}

impl PingPongPlaybook {
    pub const WAIT: u16 = 0;
    pub const COVER: u16 = 1;
    pub const COVERS: [u16; 3] = [2, 3, 4];
    pub const SHOTS: u16 = 10;

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn class_point(&self, class: u8) -> [f64; 2] {
        [self.params.half_length * 0.6, class_y(class)]
    }

    fn mine(s: &Table, me: u8) -> bool {
        (s.phase == Phase::Serve && s.server == me) || coming_to(s, me)
    }

    fn shot(id: u16, horizon: u32, real: u8, shown: u8, depth: f64, flight: f64) -> Choice {
        Choice {
            id,
            intent: if real == shown { 1 } else { 3 },
            deceptive: real != shown,
            horizon_ticks: horizon.max(1),
            param: [depth as f32, flight as f32],
            shown: Some(shown),
            real: Some(real),
        }
    }

    fn ready(&self) -> [f64; 2] {
        [-(self.params.half_length + READY_BACK), 0.0]
    }

    fn action(
        me: PlayerId,
        target: [f64; 2],
        swing: bool,
        aim: [f64; 2],
        flight: f64,
        facing: f64,
    ) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.values = Some(
            [
                target[0],
                target[1],
                f64::from(u8::from(swing)),
                aim[0],
                aim[1],
                flight,
                facing,
            ]
            .map(|v| v as f32)
            .to_vec(),
        );
        a
    }

    fn own(seat: u8, world: [f64; 2]) -> [f64; 2] {
        let m = mirror(seat);
        [m * world[0], m * world[1]]
    }

    fn chase(&self, s: &Table, me: u8) -> Option<[f64; 2]> {
        intercept(s, me).map(|i| Self::own(me, i.at))
    }
}

impl Playbook<PingPong> for PingPongPlaybook {
    fn options(&self, s: &Table, me: PlayerId, repertoire: u8, _cue: &Cue, out: &mut Vec<Choice>) {
        out.clear();
        let me = me.min(1);
        if matches!(s.phase, Phase::Settle | Phase::Over) {
            out.push(Choice {
                id: Self::WAIT,
                intent: 0,
                deceptive: false,
                horizon_ticks: 24,
                param: [0.0, 0.0],
                shown: None,
                real: None,
            });
            return;
        }
        let flight_ticks = |f: f64| (f * f64::from(self.params.tick_hz)) as u32;
        if Self::mine(s, me) {
            let until = intercept(s, me).map_or(SERVE_AFTER, |i| i.ticks);
            if repertoire == 0 {
                out.push(Self::shot(
                    Self::SHOTS,
                    until + flight_ticks(SLOW),
                    1,
                    1,
                    DEEP,
                    SLOW,
                ));
                return;
            }
            let mut id = Self::SHOTS;
            let paces: &[f64] = if repertoire >= 2 {
                &[FAST, SLOW]
            } else {
                &[SLOW]
            };
            let depths: &[f64] = if repertoire >= 2 {
                &[DEEP, SHORT]
            } else {
                &[DEEP]
            };
            for &flight in paces {
                for &depth in depths {
                    for real in 0..3u8 {
                        out.push(Self::shot(
                            id,
                            until + flight_ticks(flight),
                            real,
                            real,
                            depth,
                            flight,
                        ));
                        id += 1;
                        if repertoire >= 2 && real != 1 {
                            out.push(Self::shot(
                                id,
                                until + flight_ticks(flight),
                                real,
                                2 - real,
                                depth,
                                flight,
                            ));
                            id += 1;
                        }
                    }
                }
            }
            return;
        }
        let horizon = landing(s).map_or(48, |_| 96);
        out.push(Choice {
            id: Self::COVER,
            intent: 2,
            deceptive: false,
            horizon_ticks: horizon,
            param: [0.0, 0.0],
            shown: None,
            real: None,
        });
        if repertoire >= 1 {
            for (k, &id) in Self::COVERS.iter().enumerate() {
                out.push(Choice {
                    id,
                    intent: 2,
                    deceptive: false,
                    horizon_ticks: horizon,
                    param: [0.0, 0.0],
                    shown: None,
                    real: Some(k as u8),
                });
            }
        }
    }

    fn act(&self, s: &Table, me: PlayerId, c: &Choice, elapsed: u64, aim: [f64; 2]) -> ActionT {
        let seat = me.min(1);
        let ready = self.ready();
        if c.id >= Self::SHOTS {
            let real = c.real.unwrap_or(1);
            let shown = c.shown.unwrap_or(real);
            let facing = if shown == 1 {
                0.0
            } else {
                (f64::from(shown) - 1.0) * SHOWN
            };
            let aim_y = class_y(real) + aim[1] * AIM_NOISE;
            let target = if s.phase == Phase::Serve {
                Self::own(seat, s.paddles[usize::from(seat)].pos)
            } else {
                self.chase(s, seat).unwrap_or(ready)
            };
            let swing = s.phase != Phase::Serve || elapsed >= u64::from(SERVE_AFTER);
            let depth = f64::from(c.param[0]) + aim[0] * AIM_NOISE;
            return Self::action(
                me,
                target,
                swing,
                [depth, aim_y],
                f64::from(c.param[1]),
                facing,
            );
        }
        let target = match c.real {
            Some(k) => [ready[0], class_y(k) * COVER_Y / SIDE_Y],
            None => self.chase(s, seat).unwrap_or(ready),
        };
        Self::action(me, target, false, [DEEP, 0.0], SLOW, 0.0)
    }

    fn opponent_move(&self, s: &Table, them: PlayerId, stance: Stance) -> ActionT {
        let seat = them.min(1);
        let ready = self.ready();
        let target = match stance {
            Stance::Still => Self::own(seat, s.paddles[usize::from(seat)].pos),
            Stance::Cover(k) => [ready[0], -class_y(k) * COVER_Y / SIDE_Y],
            Stance::Shoot(_) | Stance::Naive => self.chase(s, seat).unwrap_or(ready),
        };
        let real = match stance {
            Stance::Shoot(k) => 2 - k.min(2),
            _ => 1,
        };
        Self::action(them, target, true, [DEEP, class_y(real)], SLOW, 0.0)
    }

    fn evaluate(&self, s: &Table, me: PlayerId) -> f64 {
        let me = me.min(1);
        let them = 1 - me;
        let goals = f64::from(s.score[usize::from(me)]) - f64::from(s.score[usize::from(them)]);
        let pending = match s.hitter {
            Some(h) if s.phase == Phase::Rally => {
                let receiver = 1 - h;
                let sign = if h == me { 1.0 } else { -1.0 };
                match intercept(s, receiver) {
                    None => {
                        sign * if s.bounces == 0 && landing(s).is_none() {
                            -1.0
                        } else {
                            0.0
                        }
                    }
                    Some(i) => {
                        let pad = s.paddles[usize::from(receiver)];
                        let d = [i.at[0] - pad.pos[0], i.at[1] - pad.pos[1]];
                        let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                        let time = f64::from(i.ticks) * self.params.dt();
                        let can = s.bodies[usize::from(receiver)].speed * time + self.params.reach;
                        sign * ((dist - can) / self.params.reach).clamp(-1.0, 1.0)
                    }
                }
            }
            _ => 0.0,
        };
        10.0 * goals + 3.0 * pending
    }

    fn margin(&self) -> f64 {
        0.1
    }

    fn read(&self, before: &Table, after: &Table, them: PlayerId) -> Read {
        let them = them.min(1);
        let mut read = Read::default();
        if after.hits != before.hits && after.hitter == Some(them) {
            let me = 1 - them;
            read.shot = landing(after).map(|l| class_of_y(mirror(me) * l[1]));
        }
        read
    }

    fn signal(&self, s: &Table, them: PlayerId) -> Option<u8> {
        let them = them.min(1);
        if !Self::mine(s, them) {
            return None;
        }
        let me = 1 - them;
        let f = mirror(me) * s.paddles[usize::from(them)].facing;
        (f.abs() > READ_FACING).then_some(if f < 0.0 { 0 } else { 2 })
    }

    fn importance(&self, s: &Table, _me: PlayerId) -> f64 {
        let top = s.score[0].max(s.score[1]);
        if top + 1 >= self.params.points_to_win {
            1.0
        } else {
            f64::from(top) / f64::from(self.params.points_to_win)
        }
    }

    fn unknown(&self, s: &Table, me: PlayerId) -> Option<Vec<(Stance, f64)>> {
        let third = 1.0 / 3.0;
        (!Self::mine(s, me.min(1)) && s.phase != Phase::Settle)
            .then(|| (0..3).map(|k| (Stance::Shoot(k), third)).collect())
    }

    fn possession(&self, s: &Table, me: PlayerId) -> Option<u64> {
        Self::mine(s, me.min(1)).then_some(u64::from(s.hits))
    }
}
