use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::Penalty;
use crate::rules::{Params, Phase, Shootout};

const AIM: f64 = 2.8;
const SPLIT: f64 = 1.4;
const LEAN_SHOW: f64 = 0.9;
const READ_LEAN: f64 = 0.3;
const AIM_SCALE: f64 = 4.0;
const STUTTER_AT: f64 = 1.2;
const STUTTER_TICKS: u64 = 96;
const GUESS_LEAD: f64 = 0.15;
const LEAN_READ_LEAD: f64 = 0.1;
const OPPONENT_REACTION: f64 = 0.3;
const ROOKIE_MIDDLE: f64 = 0.045;
const ROOKIE: u8 = 3;

pub struct PenaltyPlaybook {
    params: Params,
}

impl PenaltyPlaybook {
    pub const WAIT: u16 = 0;
    pub const STRIKES: [u16; 3] = [1, 2, 3];
    pub const FAKES: [u16; 2] = [4, 5];
    pub const STUTTERS: [u16; 3] = [6, 7, 8];
    pub const REACT: u16 = 9;
    pub const GUESSES: [u16; 3] = [10, 11, 12];
    pub const READ: u16 = 13;

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn class_y(class: u8) -> f64 {
        match class {
            0 => -AIM,
            2 => AIM,
            _ => 0.0,
        }
    }

    #[must_use]
    pub fn class_of(y: f64) -> u8 {
        if y < -SPLIT {
            0
        } else if y > SPLIT {
            2
        } else {
            1
        }
    }

    #[must_use]
    pub fn class_point(&self, class: u8) -> [f64; 2] {
        [self.params.half_goal, Self::class_y(class)]
    }

    fn lean_of(class: u8) -> f64 {
        match class {
            0 => -LEAN_SHOW,
            2 => LEAN_SHOW,
            _ => 0.0,
        }
    }

    fn lean_class(lean: f64) -> Option<u8> {
        (lean.abs() > READ_LEAN).then_some(if lean < 0.0 { 0 } else { 2 })
    }

    fn near_kick(&self, s: &Shootout, lead: f64) -> bool {
        matches!(s.phase, Phase::AtBall | Phase::Strike)
            || (s.phase == Phase::RunUp
                && s.dist <= self.params.kick_range + self.params.run_speed * lead)
    }

    fn predicted_aim(&self, s: &Shootout) -> Option<f64> {
        if s.phase != Phase::Flight || s.t == 0 {
            return None;
        }
        let f = f64::from(self.params.flight_ticks);
        Some(s.ball_y * f / f64::from(s.t.min(self.params.flight_ticks)))
    }

    fn action(me: PlayerId, values: [f64; 4]) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.values = Some(values.map(|v| v as f32).to_vec());
        a
    }

    fn shoot(
        &self,
        s: &Shootout,
        me: PlayerId,
        shown: u8,
        real: u8,
        run: bool,
        noise: f64,
    ) -> ActionT {
        let fire = self.near_kick(s, 0.0) && run;
        let aim = Self::class_y(real) + noise;
        Self::action(
            me,
            [
                Self::lean_of(shown),
                f64::from(u8::from(run)),
                f64::from(u8::from(fire)),
                aim,
            ],
        )
    }

    fn keep(&self, s: &Shootout, me: PlayerId, early: Option<f64>) -> ActionT {
        self.keep_after(s, me, early, 0)
    }

    fn keep_after(&self, s: &Shootout, me: PlayerId, early: Option<f64>, delay: u32) -> ActionT {
        if let Some(target) = early.filter(|_| !s.dived) {
            return Self::action(me, [0.0, 0.0, 1.0, target]);
        }
        let react = self
            .predicted_aim(s)
            .filter(|_| s.t >= delay)
            .filter(|aim| (aim - s.keeper_y).abs() > self.params.keeper_reach * 0.5 && !s.dived);
        match react {
            Some(aim) => Self::action(me, [0.0, 0.0, 1.0, aim]),
            None => Self::action(me, [0.0, 0.0, 0.0, 0.0]),
        }
    }

    fn kick_horizon(&self, s: &Shootout) -> u32 {
        let hz = f64::from(self.params.tick_hz);
        let run = (s.dist / self.params.run_speed * hz).round() as u32;
        run + self.params.windup_ticks + self.params.flight_ticks + 24
    }
}

