use feint_protocol::{ActionT, PlayerId};

use super::HockeyPlaybook;
use crate::game::Match;
use crate::vec2::Vec2;

pub(super) const AIM_Y: [f64; 3] = [-0.07, 0.0, 0.07];
pub(super) const RUN_UP: f64 = 0.1;
pub(super) const LINE_TOLERANCE: f64 = 0.01;
pub(super) const DEFENCE_DEPTH: f64 = 0.15;
pub(super) const AIM_SCALE: f64 = 2.0;
pub(super) const CHASE_SCALE: f64 = 0.5;
pub(super) const CONTACT_DRIVE: f64 = 0.15;
pub(super) const APPROACH_COS: f64 = 0.5;

pub(super) fn attack(player: PlayerId) -> f64 {
    if player == 0 { 1.0 } else { -1.0 }
}

pub(super) fn frame(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

pub(super) fn unit(v: Vec2, fallback: Vec2) -> Vec2 {
    let length = v.length();
    if length > 0.0 {
        v * (1.0 / length)
    } else {
        fallback
    }
}

impl HockeyPlaybook {
    pub(super) fn defence_spot(&self, state: &Match, player: PlayerId) -> Vec2 {
        let goal = Vec2::new(-attack(player) * self.params.half_length, 0.0);
        let towards = unit(state.world.puck.pos - goal, Vec2::new(attack(player), 0.0));
        goal + towards * DEFENCE_DEPTH
    }

    pub(super) fn strike_target(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
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

    pub(super) fn puck_in_half_of(state: &Match, player: PlayerId) -> bool {
        attack(player) * state.world.puck.pos.x < 0.0
    }

    pub(super) fn go_to(&self, player: PlayerId, world: Vec2) -> ActionT {
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

    pub(super) fn corner_aim(&self, attacker: PlayerId, class: u8) -> Vec2 {
        let k = usize::from(class).min(AIM_Y.len() - 1);
        frame(attacker, Vec2::new(self.params.half_length, AIM_Y[k]))
    }

    pub(super) fn own_corner(&self, defender: PlayerId, class: u8) -> Vec2 {
        let k = usize::from(class).min(AIM_Y.len() - 1);
        frame(defender, Vec2::new(-self.params.half_length, AIM_Y[k]))
    }

    pub(super) fn guard(corner: Vec2, puck: Vec2) -> Vec2 {
        corner + unit(puck - corner, Vec2::ZERO) * DEFENCE_DEPTH
    }

    pub(super) fn contact(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        let puck = state.world.puck.pos;
        let mallet = state.world.mallets[usize::from(player.min(1))].pos;
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let line = unit(aim - puck, Vec2::new(attack(player), 0.0));
        let touch = puck - line * reach;
        let approach = unit(touch - mallet, line);
        if approach.dot(line) > APPROACH_COS && (mallet - puck).dot(line) < 0.0 {
            touch + approach * CONTACT_DRIVE
        } else {
            puck - line * (reach + RUN_UP)
        }
    }
}
