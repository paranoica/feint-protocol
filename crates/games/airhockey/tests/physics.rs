use airhockey::{
    Body, Mallet, Params, Puck, Side, Vec2, World, goal_crossing, puck_mallet, regions,
};

const EPS: f64 = 1e-9;

fn no_damping() -> Params {
    Params {
        damping: 0.0,
        ..Params::default()
    }
}

/// Mallets parked in the top corners, out of the way of the shots below.
fn parked(params: &Params) -> World {
    let mut world = World::new(params);
    world.mallets[0] = Mallet::at_rest(Vec2::new(-0.95, 0.45));
    world.mallets[1] = Mallet::at_rest(Vec2::new(0.95, 0.45));
    world
}

fn bodies() -> [Body; 2] {
    [Body::default(); 2]
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * (1.0 + b.abs())
}

/// First goal within `ticks`, with the tick it happened on.
fn first_goal(world: &mut World, params: &Params, ticks: u32) -> Option<(u32, Side)> {
    (0..ticks).find_map(|t| world.tick(params, &bodies()).map(|side| (t, side)))
}

fn shoot(from: Vec2, vel: Vec2, params: &Params, ticks: u32) -> (Option<Side>, World) {
    let mut world = parked(params);
    world.puck = Puck { pos: from, vel };
    let goal = first_goal(&mut world, params, ticks).map(|(_, side)| side);
    (goal, world)
}

/// Shoot towards +x until the puck first comes back; `None` when it scored or never did.
fn comes_back(from: Vec2, speed: f64, params: &Params) -> Option<World> {
    let mut world = parked(params);
    world.puck = Puck {
        pos: from,
        vel: Vec2::new(speed, 0.0),
    };
    for _ in 0..120 {
        if world.tick(params, &bodies()).is_some() {
            return None;
        }
        if world.puck.vel.x < 0.0 {
            return Some(world);
        }
    }
    None
}

#[test]
fn free_puck_follows_discrete_damping() {
    let params = Params::default();
    let dt = params.tick_dt();
    let mut world = parked(&params);
    let v0 = Vec2::new(0.5, 0.1);
    world.puck = Puck {
        pos: Vec2::new(-0.3, -0.2),
        vel: v0,
    };
    let (mut pos, mut vel) = (world.puck.pos, v0);
    for _ in 0..200 {
        assert_eq!(world.tick(&params, &bodies()), None);
        pos += vel * dt;
        vel = vel * (1.0 - params.damping * dt);
    }
    assert!(close(world.puck.vel.x, vel.x, 1e-12) && close(world.puck.vel.y, vel.y, 1e-12));
    assert!(
        (world.puck.pos - pos).length() < EPS,
        "{:?} vs {pos:?}",
        world.puck.pos
    );
}

#[test]
fn rail_bounce_flips_normal_scaled_by_restitution() {
    let params = no_damping();
    let mut world = parked(&params);
    world.puck = Puck {
        pos: Vec2::new(0.0, 0.3),
        vel: Vec2::new(0.5, 2.0),
    };
    for _ in 0..100 {
        world.tick(&params, &bodies());
        if world.puck.vel.y < 0.0 {
            break;
        }
    }
    assert!(
        close(world.puck.vel.y, -0.9 * 2.0, 1e-12),
        "{:?}",
        world.puck
    );
    assert!(close(world.puck.vel.x, 0.5, 1e-12), "{:?}", world.puck);
    assert!(world.puck.pos.y <= 0.46 + EPS);
}