impl Playbook<Penalty> for PenaltyPlaybook {
    fn options(
        &self,
        s: &Shootout,
        me: PlayerId,
        repertoire: u8,
        cue: &Cue,
        out: &mut Vec<Choice>,
    ) {
        out.clear();
        let choice =
            |id: u16, intent: u8, horizon: u32, shown: Option<u8>, real: Option<u8>| Choice {
                id,
                intent,
                deceptive: false,
                horizon_ticks: horizon.max(1),
                param: [f32::from(real.unwrap_or(1)), f32::from(shown.unwrap_or(1))],
                shown,
                real,
            };
        if matches!(s.phase, Phase::Settle | Phase::Over) {
            out.push(choice(Self::WAIT, 0, 24, None, None));
            return;
        }
        let horizon = self.kick_horizon(s);
        if me != s.shooter() {
            out.push(choice(Self::REACT, 2, horizon, None, None));
            let open = !s.dived && s.phase != Phase::Flight;
            if repertoire >= 1 && open {
                for (k, &id) in Self::GUESSES.iter().enumerate() {
                    out.push(choice(id, 2, horizon, None, Some(k as u8)));
                }
            }
            if repertoire >= 2 && open {
                out.push(choice(Self::READ, 2, horizon, None, None));
            }
            return;
        }
        let kicked = matches!(s.phase, Phase::Strike | Phase::Flight);
        let strikes: &[u16] = if repertoire == 0 {
            &Self::STRIKES[..1]
        } else {
            &Self::STRIKES
        };
        for (k, &id) in strikes.iter().enumerate() {
            if repertoire == 0 {
                out.push(Choice {
                    param: [f32::from(ROOKIE), 0.0],
                    ..choice(id, 1, horizon, None, None)
                });
            } else {
                out.push(choice(id, 1, horizon, Some(k as u8), Some(k as u8)));
            }
        }
        if repertoire >= 2 {
            for (&id, shown) in Self::FAKES.iter().zip([0u8, 2]) {
                out.push(Choice {
                    deceptive: true,
                    ..choice(id, 1, horizon, Some(shown), Some(2 - shown))
                });
            }
            if s.dist > STUTTER_AT || cue.held.is_some_and(|h| Self::STUTTERS.contains(&h)) {
                for (k, &id) in Self::STUTTERS.iter().enumerate() {
                    out.push(Choice {
                        deceptive: true,
                        ..choice(id, 1, horizon + STUTTER_TICKS as u32, None, Some(k as u8))
                    });
                }
            }
        }
        if kicked {
            out.retain(|c| cue.held == Some(c.id));
            if out.is_empty() {
                out.push(choice(Self::WAIT, 0, 24, None, None));
            }
        }
    }

    fn act(&self, s: &Shootout, me: PlayerId, c: &Choice, elapsed: u64, aim: [f64; 2]) -> ActionT {
        if me == s.shooter() && matches!(s.phase, Phase::Strike | Phase::Flight) {
            return Self::action(me, [0.0, 1.0, 0.0, 0.0]);
        }
        let noise = aim[1] * AIM_SCALE;
        let rookie = c.param[0] as u8 == ROOKIE && c.real.is_none();
        let real = match c.real {
            _ if rookie && aim[0] < -ROOKIE_MIDDLE => 0,
            _ if rookie && aim[0] > ROOKIE_MIDDLE => 2,
            Some(k) => k,
            None => 1,
        };
        match c.id {
            id if Self::STRIKES.contains(&id) || Self::FAKES.contains(&id) => {
                self.shoot(s, me, c.shown.unwrap_or(real), real, true, noise)
            }
            id if Self::STUTTERS.contains(&id) => {
                let pausing = s.dist <= STUTTER_AT && elapsed < STUTTER_TICKS + 60;
                let held = pausing && s.running && s.dist > self.params.kick_range;
                self.shoot(s, me, real, real, !held, noise)
            }
            id if Self::GUESSES.contains(&id) => {
                let early = self
                    .near_kick(s, GUESS_LEAD)
                    .then(|| Self::class_y(c.real.unwrap_or(1)) + noise);
                self.keep(s, me, early)
            }
            Self::READ => {
                let early = (self.near_kick(s, LEAN_READ_LEAD) && s.lean.abs() > READ_LEAN)
                    .then(|| AIM * s.lean.signum());
                self.keep(s, me, early)
            }
            Self::REACT => self.keep(s, me, None),
            _ => Self::action(me, [0.0, 1.0, 0.0, 0.0]),
        }
    }

