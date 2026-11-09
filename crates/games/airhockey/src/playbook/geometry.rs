use feint_protocol::{ActionT, PlayerId};

use super::HockeyPlaybook;
use crate::game::Match;
use crate::vec2::Vec2;

pub(super) const AIM_SHARE: [f64; 3] = [-0.56, 0.0, 0.56];
pub(super) const RUN_UP: f64 = 0.1;
pub(super) const LINE_TOLERANCE: f64 = 0.01;
pub(super) const DEFENCE_DEPTH: f64 = 0.15;
pub(super) const AIM_SCALE: f64 = 2.0;
pub(super) const CHASE_SCALE: f64 = 0.5;
pub(super) const DEFENCE_SCALE: f64 = 2.0;
pub(super) const CONTACT_DRIVE: f64 = 0.15;
pub(super) const MIN_DRIVE: f64 = 0.005;
pub(super) const MIN_NUDGE: f64 = 0.0005;
pub(super) const NUDGE_SPEED: f64 = 0.9;
pub(super) const NUDGE_STILL: f64 = 0.3;
pub(super) const FIRE_X: f64 = -0.3;
pub(super) const FAKE_LANE: f64 = 0.25;
pub(super) const FAKE_SPEED: f64 = 2.5;
pub(super) const APPROACH_COS: f64 = 0.5;
pub(super) const SIDE_SPEED: f64 = 0.3;
pub(super) const SHOW_WIDE: f64 = 0.3;
pub(super) const WINDUP: f64 = 0.2;
pub(super) const PUMP_GAP: f64 = 0.02;
pub(super) const PUMP_TICKS: u64 = 36;
pub(super) const SHOW_TICKS: u64 = 96;
pub(super) const CONTROL_SPEED: f64 = 1.0;
pub(super) const SLOW_TICKS: u32 = 120;
pub(super) const STRIKE_TICKS: u32 = 240;
pub(super) const HOLD_TICKS: u32 = 24;
pub(super) const HOLD_RESERVE_SECONDS: f64 = 3.0;
pub(super) const MIN_BANK_RESTITUTION: f64 = 0.1;

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
            puck + line * (self.drive(state, player) - reach)
        } else {
            self.set_up(state, player, aim)
        }
    }

    pub(super) fn drive(&self, state: &Match, player: PlayerId) -> f64 {
        self.drive_after(state, player, 0.0)
    }

    fn drive_after(&self, state: &Match, player: PlayerId, away: f64) -> f64 {
        let body = state.bodies[usize::from(player.min(1))];
        let power = (body.max_speed() / self.params.max_body_speed).clamp(0.0, 1.0);
        let contact =
            power * self.params.puck_speed_cap / (1.0 + self.params.mallet_restitution) + away;
        (contact * contact / (2.0 * body.max_accel())).clamp(MIN_DRIVE, CONTACT_DRIVE)
    }

    pub(super) fn set_up(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        let puck = state.world.puck.pos;
        let line = unit(aim - puck, Vec2::new(attack(player), 0.0));
        puck - line * (self.params.puck_radius + self.params.mallet_radius + RUN_UP)
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
        let k = usize::from(class).min(AIM_SHARE.len() - 1);
        let y = AIM_SHARE[k] * self.params.mouth_half_width;
        frame(attacker, Vec2::new(self.params.half_length, y))
    }

    pub(super) fn own_corner(&self, defender: PlayerId, class: u8) -> Vec2 {
        let k = usize::from(class).min(AIM_SHARE.len() - 1);
        let y = AIM_SHARE[k] * self.params.mouth_half_width;
        frame(defender, Vec2::new(-self.params.half_length, y))
    }

    pub(super) fn guard(corner: Vec2, puck: Vec2) -> Vec2 {
        corner + unit(puck - corner, Vec2::ZERO) * DEFENCE_DEPTH
    }

    pub(super) fn soft_drive(&self, state: &Match, player: PlayerId) -> f64 {
        let body = state.bodies[usize::from(player.min(1))];
        let contact = NUDGE_SPEED / (1.0 + self.params.mallet_restitution);
        (contact * contact / (2.0 * body.max_accel())).clamp(MIN_NUDGE, MIN_DRIVE)
    }

    pub(super) fn fake_target(
        &self,
        state: &Match,
        player: PlayerId,
        shown: u8,
        real: Vec2,
        elapsed: u64,
    ) -> Vec2 {
        let puck = state.world.puck;
        let forward = attack(player) * puck.pos.x;
        if forward < FIRE_X && puck.vel.length() < NUDGE_STILL {
            let ahead = Vec2::new(attack(player) * self.params.half_length, puck.pos.y);
            return self.contact_with(state, player, ahead, self.soft_drive(state, player));
        }
        if elapsed < SHOW_TICKS {
            return self.show_point(state, player, shown);
        }
        if elapsed < SHOW_TICKS + PUMP_TICKS || forward < FIRE_X {
            return self.pump_point(state, player, shown);
        }
        let away = (attack(player) * puck.vel.x).clamp(0.0, NUDGE_SPEED);
        self.contact_with(state, player, real, self.drive_after(state, player, away))
    }

    pub(super) fn contact(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        self.contact_with(state, player, aim, self.drive(state, player))
    }

    fn contact_with(&self, state: &Match, player: PlayerId, aim: Vec2, drive: f64) -> Vec2 {
        let puck = state.world.puck.pos;
        let body = state.world.mallets[usize::from(player.min(1))];
        let mallet = body.pos;
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let line = unit(aim - puck, Vec2::new(attack(player), 0.0));
        let touch = puck - line * reach;
        let approach = unit(touch - mallet, line);
        let drift = (body.vel.x * approach.y - body.vel.y * approach.x).abs();
        if approach.dot(line) > APPROACH_COS
            && (mallet - puck).dot(line) < 0.0
            && drift < SIDE_SPEED
        {
            touch + approach * drive
        } else {
            self.set_up(state, player, aim)
        }
    }
}
