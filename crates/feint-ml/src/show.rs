use airhockey::{Params, Vec2};
use feint_protocol::PlayerId;

pub const SHOW_MIN: f64 = 0.15;
pub const SHOW_HOLD_TICKS: usize = 30;
pub const SHOW_REACH: f64 = 0.5;
pub const SHOW_BACK: f64 = 0.1;
pub const BEHIND_COS: f64 = 0.5;
pub const CORNER_SPLIT: f64 = 0.035;

#[must_use]
pub fn seen(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

#[must_use]
pub fn class(y: f64) -> u8 {
    if y < -CORNER_SPLIT {
        0
    } else if y > CORNER_SPLIT {
        2
    } else {
        1
    }
}

#[must_use]
pub fn shown_side(params: &Params, puck: Vec2, mallet: Vec2, player: PlayerId) -> Option<u8> {
    let puck = seen(player, puck);
    let line = puck - seen(player, mallet);
    let length = line.length();
    let back = params.puck_radius + params.mallet_radius + SHOW_BACK;
    if length < back || length > SHOW_REACH || line.x < BEHIND_COS * length {
        return None;
    }
    let y = puck.y + (params.half_length - puck.x) * line.y / line.x;
    let limit = params.half_width - params.puck_radius;
    if y.abs() > limit || y.abs() < SHOW_MIN {
        return None;
    }
    Some(class(y))
}
