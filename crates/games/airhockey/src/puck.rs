use crate::params::Params;
use crate::vec2::Vec2;

/// The puck: a circle sliding on the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puck {
    pub pos: Vec2,
    pub vel: Vec2,
}

impl Puck {
    #[must_use]
    pub fn at_rest(pos: Vec2) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
        }
    }

    /// Bounce off the posts: points at the four mouth edges.
    pub fn hit_posts(&mut self, params: &Params) {
        let r = params.puck_radius;
        for end in [-1.0, 1.0] {
            for edge in [-1.0, 1.0] {
                let post = Vec2::new(end * params.half_length, edge * params.mouth_half_width);
                let offset = self.pos - post;
                let distance = offset.length();
                if distance >= r {
                    continue;
                }
                let normal = if distance > 0.0 {
                    offset * (1.0 / distance)
                } else {
                    Vec2::new(-end, 0.0)
                };
                self.pos = post + normal * r;
                self.bounce(normal, params.post_restitution);
            }
        }
    }

    /// Bounce off the side rails, and off the end rails outside the mouth.
    pub fn hit_rails(&mut self, params: &Params) {
        let r = params.puck_radius;
        let y_max = params.half_width - r;
        if self.pos.y.abs() > y_max {
            let side = self.pos.y.signum();
            self.pos.y = side * y_max;
            self.bounce(Vec2::new(0.0, -side), params.rail_restitution);
        }
        let x_max = params.half_length - r;
        if self.pos.x.abs() > x_max && self.pos.y.abs() >= params.mouth_half_width {
            let end = self.pos.x.signum();
            self.pos.x = end * x_max;
            self.bounce(Vec2::new(-end, 0.0), params.rail_restitution);
        }
    }

    /// Reflect the velocity component going against `normal`, scaled by `restitution`.
    fn bounce(&mut self, normal: Vec2, restitution: f64) {
        let into = self.vel.dot(normal);
        if into < 0.0 {
            self.vel -= normal * ((1.0 + restitution) * into);
        }
    }
}
