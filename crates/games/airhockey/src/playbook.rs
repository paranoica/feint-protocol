use feint_ai::{Choice, Playbook};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::AirHockey;
use crate::game::Match;
use crate::params::Params;
use crate::vec2::Vec2;

const AIM_Y: [f64; 3] = [-0.07, 0.0, 0.07];
const RUN_UP: f64 = 0.1;
const LINE_TOLERANCE: f64 = 0.01;
const DEFENCE_DEPTH: f64 = 0.15;
const AIM_SCALE: f64 = 2.0;
const CHASE_SCALE: f64 = 0.5;

fn attack(player: PlayerId) -> f64 {
    if player == 0 { 1.0 } else { -1.0 }
}

fn frame(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

fn unit(v: Vec2, fallback: Vec2) -> Vec2 {
    let length = v.length();
    if length > 0.0 {
        v * (1.0 / length)
    } else {
        fallback
    }
}

pub struct HockeyPlaybook {
    params: Params,
}

impl HockeyPlaybook {
    pub const CHASE: u16 = 0;
    pub const DEFEND: u16 = 1;
    pub const STRIKES: [u16; 3] = [2, 3, 4];

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    fn defence_spot(&self, state: &Match, player: PlayerId) -> Vec2 {
        let goal = Vec2::new(-attack(player) * self.params.half_length, 0.0);
        let towards = unit(state.world.puck.pos - goal, Vec2::new(attack(player), 0.0));
        goal + towards * DEFENCE_DEPTH
    }

    fn strike_target(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        let puck = state.world.puck.pos;
        let mallet = state.world.mallets[usize::from(player.min(1))].pos;
        let line = unit(aim - puck, Vec2::new(attack(player), 0.0));
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let rel = mallet - puck;
        let along = rel.dot(line);
        let lateral = (rel.x * line.y - rel.y * line.x).abs();
        if along < -reach && lateral < LINE_TOLERANCE {
            puck + line * 0.15
        } else {
            puck - line * (reach + RUN_UP)
        }
    }

    fn puck_in_half_of(state: &Match, player: PlayerId) -> bool {
        attack(player) * state.world.puck.pos.x < 0.0
    }

    fn go_to(&self, player: PlayerId, world: Vec2) -> ActionT {
        let seen = frame(player, world);
        let (l, w) = (
            self.params.half_length as f32,
            self.params.half_width as f32,
        );
        let mut action = ActionT::default();
        action.player = player;
        action.values = Some(vec![
            (seen.x as f32).clamp(-l, l),
            (seen.y as f32).clamp(-w, w),
        ]);
        action
    }
}

impl Playbook<AirHockey> for HockeyPlaybook {
    fn options(&self, state: &Match, me: PlayerId, repertoire: u8, out: &mut Vec<Choice>) {
        out.clear();
        let choice = |id: u16, intent: u8, horizon_ticks: u32, k: f32| Choice {
            id,
            intent,
            deceptive: false,
            horizon_ticks,
            param: [k, 0.0],
        };
        if repertoire == 0 {
            out.push(choice(Self::CHASE, 1, 60, 0.0));
            return;
        }
        out.push(choice(Self::DEFEND, 2, 120, 0.0));
        if Self::puck_in_half_of(state, me) {
            for (k, &id) in Self::STRIKES.iter().enumerate() {
                out.push(choice(id, 1, 240, k as f32));
            }
        }
    }

    fn act(&self, s: &Match, me: PlayerId, c: &Choice, _elapsed: u64, aim: [f64; 2]) -> ActionT {
        let noise = frame(me, Vec2::new(aim[0], aim[1]));
        let target = match c.id {
            Self::CHASE => s.world.puck.pos + noise * CHASE_SCALE,
            Self::DEFEND => self.defence_spot(s, me),
            _ => {
                let k = (c.param[0].max(0.0) as usize).min(AIM_Y.len() - 1);
                let seen = Vec2::new(self.params.half_length, AIM_Y[k] + aim[1] * AIM_SCALE);
                self.strike_target(s, me, frame(me, seen))
            }
        };
        self.go_to(me, target)
    }

    fn reply(&self, s: &Match, them: PlayerId) -> ActionT {
        let target = if Self::puck_in_half_of(s, them) {
            self.defence_spot(s, them)
        } else {
            s.world.mallets[usize::from(them.min(1))].pos
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
}
