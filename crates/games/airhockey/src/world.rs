use crate::mallet::{Mallet, Region, Side};
use crate::params::{Body, Params};
use crate::puck::Puck;
use crate::vec2::Vec2;

/// Everything that moves: the puck and both mallets (index 0 is `Side::Low`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct World {
    pub puck: Puck,
    pub mallets: [Mallet; 2],
}

impl World {
    /// Puck at rest in the centre, each mallet at rest in front of its goal.
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

    /// Advance one tick; returns the side that scored, if any. After a goal the rest of
    /// the tick is skipped and the world is left as it was at the crossing.
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
            // Rails win over mallets: a pinned puck stays put and the mallet gives way.
            self.puck.hit_rails(params);
            for ((mallet, region), side) in self.mallets.iter_mut().zip(&regions).zip(SIDES) {
                mallet.push_off(self.puck.pos, reach, -away_from_goal(side), region);
            }
            self.puck.vel = self.puck.vel.clamp_length(params.puck_speed_cap);
        }
        self.puck.vel = self.puck.vel * (1.0 - params.damping * params.tick_dt());
        None
    }
}

/// The side that scores when the puck centre moves from `old` to `new`: it must cross an end
/// line with the crossing point inside the mouth.
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

/// Push the puck out of `mallet` and bounce it off the moving mallet, which has infinite
/// mass. `owner` decides the push direction when the centres coincide.
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

const SIDES: [Side; 2] = [Side::Low, Side::High];

/// The direction from a side's goal towards the other end.
fn away_from_goal(side: Side) -> Vec2 {
    match side {
        Side::Low => Vec2::new(1.0, 0.0),
        Side::High => Vec2::new(-1.0, 0.0),
    }
}

/// The region of each mallet, by index.
#[must_use]
pub fn regions(params: &Params) -> [Region; 2] {
    [
        Region::of(Side::Low, params),
        Region::of(Side::High, params),
    ]
}