#[test]
fn puck_energy_never_grows_without_mallets_in_play() {
    let params = Params {
        damping: 0.0,
        mouth_half_width: 0.0,
        ..Params::default()
    };
    let mut world = parked(&params);
    world.puck = Puck {
        pos: Vec2::new(0.1, 0.05),
        vel: Vec2::new(3.7, 2.3),
    };
    let start = world.puck.vel.length();
    let mut bounces = 0;
    for t in 0..10_000 {
        let before = world.puck.vel;
        assert_eq!(
            world.tick(&params, &bodies()),
            None,
            "goal through a closed mouth"
        );
        assert!(
            world.puck.vel.length() <= start * (1.0 + EPS),
            "speed grew at tick {t}"
        );
        let after = world.puck.vel;
        bounces += usize::from(before.x * after.x < 0.0) + usize::from(before.y * after.y < 0.0);
    }
    assert!(bounces > 10, "the puck hardly bounced: {bounces}");
    assert!(world.puck.vel.length() > 0.0, "the puck froze");
}

#[test]
fn straight_shots_into_either_mouth_score_for_the_attacker() {
    let params = no_damping();
    let (goal, _) = shoot(Vec2::new(0.6, 0.0), Vec2::new(5.0, 0.0), &params, 240);
    assert_eq!(goal, Some(Side::Low));
    let (goal, _) = shoot(Vec2::new(-0.6, 0.0), Vec2::new(-5.0, 0.0), &params, 240);
    assert_eq!(goal, Some(Side::High));
}

#[test]
fn shot_just_inside_mouth_scores() {
    let params = no_damping();
    let y = 0.125 - 0.04 - 1e-6;
    let (goal, _) = shoot(Vec2::new(0.6, y), Vec2::new(5.0, 0.0), &params, 240);
    assert_eq!(goal, Some(Side::Low));
}

#[test]
fn shot_just_outside_mouth_bounces_off_end_rail() {
    let params = no_damping();
    let y = 0.125 + 0.04 + 1e-6;
    let back = comes_back(Vec2::new(0.6, y), 5.0, &params).expect("bounced back, no goal");
    assert!(close(back.puck.vel.x, -0.9 * 5.0, 1e-12), "{:?}", back.puck);
    assert_eq!(
        back.puck.vel.y, 0.0,
        "the end rail is flat: {:?}",
        back.puck
    );
}

#[test]
fn shot_at_post_deflects() {
    let params = no_damping();
    // Centre on the post line: a head-on hit on the post sends the puck straight back.
    let back = comes_back(Vec2::new(0.6, 0.125), 5.0, &params).expect("bounced back, no goal");
    assert!(close(back.puck.vel.x, -0.9 * 5.0, 1e-12), "{:?}", back.puck);
}

#[test]
fn post_grazed_at_cap_speed_deflects() {
    // Inside the mouth, 2 mm into the post's reach: at the cap a substep moves the puck
    // 2.1 cm, the contact chord is 2.5 cm, so the post cannot be skipped. It turns the
    // puck towards the middle of the goal.
    let params = no_damping();
    let mut world = parked(&params);
    world.puck = Puck {
        pos: Vec2::new(0.2, 0.125 - 0.04 + 0.002),
        vel: Vec2::new(params.puck_speed_cap, 0.0),
    };
    let mut min_vy: f64 = 0.0;
    for _ in 0..60 {
        let goal = world.tick(&params, &bodies());
        min_vy = min_vy.min(world.puck.vel.y);
        if goal.is_some() {
            break;
        }
    }
    assert!(
        min_vy < -5.0,
        "the graze did not deflect the puck: {min_vy}"
    );
}

#[test]
fn goal_is_decided_by_the_crossing_point() {
    let params = Params::default();
    let at = |x0, y0, x1, y1| goal_crossing(Vec2::new(x0, y0), Vec2::new(x1, y1), &params);
    // Crosses at y = 0.12 inside the mouth, ends outside its y range.
    assert_eq!(at(0.99, 0.10, 1.01, 0.14), Some(Side::Low));
    // Crosses at y = 0.13 outside the mouth, whichever end is inside.
    assert_eq!(at(0.99, 0.06, 1.01, 0.20), None);
    assert_eq!(at(0.99, 0.20, 1.01, 0.06), None);
    assert_eq!(at(-0.99, -0.10, -1.01, -0.12), Some(Side::High));
    // Landing exactly on the line counts; exactly on the post line does not.
    assert_eq!(at(0.99, 0.0, 1.0, 0.0), Some(Side::Low));
    assert_eq!(at(0.99, 0.125, 1.01, 0.125), None);
    // No crossing, or crossing outwards.
    assert_eq!(at(0.98, 0.0, 0.99, 0.0), None);
    assert_eq!(at(1.01, 0.0, 0.99, 0.0), None);
}

