use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::Boxing;
use crate::rules::{Bout, Height, Kind, Params, Phase, Stage, facing};

const SLACK: f64 = 0.08;
const AIM_NOISE: f64 = 1.0;
const SHOW_MIN: u32 = 24;
const GUARD_HORIZON: u32 = 96;
const TAIL: u32 = 24;
const PENDING: f64 = 5.0;
const POINT: f64 = 10.0;

pub struct BoxingPlaybook {
    params: Params,
}

pub fn order(
    me: PlayerId,
    target: f64,
    guard: Height,
    attack: Option<(Kind, Height)>,
    cancel: bool,
) -> ActionT {
    let mut a = ActionT::default();
    a.player = me;
    let (kind, height) = attack.map_or((0.0, 0.0), |(k, h)| (k.code(), h.code()));
    a.values = Some(
        [
            target,
            guard.code(),
            kind,
            height,
            f64::from(u8::from(cancel)),
        ]
        .map(|v| v as f32)
        .to_vec(),
    );
    a
}

struct View {
    seat: u8,
    mine: f64,
    theirs: f64,
}

impl View {
    fn of(s: &Bout, seat: u8) -> Self {
        let seat = seat.min(1);
        let m = facing(seat);
        Self {
            seat,
            mine: m * s.fighters[usize::from(seat)].x,
            theirs: m * s.fighters[usize::from(1 - seat)].x,
        }
    }

    fn gap(&self) -> f64 {
        self.theirs - self.mine
    }
}

impl BoxingPlaybook {
    pub const WAIT: u16 = 0;
    pub const GUARDS: [u16; 2] = [1, 2];
    pub const SHOTS: u16 = 10;
    pub const FEINTS: u16 = 20;

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn class_point(&self, class: u8) -> [f64; 2] {
        [0.0, Height::of_class(class).code()]
    }

    fn shot_id(kind: Kind, height: Height) -> u16 {
        Self::SHOTS + 2 * u16::from(kind == Kind::Hook) + u16::from(height.class())
    }

    fn shot_of(id: u16) -> (Kind, Height) {
        let k = id - Self::SHOTS;
        let kind = if k >= 2 { Kind::Hook } else { Kind::Jab };
        (kind, Height::of_class((k % 2) as u8))
    }

    fn approach(&self, s: &Bout, v: &View, range: f64) -> u32 {
        let speed = s.bodies[usize::from(v.seat)].speed.max(1e-3);
        let far = (v.gap() - range).max(0.0);
        (far / speed * f64::from(self.params.tick_hz)) as u32
    }

    fn range(&self, kind: Kind, noise: f64) -> f64 {
        self.params.reach(kind) - SLACK + noise * AIM_NOISE
    }

    fn strike_at(
        v: &View,
        range: f64,
        idle: bool,
        attack: (Kind, Height),
    ) -> Option<(Kind, Height)> {
        (idle && v.gap() <= range + 0.02).then_some(attack)
    }
}

