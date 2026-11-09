use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::AirHockey;
use crate::game::Match;
use crate::params::Params;
use crate::vec2::Vec2;

mod geometry;
mod reads;

use geometry::{
    AIM_SCALE, AIM_Y, CHASE_SCALE, CONTROL_SPEED, HOLD_RESERVE_SECONDS, HOLD_TICKS,
    MIN_BANK_RESTITUTION, SHOW_BACK, SHOW_SIDE, SHOW_TICKS, SLOW_TICKS, STRIKE_TICKS, attack,
    frame,
};

pub struct HockeyPlaybook {
    params: Params,
}

impl HockeyPlaybook {
    pub const CHASE: u16 = 0;
    pub const DEFEND: u16 = 1;
    pub const STRIKES: [u16; 3] = [2, 3, 4];
    pub const DEFENDS: [u16; 3] = [5, 6, 7];
    pub const FAKES: [u16; 2] = [8, 9];
    pub const BANK: u16 = 10;
    pub const HOLD: u16 = 11;
    pub const DASHES: [u16; 3] = [12, 13, 14];
    pub const FAKE_BANK: u16 = 15;

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn contact_target(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        self.contact(state, player, aim)
    }

    #[must_use]
    pub fn show_point(&self, state: &Match, player: PlayerId, shown: u8) -> Vec2 {
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let side = match shown {
            0 => SHOW_SIDE,
            2 => -SHOW_SIDE,
            _ => 0.0,
        };
        state.world.puck.pos + frame(player, Vec2::new(-(reach + SHOW_BACK), side))
    }

    #[must_use]
    pub fn bank_aim(&self, state: &Match, player: PlayerId) -> Vec2 {
        let rail =
            self.bank_side(state, player) * (self.params.half_width - self.params.puck_radius);
        let beyond = rail + rail / self.params.rail_restitution;
        frame(player, Vec2::new(self.params.half_length, beyond))
    }

    fn bank_side(&self, state: &Match, player: PlayerId) -> f64 {
        let them = state.world.mallets[usize::from(1 - player.min(1))].pos;
        if frame(player, them).y >= 0.0 {
            -1.0
        } else {
            1.0
        }
    }

    fn slow(c: &Choice) -> bool {
        c.param[1] < 0.5
    }

    #[must_use]
    pub fn class_point(&self, attacker: PlayerId, class: u8) -> Vec2 {
        self.corner_aim(attacker, class)
    }

    #[must_use]
    pub fn cover_spot(&self, state: &Match, defender: PlayerId, class: u8) -> Vec2 {
        let attacker = 1 - defender.min(1);
        Self::guard(self.corner_aim(attacker, class), state.world.puck.pos)
    }
}

impl Playbook<AirHockey> for HockeyPlaybook {
    fn options(
        &self,
        state: &Match,
        me: PlayerId,
        repertoire: u8,
        cue: &Cue,
        out: &mut Vec<Choice>,
    ) {
        out.clear();
        let pace = cue.pace;
        let shot_ticks = STRIKE_TICKS + if pace < 0.5 { SLOW_TICKS } else { 0 };
        let fake_ticks = STRIKE_TICKS + SHOW_TICKS as u32;
        let choice = |id: u16, intent: u8, horizon_ticks: u32, k: f32, corner: Option<u8>| Choice {
            id,
            intent,
            deceptive: false,
            horizon_ticks,
            param: [k, pace],
            shown: corner,
            real: corner,
        };
        if repertoire == 0 {
            out.push(choice(Self::CHASE, 1, 60, 0.0, None));
            return;
        }
        if repertoire >= 2 {
            for (k, &id) in Self::DEFENDS.iter().enumerate() {
                out.push(choice(id, 2, 120, k as f32, None));
            }
        } else {
            out.push(choice(Self::DEFEND, 2, 120, 0.0, None));
        }
        if !Self::puck_in_half_of(state, me) {
            return;
        }
        for (k, &id) in Self::STRIKES.iter().enumerate() {
            out.push(choice(id, 1, shot_ticks, k as f32, Some(k as u8)));
        }
        if repertoire < 2 {
            return;
        }
        let banks = self.params.rail_restitution >= MIN_BANK_RESTITUTION;
        if banks {
            out.push(Choice {
                shown: None,
                ..choice(Self::BANK, 1, shot_ticks, 1.0, Some(1))
            });
        }
        if state.world.puck.vel.length() < CONTROL_SPEED {
            for (&id, shown) in Self::FAKES.iter().zip([0u8, 2]) {
                let real = 2 - shown;
                out.push(Choice {
                    deceptive: true,
                    shown: Some(shown),
                    ..choice(id, 1, fake_ticks, f32::from(real), Some(real))
                });
            }
            if banks {
                let shown = if self.bank_side(state, me) < 0.0 {
                    2
                } else {
                    0
                };
                out.push(Choice {
                    deceptive: true,
                    shown: Some(shown),
                    ..choice(Self::FAKE_BANK, 1, fake_ticks, 1.0, Some(1))
                });
            }
            let hz = f64::from(self.params.tick_hz);
            let reserve = f64::from(state.stall_ticks()) + HOLD_RESERVE_SECONDS * hz;
            if reserve < self.params.stall_seconds * hz {
                out.push(Choice {
                    deceptive: true,
                    shown: Some(1),
                    ..choice(Self::HOLD, 1, HOLD_TICKS, 1.0, None)
                });
            }
        }
        if let Some(twitch) = cue.twitch.filter(|&t| usize::from(t) < AIM_Y.len()) {
            for real in [0u8, 2].into_iter().filter(|&r| r != twitch) {
                out.push(Choice {
                    shown: None,
                    ..choice(
                        Self::DASHES[usize::from(real)],
                        1,
                        STRIKE_TICKS,
                        f32::from(real),
                        Some(real),
                    )
                });
            }
        }
    }

