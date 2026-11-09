use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::QuickDraw;
use crate::rules::{Duel, Gunner, Params, Phase};

const FAKE_PEAK: f64 = 0.5;
const FAKE_WAIT: f64 = 0.45;
const BITE_HAND: [f64; 2] = [0.25, 0.5];
const ALERT: [f64; 3] = [0.35, 1.0, 1.0];
const EARLY_LEFT: u8 = 0;
const STAY: u8 = 1;
const EARLY_RIGHT: u8 = 2;
const LATE_LEFT: u8 = 3;
const LATE_RIGHT: u8 = 4;
const SHOWN_FAKE: u8 = 0;
const SHOWN_DRAW: u8 = 1;
const FOLLOW_UP: u8 = 2;
const LEADS: [f64; 3] = [-1.0, 0.0, 1.0];
const CENTRE: u8 = 1;

pub struct DuelPlaybook {
    params: Params,
}

impl DuelPlaybook {
    pub const WAIT: u16 = 0;
    pub const STRIKES: [u16; 3] = [1, 2, 3];
    pub const FAKE: u16 = 4;

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn class_of_lead(&self, lead: f64) -> u8 {
        let half = self.params.roll_dist * 0.5;
        if lead < -half {
            0
        } else if lead > half {
            2
        } else {
            1
        }
    }

    #[must_use]
    pub fn class_point(&self, class: u8) -> [f64; 2] {
        [0.0, (f64::from(class) - 1.0) * self.params.roll_dist]
    }

    fn landing(&self, s: &Duel, seat: usize) -> f64 {
        let g = &s.gunners[seat];
        let w = self.params.half_width;
        g.roll.map_or(g.x, |r| {
            (g.x + f64::from(r.dir) * self.params.roll_dist).clamp(-w, w)
        })
    }

    fn origin(&self, s: &Duel, seat: usize) -> f64 {
        s.gunners[seat].x
    }

    fn threatened(&self, s: &Duel, them: usize, repertoire: u8) -> bool {
        let g = &s.gunners[them];
        let incoming = s.bullets[them]
            .is_some_and(|b| (self.landing(s, 1 - them) - b.x).abs() < self.params.hit_radius);
        let seen = match repertoire {
            0 | 1 => g.loaded && g.hand > BITE_HAND[usize::from(repertoire)],
            _ => false,
        };
        seen || g.committed || incoming
    }

    fn action(me: PlayerId, aim: f64, hold: bool, roll: i8) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.values = Some(vec![aim as f32, f32::from(u8::from(hold)), f32::from(roll)]);
        a
    }

    fn dodge_dir(&self, g: &Gunner, noise: f64) -> i8 {
        let edge = self.params.half_width - self.params.roll_dist;
        if g.x > edge {
            -1
        } else if g.x < -edge || noise >= 0.0 {
            1
        } else {
            -1
        }
    }

    fn mixed(s: &Duel, me: usize, noise: f64) -> u64 {
        let them = &s.gunners[1 - me];
        let mut h = u64::from(s.round) << 40
            ^ u64::from(them.shots) << 20
            ^ u64::from(s.gunners[me].rolls)
            ^ u64::from(me as u8) << 60
            ^ noise.to_bits();
        h = h.wrapping_add(0x9e37_79b9_7f4a_7c15);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
        h
    }

    fn reflex(&self, s: &Duel, me: usize, repertoire: u8, noise: f64) -> i8 {
        let g = &s.gunners[me];
        if s.phase != Phase::Live || !g.can_roll() || !self.threatened(s, 1 - me, repertoire) {
            return 0;
        }
        let h = Self::mixed(s, me, noise);
        let alert = (h & 0xff) as f64 / 256.0 < ALERT[usize::from(repertoire.min(2))];
        if !alert {
            return 0;
        }
        let side = if repertoire >= 2 {
            if h >> 63 == 0 { 1.0 } else { -1.0 }
        } else {
            noise
        };
        self.dodge_dir(g, side)
    }

    fn draw_ticks(&self, s: &Duel, me: usize) -> u32 {
        let seconds = 1.0 / s.bodies[me].draw_rate;
        (seconds * f64::from(self.params.tick_hz)).round() as u32
    }

    fn strike_aim(&self, s: &Duel, me: usize, lead: usize) -> f64 {
        let w = self.params.half_width;
        let base = self.origin(s, 1 - me);
        (base + LEADS[lead.min(LEADS.len() - 1)] * self.params.roll_dist).clamp(-w, w)
    }

    fn lead_of_class(class: u8) -> usize {
        usize::from(class.min(2))
    }

    fn repertoire_of(choice: &Choice) -> u8 {
        choice.param[1] as u8
    }
}

