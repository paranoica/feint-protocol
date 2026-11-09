use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::AirHockey;
use crate::game::Match;
use crate::params::Params;
use crate::vec2::Vec2;

mod geometry;
mod reads;

use geometry::{AIM_SCALE, AIM_Y, CHASE_SCALE, attack, frame};

pub struct HockeyPlaybook {
    params: Params,
}

impl HockeyPlaybook {
    pub const CHASE: u16 = 0;
    pub const DEFEND: u16 = 1;
    pub const STRIKES: [u16; 3] = [2, 3, 4];
    pub const DEFENDS: [u16; 3] = [5, 6, 7];

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    #[must_use]
    pub fn contact_target(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        self.contact(state, player, aim)
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
        _cue: &Cue,
        out: &mut Vec<Choice>,
    ) {
        out.clear();
        let choice = |id: u16, intent: u8, horizon_ticks: u32, k: f32, corner: Option<u8>| Choice {
            id,
            intent,
            deceptive: false,
            horizon_ticks,
            param: [k, 0.0],
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
        if Self::puck_in_half_of(state, me) {
            for (k, &id) in Self::STRIKES.iter().enumerate() {
                out.push(choice(id, 1, 240, k as f32, Some(k as u8)));
            }
        }
    }

    fn act(&self, s: &Match, me: PlayerId, c: &Choice, _elapsed: u64, aim: [f64; 2]) -> ActionT {
        let noise = frame(me, Vec2::new(aim[0], aim[1]));
        let k = (c.param[0].max(0.0) as usize).min(AIM_Y.len() - 1);
        let target = match c.id {
            Self::CHASE => s.world.puck.pos + noise * CHASE_SCALE,
            Self::DEFEND => self.defence_spot(s, me),
            id if Self::DEFENDS.contains(&id) => {
                Self::guard(self.own_corner(me, k as u8), s.world.puck.pos)
            }
            _ => {
                let seen = Vec2::new(self.params.half_length, AIM_Y[k] + aim[1] * AIM_SCALE);
                self.strike_target(s, me, frame(me, seen))
            }
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
        Self::puck_in_half_of(state, me).then_some(0)
    }
}