    fn act(&self, s: &Match, me: PlayerId, c: &Choice, elapsed: u64, aim: [f64; 2]) -> ActionT {
        let noise = frame(me, Vec2::new(aim[0], aim[1]));
        let lateral = frame(me, Vec2::new(0.0, aim[1] * AIM_SCALE));
        let k = (c.param[0].max(0.0) as usize).min(AIM_Y.len() - 1);
        let waiting = Self::slow(c) && elapsed < u64::from(SLOW_TICKS);
        let real = self.corner_aim(me, k as u8) + lateral;
        let target = match c.id {
            Self::CHASE => s.world.puck.pos + noise * CHASE_SCALE,
            Self::DEFEND => self.defence_spot(s, me),
            id if Self::DEFENDS.contains(&id) => {
                Self::guard(self.own_corner(me, k as u8), s.world.puck.pos)
            }
            id if (Self::FAKES.contains(&id) || id == Self::FAKE_BANK) && elapsed < SHOW_TICKS => {
                self.show_point(s, me, c.shown.unwrap_or(1))
            }
            id if Self::FAKES.contains(&id) || Self::DASHES.contains(&id) => {
                self.contact(s, me, real)
            }
            Self::FAKE_BANK => self.contact(s, me, self.bank_aim(s, me) + lateral),
            Self::BANK if waiting => self.set_up(s, me, self.bank_aim(s, me)),
            Self::BANK => self.contact(s, me, self.bank_aim(s, me) + lateral),
            Self::HOLD => self.set_up(s, me, self.corner_aim(me, 1)),
            _ if waiting => self.set_up(s, me, real),
            _ => self.strike_target(s, me, real),
        };
        self.go_to(me, target)
    }

    fn opponent_move(&self, s: &Match, them: PlayerId, stance: Stance) -> ActionT {
        let mine = Self::puck_in_half_of(s, them);
        let target = match stance {
            Stance::Cover(class) if !mine => self.cover_spot(s, them, class),
            Stance::Shoot(class) if mine => {
                self.strike_target(s, them, self.corner_aim(them, class))
            }
            _ if mine => self.defence_spot(s, them),
            _ => s.world.mallets[usize::from(them.min(1))].pos,
        };
        self.go_to(them, target)
    }

    fn evaluate(&self, s: &Match, me: PlayerId) -> f64 {
        let me_i = usize::from(me.min(1));
        let score = s.score();
        let goals = f64::from(score[me_i]) - f64::from(score[1 - me_i]);
        let towards = attack(me);
        let puck = s.world.puck;
        let length = self.params.half_length;
        let progress = 0.3 * towards * puck.pos.x / length;
        let speed = towards * puck.vel.x / self.params.puck_speed_cap;
        let closeness_to_own_goal = 1.0 - (towards * puck.pos.x + length) / (2.0 * length);
        let danger = if speed < 0.0 {
            -speed * 0.5 * closeness_to_own_goal
        } else {
            0.0
        };
        10.0 * goals + progress + 0.3 * speed - danger
    }

    fn margin(&self) -> f64 {
        0.1
    }

    fn read(&self, before: &Match, after: &Match, them: PlayerId) -> Read {
        self.read_states(before, after, them)
    }

    fn importance(&self, state: &Match, _me: PlayerId) -> f64 {
        self.match_importance(state)
    }

    fn possession(&self, state: &Match, me: PlayerId) -> Option<u64> {
        let [a, b] = state.score();
        Self::puck_in_half_of(state, me).then_some(u64::from(a) + u64::from(b))
    }
}