impl Playbook<QuickDraw> for DuelPlaybook {
    fn options(&self, s: &Duel, me: PlayerId, repertoire: u8, cue: &Cue, out: &mut Vec<Choice>) {
        out.clear();
        let me = usize::from(me.min(1));
        let horizon = self.draw_ticks(s, me) + self.params.flight_ticks + 24;
        let choice = |id: u16, intent: u8, class: Option<u8>, deceptive: bool| Choice {
            id,
            intent,
            deceptive,
            horizon_ticks: horizon,
            param: [f32::from(class.unwrap_or(1)), f32::from(repertoire)],
            shown: class.map(|_| if deceptive { SHOWN_FAKE } else { SHOWN_DRAW }),
            real: None,
        };
        out.push(Choice {
            horizon_ticks: 24,
            ..choice(Self::WAIT, 2, None, false)
        });
        let g = &s.gunners[me];
        if s.phase != Phase::Live || !g.loaded {
            return;
        }
        if g.committed {
            match cue.held {
                Some(id) if Self::STRIKES.contains(&id) => {
                    let lead = Self::STRIKES.iter().position(|&s| s == id).unwrap_or(1) as u8;
                    out.push(choice(id, 1, Some(lead), false));
                }
                Some(Self::FAKE) => out.push(Choice {
                    horizon_ticks: horizon * 2,
                    ..choice(Self::FAKE, 1, Some(FOLLOW_UP), true)
                }),
                _ => {}
            }
            return;
        }
        if repertoire == 0 {
            out.push(choice(Self::STRIKES[1], 1, Some(CENTRE), false));
            return;
        }
        for (k, &id) in Self::STRIKES.iter().enumerate() {
            out.push(choice(id, 1, Some(k as u8), false));
        }
        if repertoire >= 2 {
            out.push(Choice {
                horizon_ticks: horizon * 2,
                ..choice(Self::FAKE, 1, Some(FOLLOW_UP), true)
            });
        }
    }

    fn act(&self, s: &Duel, me: PlayerId, c: &Choice, elapsed: u64, aim: [f64; 2]) -> ActionT {
        let seat = usize::from(me.min(1));
        let them = 1 - seat;
        let repertoire = Self::repertoire_of(c);
        let roll = self.reflex(s, seat, repertoire, aim[0]);
        let g = &s.gunners[seat];
        let lead = c.param[0] as usize;
        let hz = f64::from(self.params.tick_hz);
        match c.id {
            id if Self::STRIKES.contains(&id) => {
                let target = self.strike_aim(s, seat, lead) + aim[1];
                Self::action(me, target, g.loaded, roll)
            }
            Self::FAKE => {
                let rate = s.bodies[seat].draw_rate;
                let up = (FAKE_PEAK / rate * hz) as u64;
                let window = up + ((FAKE_PEAK / rate + FAKE_WAIT) * hz) as u64;
                let bitten = s.gunners[them].roll.is_some() || s.gunners[them].recovery > 0;
                let real = bitten || elapsed >= window;
                let hold = if real { g.loaded } else { elapsed < up };
                let lead = if bitten { CENTRE } else { lead as u8 };
                let target = self.strike_aim(s, seat, usize::from(lead)) + aim[1];
                Self::action(me, target, hold, roll)
            }
            _ => Self::action(me, s.gunners[them].x, false, roll),
        }
    }

