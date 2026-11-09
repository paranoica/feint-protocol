use crate::mallet::{Mallet, Region, Side};
use crate::params::{Body, Params};
use crate::puck::Puck;
use crate::vec2::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct World {
    pub puck: Puck,
    pub mallets: [Mallet; 2],
}

impl World {
    #[must_use]
    pub fn new(params: &Params) -> Self {
        let x = params.half_length * 0.8;
        Self {
            puck: Puck::at_rest(Vec2::ZERO),
            mallets: [
                Mallet::at_rest(Vec2::new(-x, 0.0)),
                Mallet::at_rest(Vec2::new(x, 0.0)),
            ],
        }
    }

    pub fn hold(&mut self, params: &Params, bodies: &[Body; 2]) {
        let dt = params.substep_dt();
        let regions = regions(params);
        let reach = params.puck_radius + params.mallet_radius;
        for _ in 0..params.substeps {
            for (((mallet, region), body), side) in
                self.mallets.iter_mut().zip(&regions).zip(bodies).zip(SIDES)
            {
                mallet.step(region, body, dt);
                mallet.push_off(self.puck.pos, reach, -away_from_goal(side), region);
            }
        }
    }

    pub fn tick(&mut self, params: &Params, bodies: &[Body; 2]) -> Option<Side> {
        let dt = params.substep_dt();
        let regions = regions(params);
        let reach = params.puck_radius + params.mallet_radius;
        for _ in 0..params.substeps {
            for ((mallet, region), body) in self.mallets.iter_mut().zip(&regions).zip(bodies) {
                mallet.step(region, body, dt);
            }
            let old = self.puck.pos;
            self.puck.pos += self.puck.vel * dt;
            for (mallet, side) in self.mallets.iter().zip(SIDES) {
                puck_mallet(&mut self.puck, mallet, side, params);
            }
            self.puck.hit_posts(params);
            if let Some(scorer) = goal_crossing(old, self.puck.pos, params) {
                return Some(scorer);
            }
            self.puck.hit_rails(params);
            for ((mallet, region), side) in self.mallets.iter_mut().zip(&regions).zip(SIDES) {
                mallet.push_off(self.puck.pos, reach, -away_from_goal(side), region);
            }
            for mallet in &self.mallets {
                slide_off(&mut self.puck, mallet, params);
            }
            self.puck.vel = self.puck.vel.clamp_length(params.puck_speed_cap);
        }
        self.puck.vel = self.puck.vel * (1.0 - params.damping * params.tick_dt());
        None
    }
}

#[must_use]
pub fn goal_crossing(old: Vec2, new: Vec2, params: &Params) -> Option<Side> {
    let end = params.half_length;
    let (scorer, line) = if old.x < end && new.x >= end {
        (Side::Low, end)
    } else if old.x > -end && new.x <= -end {
        (Side::High, -end)
    } else {
        return None;
    };
    let t = (line - old.x) / (new.x - old.x);
    let y = old.y + t * (new.y - old.y);
    (y.abs() < params.mouth_half_width).then_some(scorer)
}

pub fn puck_mallet(puck: &mut Puck, mallet: &Mallet, owner: Side, params: &Params) {
    let reach = params.puck_radius + params.mallet_radius;
    let offset = puck.pos - mallet.pos;
    let distance = offset.length();
    if distance >= reach {
        return;
    }
    let normal = if distance > 0.0 {
        offset * (1.0 / distance)
    } else {
        away_from_goal(owner)
    };
    puck.pos = mallet.pos + normal * reach;
    let closing = (puck.vel - mallet.vel).dot(normal);
    if closing < 0.0 {
        puck.vel -= normal * ((1.0 + params.mallet_restitution) * closing);
    }
}

