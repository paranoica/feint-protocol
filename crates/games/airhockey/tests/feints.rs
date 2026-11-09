use airhockey::{AirHockey, Body, HockeyPlaybook, Mallet, Match, Params, Puck, Vec2, World};
use feint_ai::{Choice, Cue, Playbook, Stance};
use feint_protocol::GameAdapter;

fn playbook() -> HockeyPlaybook {
    HockeyPlaybook::new(Params::default())
}

fn scene(puck: Vec2, mallets: [Vec2; 2], score: [u32; 2]) -> Match {
    let world = World {
        puck: Puck::at_rest(puck),
        mallets: mallets.map(Mallet::at_rest),
    };
    Match::from_observed(Params::default(), [Body::default(); 2], world, 0, score, 0)
}

fn side(me: u8, p: Vec2) -> Vec2 {
    if me == 0 { p } else { -p }
}

fn unit(v: Vec2) -> Vec2 {
    v * (1.0 / v.length())
}

fn options(state: &Match, me: u8, repertoire: u8) -> Vec<Choice> {
    let mut out = Vec::new();
    playbook().options(state, me, repertoire, &Cue::default(), &mut out);
    out
}

#[test]
fn importance_follows_the_score() {
    let p = playbook();
    let at = |score: [u32; 2]| {
        p.importance(
            &scene(
                Vec2::ZERO,
                [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
                score,
            ),
            0,
        )
    };
    for decisive in [[6, 0], [0, 6], [6, 6], [5, 5]] {
        assert_eq!(at(decisive), 1.0, "{decisive:?}");
    }
    for calm in [[0, 0], [3, 2], [4, 4], [5, 3]] {
        assert_eq!(at(calm), 0.0, "{calm:?}");
    }
}

#[test]
fn possession_is_the_puck_in_my_half() {
    let p = playbook();
    let mallets = [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)];
    let low = scene(Vec2::new(-0.3, 0.1), mallets, [0, 0]);
    assert!(p.possession(&low, 0).is_some() && p.possession(&low, 1).is_none());
    let high = scene(Vec2::new(0.3, 0.1), mallets, [0, 0]);
    assert!(p.possession(&high, 0).is_none() && p.possession(&high, 1).is_some());
}

#[test]
fn repertoire_two_offers_defence_variants_and_strikes_carry_their_corner() {
    for me in [0u8, 1] {
        let state = scene(
            side(me, Vec2::new(-0.4, 0.0)),
            [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
            [0, 0],
        );
        let two: Vec<u16> = options(&state, me, 2).iter().map(|c| c.id).collect();
        for id in HockeyPlaybook::DEFENDS
            .iter()
            .chain(&HockeyPlaybook::STRIKES)
        {
            assert!(two.contains(id), "side {me}: {id} missing from {two:?}");
        }
        assert!(!two.contains(&HockeyPlaybook::DEFEND));
        let one: Vec<Choice> = options(&state, me, 1);
        assert!(one.iter().any(|c| c.id == HockeyPlaybook::DEFEND));
        assert!(!one.iter().any(|c| HockeyPlaybook::DEFENDS.contains(&c.id)));
        for (k, &id) in HockeyPlaybook::STRIKES.iter().enumerate() {
            let strike = one.iter().find(|c| c.id == id).unwrap();
            assert_eq!((strike.shown, strike.real), (Some(k as u8), Some(k as u8)));
        }
    }
}

#[test]
fn cover_spot_stands_on_the_line_from_the_shown_corner_to_the_puck() {
    for attacker in [0u8, 1] {
        let defender = 1 - attacker;
        let puck = side(attacker, Vec2::new(-0.4, 0.05));
        let state = scene(puck, [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)], [0, 0]);
        let corner = side(attacker, Vec2::new(1.0, 0.07));
        let expected = corner + unit(puck - corner) * 0.15;
        let got = playbook().cover_spot(&state, defender, 2);
        assert!(
            (got - expected).length() < 1e-9,
            "attacker {attacker}: {got:?} vs {expected:?}"
        );
    }
}

#[test]
fn stances_move_the_rollout_opponent() {
    let game = AirHockey::new(Params::default(), 3);
    let p = playbook();
    let state = scene(
        Vec2::new(-0.4, 0.05),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
        [0, 0],
    );
    let cover = p.opponent_move(&state, 1, Stance::Cover(2));
    let after = game.simulate(&state, &[cover], 120);
    let spot = p.cover_spot(&state, 1, 2);
    assert!(
        (after.world.mallets[1].pos - spot).length() < 0.01,
        "{:?} vs {spot:?}",
        after.world.mallets[1].pos
    );

    let theirs = scene(
        Vec2::new(0.4, 0.0),
        [Vec2::new(-0.95, -0.45), Vec2::new(0.8, 0.0)],
        [0, 0],
    );
    let mut s = theirs.clone();
    let mut shot = None;
    for _ in 0..120 {
        let shoot = p.opponent_move(&s, 1, Stance::Shoot(0));
        let next = game.simulate(&s, &[shoot], 2);
        shot = shot.or(p.read(&s, &next, 1).shot);
        s = next;
    }
    assert_eq!(
        shot,
        Some(0),
        "the shot was not read as the shooter's right corner"
    );
    assert_eq!(s.score(), [0, 1], "an open goal should be scored");
}

#[test]
fn reads_see_which_corner_the_opponent_covers() {
    for attacker in [0u8, 1] {
        let defender = 1 - attacker;
        let puck = side(attacker, Vec2::new(-0.4, 0.05));
        let mut mallets = [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)];
        let before = scene(puck, mallets, [0, 0]);
        let p = playbook();
        mallets[usize::from(defender)] = p.cover_spot(&before, defender, 2);
        let after = scene(puck, mallets, [0, 0]);
        assert_eq!(
            p.read(&before, &after, defender).covered,
            Some(2),
            "attacker {attacker}"
        );
        let still = scene(puck, [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)], [0, 0]);
        assert_eq!(p.read(&before, &still, defender).covered, None);
    }
}

#[test]
fn contact_aiming_leaves_along_the_aim_from_any_side() {
    let params = Params::default();
    let p = playbook();
    let puck = Vec2::new(-0.4, 0.0);
    let aim = Vec2::new(1.0, 0.07);
    let wanted = unit(aim - puck);
    for start in [
        Vec2::new(-0.8, 0.0),
        Vec2::new(-0.65, 0.2),
        Vec2::new(-0.65, -0.2),
    ] {
        let mut world = World {
            puck: Puck::at_rest(puck),
            mallets: [
                Mallet::at_rest(start),
                Mallet::at_rest(Vec2::new(0.95, 0.45)),
            ],
        };
        let mut hit = None;
        for _ in 0..240 {
            let state =
                Match::from_observed(params.clone(), [Body::default(); 2], world, 0, [0, 0], 0);
            world.mallets[0].target = p.contact_target(&state, 0, aim);
            world.tick(&params, &[Body::default(); 2]);
            if world.puck.vel.length() > 0.5 {
                hit = Some(world.puck.vel);
                break;
            }
        }
        let v = hit.unwrap_or_else(|| panic!("from {start:?}: never hit the puck"));
        let cos = unit(v).dot(wanted);
        assert!(
            cos > 2.0f64.to_radians().cos_approx(),
            "from {start:?}: exit {v:?}, wanted {wanted:?}"
        );
    }
}

trait CosApprox {
    fn cos_approx(self) -> f64;
}

impl CosApprox for f64 {
    fn cos_approx(self) -> f64 {
        1.0 - self * self / 2.0
    }
}