    fn opponent_move(&self, s: &Duel, them: PlayerId, stance: Stance) -> ActionT {
        let seat = usize::from(them.min(1));
        let me = 1 - seat;
        let g = &s.gunners[seat];
        let mine = &s.gunners[me];
        let live = s.phase == Phase::Live && g.can_roll();
        let shot = mine.committed || s.bullets[me].is_some();
        let roll = match stance {
            Stance::Cover(k)
                if live
                    && (shot
                        || (k == EARLY_LEFT || k == EARLY_RIGHT) && mine.hand > BITE_HAND[0]) =>
            {
                match k {
                    EARLY_LEFT | LATE_LEFT => -1,
                    EARLY_RIGHT | LATE_RIGHT => 1,
                    _ => 0,
                }
            }
            Stance::Cover(_) => 0,
            Stance::Shoot(_) if live && shot => self.dodge_dir(g, 1.0),
            _ => 0,
        };
        match stance {
            Stance::Shoot(k) if g.loaded => Self::action(
                them,
                self.strike_aim(s, seat, Self::lead_of_class(k)),
                true,
                roll,
            ),
            _ => Self::action(them, mine.x, g.loaded && g.hand > 0.0, roll),
        }
    }

    fn evaluate(&self, s: &Duel, me: PlayerId) -> f64 {
        let i = usize::from(me.min(1));
        let j = 1 - i;
        let wins = f64::from(s.score[i]) - f64::from(s.score[j]);
        let threat = |shooter: usize| -> f64 {
            let target = self.landing(s, 1 - shooter);
            let g = &s.gunners[shooter];
            let aim = s.bullets[shooter]
                .map(|b| b.x)
                .or_else(|| g.committed.then_some(g.aim).flatten());
            match aim {
                Some(x) if (target - x).abs() < self.params.hit_radius => 3.0,
                Some(_) => -1.0,
                None => 0.0,
            }
        };
        let loaded =
            f64::from(u8::from(s.gunners[i].loaded)) - f64::from(u8::from(s.gunners[j].loaded));
        let live = if s.phase == Phase::Live {
            threat(i) - threat(j) + loaded
        } else {
            0.0
        };
        10.0 * wins + live
    }

    fn margin(&self) -> f64 {
        0.1
    }

    fn read(&self, before: &Duel, after: &Duel, them: PlayerId) -> Read {
        let mut read = Read::default();
        if before.round != after.round {
            return read;
        }
        let t = usize::from(them.min(1));
        let me = 1 - t;
        if before.bullets[t].is_none()
            && let Some(b) = after.bullets[t]
        {
            read.shot = Some(self.class_of_lead(b.lead));
        }
        if after.gunners[me].shots > before.gunners[me].shots {
            read.covered = Some(match after.gunners[me].last_response {
                -1 => EARLY_LEFT,
                1 => EARLY_RIGHT,
                -2 => LATE_LEFT,
                2 => LATE_RIGHT,
                _ => STAY,
            });
        }
        read
    }

    fn importance(&self, s: &Duel, _me: PlayerId) -> f64 {
        let close = s.score.iter().any(|&w| w + 1 >= self.params.wins);
        if close {
            1.0
        } else {
            f64::from(s.round) / f64::from(2 * self.params.wins)
        }
    }

    fn signal_classes(&self) -> u8 {
        5
    }

    fn possession(&self, s: &Duel, me: PlayerId) -> Option<u64> {
        let g = &s.gunners[usize::from(me.min(1))];
        let mine = usize::from(me.min(1));
        let armed = g.loaded || s.bullets[mine].is_some();
        let open = matches!(s.phase, Phase::Ready | Phase::Live);
        (open && armed).then_some(u64::from(s.round) << 16 | u64::from(g.shots))
    }
}
