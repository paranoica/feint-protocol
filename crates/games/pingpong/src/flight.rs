use crate::rules::{Phase, Table};

const LOW: f64 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intercept {
    pub at: [f64; 2],
    pub height: f64,
    pub ticks: u32,
}

#[must_use]
pub fn coming_to(t: &Table, receiver: u8) -> bool {
    t.phase == Phase::Rally && t.hitter == Some(1 - receiver)
}

#[must_use]
pub fn intercept(t: &Table, receiver: u8) -> Option<Intercept> {
    if !coming_to(t, receiver) {
        return None;
    }
    let p = &t.params;
    let dt = p.dt();
    let side = if receiver == 0 { -1.0 } else { 1.0 };
    let near = p.half_length - 0.5 * p.over_table;
    let far = p.half_length + p.back_room - p.reach;
    let mut pos = t.ball.pos;
    let mut vel = t.ball.vel;
    let mut bounces = t.bounces;
    let mut best = None;
    for tick in 0..2 * p.tick_hz {
        vel[2] -= p.gravity * dt;
        let next = [
            pos[0] + vel[0] * dt,
            pos[1] + vel[1] * dt,
            pos[2] + vel[2] * dt,
        ];
        if (pos[2] > 0.0 && next[2] <= 0.0) || (pos[2] == 0.0 && next[2] < 0.0) {
            let on_table = next[0].abs() <= p.half_length && next[1].abs() <= p.half_width;
            if bounces == 0 && on_table && side * next[0] > 0.0 {
                bounces = 1;
                pos = [next[0], next[1], 0.0];
                vel[2] = -vel[2] * p.bounce;
                continue;
            }
            return best;
        }
        pos = next;
        let x = side * pos[0];
        if bounces == 1 && x >= near && x <= far && pos[2] <= p.hit_top && pos[2] >= LOW {
            best = Some(Intercept {
                at: [pos[0], pos[1]],
                height: pos[2],
                ticks: tick + 1,
            });
        }
        if x > far {
            return best;
        }
    }
    best
}

#[must_use]
pub fn good(t: &Table) -> Option<bool> {
    let hitter = t
        .hitter
        .filter(|_| t.phase == Phase::Rally && t.bounces == 0)?;
    let p = &t.params;
    let dt = p.dt();
    let side = if hitter == 0 { 1.0 } else { -1.0 };
    let mut pos = t.ball.pos;
    let mut vel = t.ball.vel;
    for _ in 0..2 * p.tick_hz {
        vel[2] -= p.gravity * dt;
        let next = [
            pos[0] + vel[0] * dt,
            pos[1] + vel[1] * dt,
            pos[2] + vel[2] * dt,
        ];
        if pos[0] * next[0] < 0.0 {
            let k = pos[0] / (pos[0] - next[0]);
            let z = pos[2] + (next[2] - pos[2]) * k;
            let y = pos[1] + (next[1] - pos[1]) * k;
            if z < p.net_height && z >= 0.0 && y.abs() <= p.half_width + p.net_height {
                return Some(false);
            }
        }
        if (pos[2] > 0.0 && next[2] <= 0.0) || (pos[2] == 0.0 && next[2] < 0.0) {
            let on = next[0].abs() <= p.half_length && next[1].abs() <= p.half_width;
            return Some(on && side * next[0] > 0.0);
        }
        pos = next;
    }
    None
}

#[must_use]
pub fn landing(t: &Table) -> Option<[f64; 2]> {
    if t.phase != Phase::Rally || t.bounces != 0 {
        return None;
    }
    let p = &t.params;
    let dt = p.dt();
    let mut pos = t.ball.pos;
    let mut vel = t.ball.vel;
    for _ in 0..2 * p.tick_hz {
        vel[2] -= p.gravity * dt;
        let next = [
            pos[0] + vel[0] * dt,
            pos[1] + vel[1] * dt,
            pos[2] + vel[2] * dt,
        ];
        if (pos[2] > 0.0 && next[2] <= 0.0) || (pos[2] == 0.0 && next[2] < 0.0) {
            return Some([next[0], next[1]]);
        }
        pos = next;
    }
    None
}