fn slide_off(puck: &mut Puck, mallet: &Mallet, params: &Params) {
    const SLACK: f64 = 1e-12;
    let reach = params.puck_radius + params.mallet_radius;
    let offset = puck.pos - mallet.pos;
    let distance = offset.length();
    if distance >= reach - SLACK {
        return;
    }
    let normal = if distance > 0.0 {
        offset * (1.0 / distance)
    } else {
        Vec2::new(0.0, if puck.pos.y > 0.0 { -1.0 } else { 1.0 })
    };
    let y_max = params.half_width - params.puck_radius;
    let x_max = params.half_length - params.puck_radius;
    let mut pos = mallet.pos + normal * reach;
    if pos.y.abs() > y_max {
        pos.y = pos.y.signum() * y_max;
        let dy = pos.y - mallet.pos.y;
        let dx = (reach * reach - dy * dy).max(0.0).sqrt();
        pos.x = mallet.pos.x + if offset.x >= 0.0 { dx } else { -dx };
    }
    let x_limit = if pos.y.abs() < params.mouth_half_width {
        params.half_length.next_down()
    } else {
        x_max
    };
    puck.pos = Vec2::new(pos.x.clamp(-x_limit, x_limit), pos.y);
    let offset = puck.pos - mallet.pos;
    let gap = offset.length();
    if gap > 0.0 {
        let normal = offset * (1.0 / gap);
        let closing = (puck.vel - mallet.vel).dot(normal);
        if closing < 0.0 {
            puck.vel -= normal * closing;
        }
    }
    if puck.pos.y.abs() >= y_max && puck.vel.y * puck.pos.y > 0.0 {
        puck.vel.y = 0.0;
    }
}

pub(crate) const SIDES: [Side; 2] = [Side::Low, Side::High];

pub(crate) fn away_from_goal(side: Side) -> Vec2 {
    match side {
        Side::Low => Vec2::new(1.0, 0.0),
        Side::High => Vec2::new(-1.0, 0.0),
    }
}

#[must_use]
pub fn regions(params: &Params) -> [Region; 2] {
    [
        Region::of(Side::Low, params),
        Region::of(Side::High, params),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(params: &Params) -> f64 {
        params.puck_radius + params.mallet_radius
    }

    #[test]
    fn rounding_level_touch_leaves_a_puck_in_the_mouth_alone() {
        let params = Params::default();
        let mallet = Mallet::at_rest(Vec2::new(-0.8728, 0.0965));
        let dir = Vec2::new(-0.09, -0.004) * (1.0 / Vec2::new(-0.09, -0.004).length());
        let puck_pos = mallet.pos + dir * (reach(&params) - 1e-15);
        let mut puck = Puck {
            pos: puck_pos,
            vel: Vec2::new(-3.0, 0.0),
        };
        assert!(
            puck.pos.x < -(params.half_length - params.puck_radius),
            "set-up: in the mouth"
        );
        assert!(
            (puck.pos - mallet.pos).length() < reach(&params),
            "set-up: a real overlap"
        );
        let before = puck;
        slide_off(&mut puck, &mallet, &params);
        assert_eq!(puck, before);
    }

    #[test]
    fn real_overlap_in_the_mouth_is_resolved_without_pulling_the_puck_out() {
        let params = Params::default();
        let mallet = Mallet::at_rest(Vec2::new(-0.989 + reach(&params) - 0.006, 0.0));
        let mut puck = Puck {
            pos: Vec2::new(-0.989, 0.0),
            vel: Vec2::new(-2.0, 0.0),
        };
        slide_off(&mut puck, &mallet, &params);
        assert!(
            (puck.pos - mallet.pos).length() >= reach(&params) - 1e-12,
            "{puck:?}"
        );
        assert!(puck.pos.x < -0.96, "pulled out of the mouth: {puck:?}");
    }

    #[test]
    fn velocity_correction_uses_a_unit_normal_after_the_rail_clamp() {
        let params = Params::default();
        let mallet = Mallet::at_rest(Vec2::new(0.9, 0.42));
        let mut puck = Puck {
            pos: Vec2::new(0.96, 0.46),
            vel: Vec2::new(-1.0, -1.0),
        };
        slide_off(&mut puck, &mallet, &params);
        let offset = puck.pos - mallet.pos;
        let normal = offset * (1.0 / offset.length());
        assert!(puck.vel.dot(normal) >= -1e-12, "still closing: {puck:?}");
    }
}