#[test]
fn moving_mallet_hits_resting_puck_with_kinematic_formula() {
    let params = no_damping();
    let mallet = Mallet {
        pos: Vec2::ZERO,
        vel: Vec2::new(4.0, 0.0),
        target: Vec2::ZERO,
    };
    let mut puck = Puck::at_rest(Vec2::new(0.089, 0.0));
    puck_mallet(&mut puck, &mallet, Side::Low, &params);
    assert!(
        close(puck.pos.x, 0.09, 1e-12) && puck.pos.y == 0.0,
        "{puck:?}"
    );
    assert!(
        close(puck.vel.x, (1.0 + 0.8) * 4.0, 1e-12) && puck.vel.y == 0.0,
        "{puck:?}"
    );
}

#[test]
fn separating_puck_keeps_its_velocity_and_far_puck_is_untouched() {
    let params = no_damping();
    let mallet = Mallet {
        pos: Vec2::ZERO,
        vel: Vec2::new(4.0, 0.0),
        target: Vec2::ZERO,
    };
    let mut leaving = Puck {
        pos: Vec2::new(0.089, 0.0),
        vel: Vec2::new(10.0, 0.0),
    };
    puck_mallet(&mut leaving, &mallet, Side::Low, &params);
    assert_eq!(leaving.vel, Vec2::new(10.0, 0.0));
    assert!(close(leaving.pos.x, 0.09, 1e-12));
    let far = Puck {
        pos: Vec2::new(0.0901, 0.0),
        vel: Vec2::new(-1.0, 0.0),
    };
    let mut copy = far;
    puck_mallet(&mut copy, &mallet, Side::Low, &params);
    assert_eq!(copy, far);
}

#[test]
fn puck_on_mallet_centre_is_pushed_away_from_owner_goal() {
    let params = no_damping();
    let mallet = Mallet::at_rest(Vec2::new(-0.5, 0.0));
    for (owner, x) in [(Side::Low, -0.41), (Side::High, -0.59)] {
        let mut puck = Puck::at_rest(Vec2::new(-0.5, 0.0));
        puck_mallet(&mut puck, &mallet, owner, &params);
        assert!(puck.pos.is_finite() && puck.vel.is_finite(), "{puck:?}");
        assert!(
            close(puck.pos.x, x, 1e-12) && puck.pos.y == 0.0,
            "{owner:?}: {puck:?}"
        );
    }
}

#[test]
fn fast_puck_bounces_off_mallet_without_tunnelling() {
    let params = no_damping();
    let mut world = parked(&params);
    world.mallets[0] = Mallet::at_rest(Vec2::new(-0.5, 0.0));
    world.puck = Puck {
        pos: Vec2::new(0.2, 0.01),
        vel: Vec2::new(-params.puck_speed_cap, 0.0),
    };
    let mut bounced = false;
    for _ in 0..60 {
        world.tick(&params, &bodies());
        assert!(
            world.puck.pos.x > world.mallets[0].pos.x,
            "tunnelled: {world:?}"
        );
        bounced |= world.puck.vel.x > 0.0;
    }
    assert!(bounced);
}