impl Playbook<Boxing> for BoxingPlaybook {
    fn options(&self, s: &Bout, me: PlayerId, repertoire: u8, _cue: &Cue, out: &mut Vec<Choice>) {
        out.clear();
        let me = me.min(1);
        if s.phase != Phase::Fight {
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
        let v = View::of(s, me);
        let guards: &[Height] = if repertoire == 0 {
            &[Height::High]
        } else {
            &[Height::Low, Height::High]
        };
        for &h in guards {
            out.push(Choice {
                id: Self::GUARDS[usize::from(h.class())],
                intent: 2,
                deceptive: false,
                horizon_ticks: GUARD_HORIZON,
                param: [0.0, 0.0],
                shown: None,
                real: Some(h.class()),
            });
        }
        let shots: &[(Kind, Height)] = if repertoire == 0 {
            &[(Kind::Jab, Height::High)]
        } else {
            &[
                (Kind::Jab, Height::Low),
                (Kind::Jab, Height::High),
                (Kind::Hook, Height::Low),
                (Kind::Hook, Height::High),
            ]
        };
        for &(kind, height) in shots {
            let travel = self.approach(s, &v, self.range(kind, 0.0));
            out.push(Choice {
                id: Self::shot_id(kind, height),
                intent: 1,
                deceptive: false,
                horizon_ticks: travel + s.windup(me, kind) + TAIL,
                param: [0.0, 0.0],
                shown: Some(height.class()),
                real: Some(height.class()),
            });
        }
        if repertoire >= 2 {
            let travel = self.approach(s, &v, self.range(Kind::Hook, 0.0));
            let length = travel
                + s.windup(me, Kind::Hook)
                + self.params.cancel_recover
                + s.windup(me, Kind::Jab)
                + TAIL;
            for shown in [Height::Low, Height::High] {
                out.push(Choice {
                    id: Self::FEINTS + u16::from(shown.class()),
                    intent: 3,
                    deceptive: true,
                    horizon_ticks: length,
                    param: [s.fighters[usize::from(me)].feints as f32, 0.0],
                    shown: Some(shown.class()),
                    real: Some(shown.other().class()),
                });
            }
        }
    }

    fn act(&self, s: &Bout, me: PlayerId, c: &Choice, _elapsed: u64, aim: [f64; 2]) -> ActionT {
        let seat = me.min(1);
        let v = View::of(s, seat);
        let f = &s.fighters[usize::from(seat)];
        let idle = f.stage == Stage::Idle;
        if Self::GUARDS.contains(&c.id) {
            let h = Height::of_class(c.real.unwrap_or(1));
            return order(me, v.mine, h, None, false);
        }
        if (Self::SHOTS..Self::FEINTS).contains(&c.id) {
            let (kind, height) = Self::shot_of(c.id);
            let range = self.range(kind, aim[0]);
            let attack = Self::strike_at(&v, range, idle, (kind, height));
            return order(me, v.theirs - range, f.guard, attack, false);
        }
        if c.id >= Self::FEINTS {
            let shown = Height::of_class(c.shown.unwrap_or(1));
            let real = shown.other();
            let range = self.range(Kind::Hook, aim[0]);
            let jab = self.range(Kind::Jab, aim[0]);
            let target = v.theirs - range;
            let (attack, cancel) = match f.stage {
                Stage::WindUp {
                    kind: Kind::Hook,
                    height,
                    left,
                } if height == shown => {
                    let shown_for = s.windup(seat, Kind::Hook).saturating_sub(left);
                    let bit = s.fighters[usize::from(1 - seat)].guard == shown;
                    (None, shown_for >= SHOW_MIN && bit)
                }
                Stage::Recover { .. } => (Self::strike_at(&v, jab, true, (Kind::Jab, real)), false),
                Stage::Idle if f64::from(f.feints) == f64::from(c.param[0]) => {
                    (Self::strike_at(&v, range, true, (Kind::Hook, shown)), false)
                }
                Stage::Idle => (Self::strike_at(&v, jab, true, (Kind::Jab, real)), false),
                _ => (None, false),
            };
            return order(me, target, f.guard, attack, cancel);
        }
        order(me, v.mine, f.guard, None, false)
    }

    fn opponent_move(&self, s: &Bout, them: PlayerId, stance: Stance) -> ActionT {
        let seat = them.min(1);
        let v = View::of(s, seat);
        let f = &s.fighters[usize::from(seat)];
        let o = &s.fighters[usize::from(1 - seat)];
        match stance {
            Stance::Still => order(them, v.mine, f.guard, None, false),
            Stance::Cover(k) => order(them, v.mine, Height::of_class(k), None, false),
            Stance::Shoot(k) => {
                let range = self.range(Kind::Jab, 0.0);
                let attack = Self::strike_at(
                    &v,
                    range,
                    f.stage == Stage::Idle,
                    (Kind::Jab, Height::of_class(k)),
                );
                order(them, v.theirs - range, f.guard, attack, false)
            }
            Stance::Naive => {
                let guard = match o.stage {
                    Stage::WindUp { height, .. } => height,
                    _ => f.guard,
                };
                order(them, v.mine, guard, None, false)
            }
        }
    }

    fn evaluate(&self, s: &Bout, me: PlayerId) -> f64 {
        let me = me.min(1);
        let them = 1 - me;
        let points = f64::from(s.score[usize::from(me)]) - f64::from(s.score[usize::from(them)]);
        let gap = s.gap();
        let mut pending = 0.0;
        if s.phase == Phase::Fight {
            for (seat, sign) in [(me, 1.0), (them, -1.0)] {
                let f = &s.fighters[usize::from(seat)];
                let other = &s.fighters[usize::from(1 - seat)];
                if let Stage::WindUp { kind, height, .. } = f.stage
                    && gap <= self.params.reach(kind)
                    && other.guard != height
                {
                    pending += sign * f64::from(self.params.points(kind));
                }
            }
        }
        POINT * points + PENDING * pending
    }

    fn margin(&self) -> f64 {
        0.1
    }

    fn read(&self, before: &Bout, after: &Bout, them: PlayerId) -> Read {
        let them = usize::from(them.min(1));
        let me = 1 - them;
        let mut read = Read::default();
        if after.fighters[them].strikes != before.fighters[them].strikes {
            read.shot = after.fighters[them].last.map(Height::class);
        }
        if after.fighters[me].strikes != before.fighters[me].strikes {
            read.covered = Some(after.fighters[them].guard.class());
        }
        read
    }

    fn signal(&self, s: &Bout, them: PlayerId) -> Option<u8> {
        if s.phase != Phase::Fight {
            return None;
        }
        match s.fighters[usize::from(them.min(1))].stage {
            Stage::WindUp { height, .. } => Some(height.class()),
            _ => None,
        }
    }

    fn signal_classes(&self) -> u8 {
        2
    }

    fn importance(&self, s: &Bout, _me: PlayerId) -> f64 {
        let top = s.score[0].max(s.score[1]);
        if top + self.params.hook_points >= self.params.points_to_win {
            1.0
        } else {
            f64::from(top) / f64::from(self.params.points_to_win)
        }
    }

    fn unknown(&self, s: &Bout, _me: PlayerId) -> Option<Vec<(Stance, f64)>> {
        (s.phase == Phase::Fight).then(|| vec![(Stance::Shoot(0), 0.5), (Stance::Shoot(1), 0.5)])
    }

    fn possession(&self, s: &Bout, _me: PlayerId) -> Option<u64> {
        (s.phase == Phase::Fight)
            .then(|| u64::from(s.fighters[0].strikes) + u64::from(s.fighters[1].strikes))
    }
}