    fn opponent_move(&self, s: &Shootout, them: PlayerId, stance: Stance) -> ActionT {
        if them == s.shooter() {
            let by_lean = Self::lean_class(s.lean).unwrap_or(1);
            let class = match stance {
                Stance::Shoot(k) => k,
                _ => by_lean,
            };
            let run = !matches!(stance, Stance::Still);
            return self.shoot(s, them, by_lean, class, run, 0.0);
        }
        match stance {
            Stance::Cover(k) => {
                let early = self.near_kick(s, GUESS_LEAD).then(|| Self::class_y(k));
                self.keep(s, them, early)
            }
            Stance::Still => Self::action(them, [s.keeper_y, 0.0, 0.0, 0.0]),
            _ => {
                let delay = (OPPONENT_REACTION * f64::from(self.params.tick_hz)) as u32;
                self.keep_after(s, them, None, delay)
            }
        }
    }

    fn evaluate(&self, s: &Shootout, me: PlayerId) -> f64 {
        let i = usize::from(me.min(1));
        let goals = f64::from(s.score[i]) - f64::from(s.score[1 - i]);
        let mine = me == s.shooter();
        let pending = match (s.phase, s.aim) {
            (Phase::Flight, Some(aim)) => {
                let left =
                    f64::from(self.params.flight_ticks.saturating_sub(s.t)) * self.params.dt();
                let body = s.bodies[usize::from(s.keeper())];
                let reach = self.params.keeper_reach
                    + match s.dive {
                        Some(d)
                            if d.left > 0 && (d.target - s.keeper_y) * (aim - s.keeper_y) > 0.0 =>
                        {
                            body.dive_speed * left
                        }
                        Some(_) => 0.0,
                        None => body.dive_speed * left,
                    };
                let goal = aim.abs() < self.params.half_goal && (aim - s.keeper_y).abs() > reach;
                if goal { 3.0 } else { -3.0 }
            }
            _ => 0.0,
        };
        10.0 * goals + if mine { pending } else { -pending }
    }

    fn margin(&self) -> f64 {
        0.1
    }

    fn read(&self, before: &Shootout, after: &Shootout, them: PlayerId) -> Read {
        let mut read = Read::default();
        if before.kicks != after.kicks {
            return read;
        }
        if them == after.keeper() && after.dived && !before.dived {
            read.covered = after.dive.map(|d| Self::class_of(d.target));
        }
        let aimed = |x: &Shootout| x.phase == Phase::Flight && x.aim.is_some();
        if them == after.shooter() && aimed(after) && !aimed(before) {
            read.shot = after.aim.map(Self::class_of);
        }
        read
    }

    fn signal(&self, s: &Shootout, them: PlayerId) -> Option<u8> {
        let open = matches!(s.phase, Phase::RunUp | Phase::AtBall | Phase::Strike);
        Self::lean_class(s.lean).filter(|_| them == s.shooter() && open)
    }

    fn importance(&self, s: &Shootout, _me: PlayerId) -> f64 {
        let regular = 2 * self.params.kicks_each;
        if s.kicks + 2 >= regular {
            1.0
        } else {
            f64::from(s.kicks) / f64::from(regular)
        }
    }

    fn possession(&self, s: &Shootout, me: PlayerId) -> Option<u64> {
        (me == s.shooter() && !matches!(s.phase, Phase::Settle | Phase::Over))
            .then_some(u64::from(s.kicks))
    }
}
