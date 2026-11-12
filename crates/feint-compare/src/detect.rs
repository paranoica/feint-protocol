use airhockey::analysis::Frame;
use airhockey::{Params, Vec2};
use feint_protocol::PlayerId;

pub const CONTACT_SLACK: f64 = 0.01;
pub const SHOT_SPEED: f64 = 1.4;
pub const SHOW_MIN: f64 = 0.15;
pub const SHOW_HOLD_TICKS: usize = 30;
pub const SHOW_REACH: f64 = 0.5;
pub const SHOW_BACK: f64 = 0.1;
pub const SHOW_WINDOW_TICKS: u64 = 360;
pub const BEHIND_COS: f64 = 0.5;
pub const CORNER_SPLIT: f64 = 0.035;
pub const SEPARATION_TICKS: usize = 24;
pub const READ_TICKS: usize = 12;
pub const MIN_READ_TICKS: usize = 2;
pub const SCORE_WINDOW_TICKS: u64 = 240;
pub const RETURN_SLACK: f64 = 0.02;
const MAX_BOUNCES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shot {
    pub player: PlayerId,
    pub from: u64,
    pub contact: u64,
    pub shown: Option<u8>,
    pub realised: u8,
    pub scored: bool,
}

impl Shot {
    #[must_use]
    pub fn fake(&self) -> bool {
        self.shown.is_some_and(|s| s != self.realised)
    }
}

fn seen(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

fn class(y: f64) -> u8 {
    if y < -CORNER_SPLIT {
        0
    } else if y > CORNER_SPLIT {
        2
    } else {
        1
    }
}

fn goal_y(params: &Params, from: Vec2, dir: Vec2) -> Option<f64> {
    if dir.x <= 0.0 {
        return None;
    }
    let limit = params.half_width - params.puck_radius;
    let mut left = (params.half_length - from.x) / dir.x;
    let (mut y, mut vy) = (from.y, dir.y);
    for _ in 0..MAX_BOUNCES {
        let wall = if vy > 0.0 { limit } else { -limit };
        if vy == 0.0 || (wall - y) / vy >= left {
            return Some((y + vy * left).clamp(-limit, limit));
        }
        left -= (wall - y) / vy;
        y = wall;
        vy = -vy * params.rail_restitution;
    }
    None
}

fn shown_side(params: &Params, frame: &Frame, player: PlayerId) -> Option<u8> {
    let me = usize::from(player);
    let puck = seen(player, frame.puck);
    let line = puck - seen(player, frame.mallets[me]);
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

fn touch_starts(frames: &[Frame], reach: f64) -> Vec<[bool; 2]> {
    let mut was = [false; 2];
    frames
        .iter()
        .map(|f| {
            let now = [0, 1].map(|i| (f.puck - f.mallets[i]).length() <= reach);
            let starts = [now[0] && !was[0], now[1] && !was[1]];
            was = now;
            starts
        })
        .collect()
}

fn shown(
    params: &Params,
    frames: &[Frame],
    since: usize,
    contact: usize,
    player: PlayerId,
    in_play: u64,
) -> Option<u8> {
    let earliest = frames[contact]
        .tick
        .saturating_sub(SHOW_WINDOW_TICKS)
        .max(in_play);
    let mut best: Option<(u8, usize)> = None;
    let mut run: Option<(u8, usize)> = None;
    for f in frames[since..contact].iter().filter(|f| f.tick >= earliest) {
        run = match (shown_side(params, f, player), run) {
            (Some(side), Some((s, n))) if s == side => Some((s, n + 1)),
            (Some(side), _) => Some((side, 1)),
            (None, _) => None,
        };
        if let Some((s, n)) = run
            && n >= SHOW_HOLD_TICKS
            && best.is_none_or(|(_, b)| n > b)
        {
            best = Some((s, n));
        }
    }
    best.map(|(s, _)| s)
}

fn realised(
    params: &Params,
    frames: &[Frame],
    starts: &[[bool; 2]],
    contact: usize,
    player: PlayerId,
    reach: f64,
) -> Option<u8> {
    let me = usize::from(player);
    let apart = (contact + 1..frames.len().min(contact + 1 + SEPARATION_TICKS))
        .find(|&k| (frames[k].puck - frames[k].mallets[me]).length() > reach)?;
    let jump = params.puck_speed_cap * params.tick_dt() * 2.0;
    let mut last = apart;
    for k in apart + 1..frames.len().min(apart + 1 + READ_TICKS) {
        let step = (frames[k].puck - frames[k - 1].puck).length();
        if frames[k].score != frames[apart].score || step > jump || starts[k][0] || starts[k][1] {
            break;
        }
        last = k;
    }
    if last < apart + MIN_READ_TICKS {
        return None;
    }
    let from = seen(player, frames[apart].puck);
    let dir = seen(player, frames[last].puck) - from;
    let seconds = (frames[last].tick - frames[apart].tick) as f64 * params.tick_dt();
    if dir.x / seconds < SHOT_SPEED {
        return None;
    }
    goal_y(params, from, dir).map(class)
}

fn credit(frames: &[Frame], starts: &[[bool; 2]], out: &mut [Shot], player: PlayerId) {
    let me = usize::from(player);
    let them = 1 - me;
    for k in 1..frames.len() {
        if frames[k].score[me] <= frames[k - 1].score[me] {
            continue;
        }
        let goal = frames[k].tick;
        let lo = frames.partition_point(|f| f.tick < goal.saturating_sub(SCORE_WINDOW_TICKS));
        let Some(last) = (lo..k).rev().find(|&j| starts[j][me]) else {
            continue;
        };
        if starts[last][them] {
            continue;
        }
        let mut furthest = f64::NEG_INFINITY;
        let turned = frames[last..k].iter().any(|f| {
            let x = seen(player, f.puck).x;
            furthest = furthest.max(x);
            x < furthest - RETURN_SLACK
        });
        if turned {
            continue;
        }
        let touched = frames[last].tick;
        if let Some(shot) = out
            .iter_mut()
            .find(|s| s.player == player && s.contact == touched)
        {
            shot.scored = true;
        }
    }
}

#[must_use]
pub fn shots(params: &Params, frames: &[Frame]) -> Vec<Shot> {
    let reach = params.puck_radius + params.mallet_radius + CONTACT_SLACK;
    let starts = touch_starts(frames, reach);
    let mut out = Vec::new();
    let mut since = [0usize; 2];
    let mut in_play = 0;
    let jump = params.puck_speed_cap * params.tick_dt() * 2.0;
    for k in 0..frames.len() {
        if k > 0
            && (frames[k].score != frames[k - 1].score
                || (frames[k].puck - frames[k - 1].puck).length() > jump)
        {
            since = [k, k];
            in_play = frames[k].tick + u64::from(params.serve_pause_ticks());
        }
        for player in 0..2u8 {
            let me = usize::from(player);
            if !starts[k][me] {
                continue;
            }
            since[1 - me] = k;
            if let Some(side) = realised(params, frames, &starts, k, player, reach) {
                out.push(Shot {
                    player,
                    from: frames[since[me]].tick,
                    contact: frames[k].tick,
                    shown: shown(params, frames, since[me], k, player, in_play),
                    realised: side,
                    scored: false,
                });
                since[me] = k;
            }
        }
    }
    for player in 0..2u8 {
        credit(frames, &starts, &mut out, player);
    }
    out
}
