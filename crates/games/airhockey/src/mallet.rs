use std::f64::consts::FRAC_1_SQRT_2;

use crate::params::{Body, Params};
use crate::vec2::Vec2;

/// Which end a player defends: `Low` defends x = -half_length, `High` defends +half_length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Low,
    High,
}

/// The box a mallet's centre must stay in: its own half, inside the rails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub min: Vec2,
    pub max: Vec2,
}

impl Region {
    #[must_use]
    pub fn of(side: Side, params: &Params) -> Self {
        let r = params.mallet_radius;
        let (x_min, x_max) = match side {
            Side::Low => (-params.half_length + r, -r),
            Side::High => (r, params.half_length - r),
        };
        let y_max = params.half_width - r;
        Self {
            min: Vec2::new(x_min, -y_max),
            max: Vec2::new(x_max, y_max),
        }
    }

    /// The nearest point of the region.
    #[must_use]
    pub fn clamp(&self, point: Vec2) -> Vec2 {
        Vec2::new(
            point.x.clamp(self.min.x, self.max.x),
            point.y.clamp(self.min.y, self.max.y),
        )
    }
}

/// A mallet driven towards a target point under its owner's body limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mallet {
    pub pos: Vec2,
    pub vel: Vec2,
    pub target: Vec2,
}

impl Mallet {
    /// At rest at `pos`, aiming at where it stands.
    #[must_use]
    pub fn at_rest(pos: Vec2) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
            target: pos,
        }
    }

    /// Advance one substep of `dt` seconds.
    ///
    /// Speed never exceeds the body's limit and the velocity changes by at most
    /// `max_accel·dt` per substep. Near the target and near the region's edges the speed
    /// is capped by the discrete braking profile, so the mallet stops on the target and
    /// never has to stop harder than it can.
    pub fn step(&mut self, region: &Region, body: &Body, dt: f64) {
        if !self.target.is_finite() {
            self.target = self.pos;
        }
        let accel = body.max_accel();
        let dv_max = accel * dt;
        let target = region.clamp(self.target);
        let to_target = target - self.pos;
        let distance = to_target.length();

        if distance <= dv_max * dt && self.vel.length() <= dv_max {
            self.pos = target;
            self.vel = Vec2::ZERO;
            return;
        }

        let mut want = Vec2::ZERO;
        if distance > 0.0 {
            let speed = body.max_speed().min(braking_speed(distance, accel, dt));
            want = to_target * (speed / distance);
        }

        // Per axis with a/√2, so braking towards two edges at once stays within `accel`.
        // The braking correction comes first; only what is left of the budget turns the
        // mallet towards its target, so a sharp turn can never cost it the ability to stop.
        let edge_accel = accel * FRAC_1_SQRT_2;
        let lo = Vec2::new(
            -braking_speed(self.pos.x - region.min.x, edge_accel, dt),
            -braking_speed(self.pos.y - region.min.y, edge_accel, dt),
        );
        let hi = Vec2::new(
            braking_speed(region.max.x - self.pos.x, edge_accel, dt),
            braking_speed(region.max.y - self.pos.y, edge_accel, dt),
        );
        let forced = (self.vel.clamp_each(lo, hi) - self.vel).clamp_length(dv_max);
        let budget = (dv_max - forced.length()).max(0.0);
        let braked = self.vel + forced;
        self.vel = braked + (want.clamp_each(lo, hi) - braked).clamp_length(budget);
        self.pos += self.vel * dt;
        self.stop_at_edges(region);
    }

    /// Move out to `distance` from `from` (the puck pinned by a rail or the other mallet)
    /// and drop the velocity towards it. A contact force: it may exceed the body's
    /// acceleration, unlike the controller. `fallback` is the direction when the centres
    /// coincide.
    pub(crate) fn push_off(&mut self, from: Vec2, distance: f64, fallback: Vec2, region: &Region) {
        let offset = self.pos - from;
        let gap = offset.length();
        if gap >= distance {
            return;
        }
        let normal = if gap > 0.0 {
            offset * (1.0 / gap)
        } else {
            fallback
        };
        self.pos = from + normal * distance;
        let towards = self.vel.dot(normal);
        if towards < 0.0 {
            self.vel -= normal * towards;
        }
        self.stop_at_edges(region);
    }

    /// Only reached when the mallet was already moving faster than it could brake, such as
    /// right after being pushed by the puck: the edge stops it like a wall.
    fn stop_at_edges(&mut self, region: &Region) {
        if self.pos.x < region.min.x || self.pos.x > region.max.x {
            self.pos.x = self.pos.x.clamp(region.min.x, region.max.x);
            self.vel.x = 0.0;
        }
        if self.pos.y < region.min.y || self.pos.y > region.max.y {
            self.pos.y = self.pos.y.clamp(region.min.y, region.max.y);
            self.vel.y = 0.0;
        }
    }
}

/// The highest speed from which braking by `accel·dt` per step of `dt` stops within
/// `distance`, with semi-implicit Euler (speed first, then position).
///
/// Starting at `(k + r)·a·dt` with whole `k` and `r` in `[0, 1)`, the steps cover
/// `(k + 1)(k/2 + r)·a·dt²`; this inverts that sum. The continuous `sqrt(2·a·d)` and its
/// smooth discrete fit both overshoot on the last, fractional step.
fn braking_speed(distance: f64, accel: f64, dt: f64) -> f64 {
    if distance <= 0.0 {
        return 0.0;
    }
    let unit = accel * dt;
    let w = distance / (unit * dt);
    let k = (((8.0 * w + 1.0).sqrt() - 1.0) / 2.0).floor();
    let r = (w / (k + 1.0) - k / 2.0).clamp(0.0, 1.0);
    (k + r) * unit
}