fn assert_sane(world: &World, params: &Params, context: &str) {
    let puck = world.puck;
    assert!(
        puck.pos.is_finite() && puck.vel.is_finite(),
        "{context}: {world:?}"
    );
    assert!(
        puck.vel.length() <= params.puck_speed_cap * (1.0 + EPS),
        "{context}: over the cap {world:?}"
    );
    let r = params.puck_radius;
    assert!(
        puck.pos.y.abs() <= params.half_width - r + EPS,
        "{context}: off the table {world:?}"
    );
    let in_mouth = puck.pos.y.abs() < params.mouth_half_width;
    let x_limit = if in_mouth {
        params.half_length + 0.03
    } else {
        params.half_length - r + EPS
    };
    assert!(
        puck.pos.x.abs() <= x_limit,
        "{context}: off the table {world:?}"
    );
    for (mallet, region) in world.mallets.iter().zip(regions(params)) {
        assert!(
            mallet.pos.is_finite() && mallet.vel.is_finite(),
            "{context}: {world:?}"
        );
        assert!(
            mallet.pos.x >= region.min.x - EPS
                && mallet.pos.x <= region.max.x + EPS
                && mallet.pos.y >= region.min.y - EPS
                && mallet.pos.y <= region.max.y + EPS,
            "{context}: mallet out of its region {world:?}"
        );
    }
}

fn assert_no_overlap(world: &World, params: &Params) {
    let min = params.puck_radius + params.mallet_radius;
    for mallet in &world.mallets {
        assert!(
            (world.puck.pos - mallet.pos).length() >= min - 1e-6,
            "puck inside a mallet: {world:?}"
        );
    }
}

#[test]
fn puck_pinned_against_rail_stays_on_table_and_finite() {
    let params = Params::default();
    let mut world = parked(&params);
    world.mallets[0] = Mallet::at_rest(Vec2::new(-0.5, 0.2));
    world.mallets[0].target = Vec2::new(-0.5, 0.45);
    world.puck = Puck::at_rest(Vec2::new(-0.5, 0.46));
    for t in 0..480 {
        assert_eq!(world.tick(&params, &bodies()), None);
        assert_sane(&world, &params, &format!("tick {t}"));
        assert_no_overlap(&world, &params);
    }
    let gap = (world.puck.pos - world.mallets[0].pos).length() - 0.09;
    assert!(
        gap.abs() < 1e-3,
        "the mallet is not pressing the puck: gap {gap}"
    );
}

#[test]
fn puck_squeezed_at_centre_line_is_finite() {
    let params = Params::default();
    let mut world = World::new(&params);
    world.mallets[0].target = Vec2::new(-0.05, 0.0);
    world.mallets[1].target = Vec2::new(0.05, 0.0);
    for t in 0..480 {
        assert_eq!(world.tick(&params, &bodies()), None);
        assert_sane(&world, &params, &format!("tick {t}"));
        assert_no_overlap(&world, &params);
    }
    for mallet in &world.mallets {
        let gap = (world.puck.pos - mallet.pos).length() - 0.09;
        assert!(
            gap.abs() < 1e-3,
            "a mallet is not pressing the puck: gap {gap}"
        );
    }
}

struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

#[test]
fn random_play_keeps_state_finite_and_puck_in_bounds() {
    let params = Params::default();
    let mut world = World::new(&params);
    let mut rng = Lcg(2026);
    let mut goals = 0;
    for t in 0..1_000_000u32 {
        if t % 30 == 0 {
            for mallet in &mut world.mallets {
                mallet.target = Vec2::new(rng.range(-1.2, 1.2), rng.range(-0.7, 0.7));
            }
        }
        if t % 997 == 0 {
            world.puck.vel = Vec2::new(rng.range(-30.0, 30.0), rng.range(-30.0, 30.0));
        }
        if world.tick(&params, &bodies()).is_some() {
            goals += 1;
            world.puck = Puck::at_rest(Vec2::new(rng.range(-0.5, 0.5), 0.0));
        }
        assert_sane(&world, &params, &format!("tick {t}"));
    }
    assert!(goals > 10, "too few goals for a meaningful run: {goals}");
}
