use std::f64::consts::FRAC_1_SQRT_2;

use crate::params::{Body, Params};
use crate::vec2::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Low,
    High,
}

impl Side {
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High => Self::Low,
        }
    }

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }
}

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

    #[must_use]
    pub fn clamp(&self, point: Vec2) -> Vec2 {
        Vec2::new(
            point.x.clamp(self.min.x, self.max.x),
            point.y.clamp(self.min.y, self.max.y),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mallet {
    pub pos: Vec2,
    pub vel: Vec2,
    pub target: Vec2,
}

impl Mallet {
    #[must_use]
    pub fn at_rest(pos: Vec2) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
            target: pos,
        }
    }

    pub fn step(&mut self, region: &Region, body: &Body, dt: f64) {
        if !self.target.is_finite() {
            self.target = self.pos;
        }
        let accel = body.max_accel();
        let dv_max = accel * dt;
        let target = region.clamp(self.target);
        let to_target = target - self.pos;
        let distance = to_target.length();

        if distance <= dv_max.min(body.max_speed()) * dt && self.vel.length() <= dv_max {
            self.pos = target;
            self.vel = Vec2::ZERO;
            return;
        }

        let mut want = Vec2::ZERO;
        if distance > 0.0 {
            let speed = body.max_speed().min(braking_speed(distance, accel, dt));
            want = to_target * (speed / distance);
        }

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

fn braking_speed(distance: f64, accel: f64, dt: f64) -> f64 {
    if distance <= 0.0 {
        return 0.0;
    }
    let unit = accel * dt;
    let w = distance / (unit * dt);
    if !w.is_finite() {
        return 0.0;
    }
    let k = (((8.0 * w + 1.0).sqrt() - 1.0) / 2.0).floor();
    let r = (w / (k + 1.0) - k / 2.0).clamp(0.0, 1.0);
    (k + r) * unit
}
