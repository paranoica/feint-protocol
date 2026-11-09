use feint_ai::Read;
use feint_protocol::PlayerId;

use super::HockeyPlaybook;
use super::geometry::{AIM_Y, attack, frame};
use crate::game::Match;
use crate::vec2::Vec2;

const SHIFT: f64 = 0.03;
const COVER_REACH: f64 = 0.06;
const SHOT_SPEED: f64 = 2.0;
const CORNER_SPLIT: f64 = 0.035;
const MAX_BOUNCES: usize = 4;

impl HockeyPlaybook {
    fn covered_class(&self, state: &Match, defender: PlayerId) -> Option<u8> {
        let attacker = 1 - defender.min(1);
        let mallet = state.world.mallets[usize::from(defender.min(1))].pos;
        let puck = state.world.puck.pos;
        let mut best: Option<(u8, f64)> = None;
        for class in 0..AIM_Y.len() as u8 {
            let corner = self.corner_aim(attacker, class);
            let gap = distance_to_segment(mallet, corner, puck);
            if best.is_none_or(|(_, b)| gap < b) {
                best = Some((class, gap));
            }
        }
        best.filter(|&(_, gap)| gap < COVER_REACH)
            .map(|(class, _)| class)
    }

    fn shot_class(&self, before: &Match, after: &Match, shooter: PlayerId) -> Option<u8> {
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let mallet = before.world.mallets[usize::from(shooter.min(1))].pos;
        let near = (before.world.puck.pos - mallet).length() < reach + 0.03;
        let puck = after.world.puck;
        let towards = attack(shooter) * puck.vel.x;
        if !near || towards < SHOT_SPEED {
            return None;
        }
        let goal_x = attack(shooter) * self.params.half_length;
        let limit = self.params.half_width - self.params.puck_radius;
        let mut left = (goal_x - puck.pos.x) / puck.vel.x;
        let (mut y, mut vy) = (puck.pos.y, puck.vel.y);
        for _ in 0..MAX_BOUNCES {
            let wall = if vy > 0.0 { limit } else { -limit };
            if vy == 0.0 || (wall - y) / vy >= left {
                break;
            }
            left -= (wall - y) / vy;
            y = wall;
            vy = -vy * self.params.rail_restitution;
        }
        let y = (y + vy * left).clamp(-limit, limit);
        let crossing = frame(shooter, Vec2::new(goal_x, y));
        Some(if crossing.y < -CORNER_SPLIT {
            0
        } else if crossing.y > CORNER_SPLIT {
            2
        } else {
            1
        })
    }

    pub(super) fn read_states(&self, before: &Match, after: &Match, them: PlayerId) -> Read {
        let moved = (after.world.mallets[usize::from(them.min(1))].pos
            - before.world.mallets[usize::from(them.min(1))].pos)
            .length()
            > SHIFT;
        Read {
            covered: if moved {
                self.covered_class(after, them)
            } else {
                None
            },
            shot: self.shot_class(before, after, them),
        }
    }

    pub(super) fn match_importance(&self, state: &Match) -> f64 {
        let [a, b] = state.score();
        let win = self.params.goals_to_win;
        let near = |g: u32| g.saturating_add(1) >= win;
        let late_tie = a == b && a.saturating_add(2) >= win;
        if near(a) || near(b) || late_tie {
            1.0
        } else {
            0.0
        }
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let span = ab.dot(ab);
    let t = if span > 0.0 {
        ((p - a).dot(ab) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + ab * t)).length()
}
