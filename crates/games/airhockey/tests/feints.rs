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
        Vec2::new(-0.53, 0.10),
        Vec2::new(-0.53, -0.10),
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

fn stalled(puck: Vec2, mallets: [Vec2; 2], stall_ticks: u32) -> Match {
    let world = World {
        puck: Puck::at_rest(puck),
        mallets: mallets.map(Mallet::at_rest),
    };
    Match::from_observed(
        Params::default(),
        [Body::default(); 2],
        world,
        0,
        [0, 0],
        stall_ticks,
    )
}

#[test]
fn the_show_point_stands_to_one_side_behind_the_puck() {
    let p = playbook();
    for me in [0u8, 1] {
        let state = scene(
            side(me, Vec2::new(-0.4, 0.0)),
            [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
            [0, 0],
        );
        let reach = 0.09;
        let right = p.show_point(&state, me, 0);
        let left = p.show_point(&state, me, 2);
        assert!(
            (right - side(me, Vec2::new(-0.4 - (reach + 0.04), 0.10))).length() < 1e-9,
            "side {me}: {right:?}"
        );
        assert!(
            (left - side(me, Vec2::new(-0.4 - (reach + 0.04), -0.10))).length() < 1e-9,
            "side {me}: {left:?}"
        );
    }
}

#[test]
fn the_bank_aim_corrects_for_rail_restitution_on_the_open_side() {
    let p = playbook();
    let y_rail: f64 = 0.5 - 0.04;
    let beyond = y_rail + y_rail / 0.9;
    for me in [0u8, 1] {
        let with_opponent_at = |y: f64| {
            let mine = side(me, Vec2::new(-0.8, 0.0));
            let theirs = side(me, Vec2::new(0.9, y));
            let mallets = if me == 0 {
                [mine, theirs]
            } else {
                [theirs, mine]
            };
            scene(side(me, Vec2::new(-0.4, 0.0)), mallets, [0, 0])
        };
        let above = p.bank_aim(&with_opponent_at(0.3), me);
        assert!(
            (above - side(me, Vec2::new(1.0, -beyond))).length() < 1e-9,
            "side {me}: {above:?}"
        );
        let below = p.bank_aim(&with_opponent_at(-0.3), me);
        assert!(
            (below - side(me, Vec2::new(1.0, beyond))).length() < 1e-9,
            "side {me}: {below:?}"
        );
    }
}

#[test]
fn feints_are_offered_with_their_signals_and_dashes_follow_a_twitch() {
    let p = playbook();
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
        [0, 0],
    );
    let found = |out: &[Choice], id: u16| out.iter().find(|c| c.id == id).copied();
    let mut out = Vec::new();
    p.options(&state, 0, 2, &Cue::default(), &mut out);
    let fake = found(&out, HockeyPlaybook::FAKES[0]).expect("fake right-to-left");
    assert!(fake.deceptive && fake.shown == Some(0) && fake.real == Some(2));
    let fake = found(&out, HockeyPlaybook::FAKES[1]).expect("fake left-to-right");
    assert!(fake.deceptive && fake.shown == Some(2) && fake.real == Some(0));
    let bank = found(&out, HockeyPlaybook::BANK).expect("bank shot");
    assert!(!bank.deceptive && bank.shown.is_none() && bank.real == Some(1));
    assert!(found(&out, HockeyPlaybook::HOLD).is_some_and(|c| c.deceptive));
    assert!(
        HockeyPlaybook::DASHES
            .iter()
            .all(|&id| found(&out, id).is_none())
    );

    for (twitch, dash) in [(0u8, 2usize), (2, 0)] {
        let cue = Cue {
            pace: 1.0,
            twitch: Some(twitch),
        };
        p.options(&state, 0, 2, &cue, &mut out);
        let d = found(&out, HockeyPlaybook::DASHES[dash]).expect("a dash away from the twitch");
        assert_eq!(d.real, Some(dash as u8));
    }

    let tired = stalled(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)],
        4 * 240 + 1,
    );
    p.options(&tired, 0, 2, &Cue::default(), &mut out);
    assert!(
        found(&out, HockeyPlaybook::HOLD).is_none(),
        "no holding near the 7 s rule"
    );
    p.options(&state, 0, 1, &Cue::default(), &mut out);
    assert!(
        !out.iter().any(|c| c.deceptive),
        "repertoire 1 has no feints"
    );

    for speed in [1.5, 0.5] {
        let mut moving = state.clone();
        moving.world.puck.vel = Vec2::new(0.0, speed);
        p.options(&moving, 0, 2, &Cue::default(), &mut out);
        let controlled = speed < 1.0;
        assert_eq!(
            out.iter().any(|c| c.deceptive),
            controlled,
            "puck speed {speed}"
        );
        assert!(
            found(&out, HockeyPlaybook::STRIKES[1]).is_some(),
            "strikes stay on a moving puck"
        );
    }
}

fn run_choice(state: &Match, choice: &Choice, ticks: u32) -> (Vec<World>, Match) {
    let params = Params::default();
    let p = playbook();
    let mut s = state.clone();
    let mut worlds = Vec::new();
    let game = AirHockey::new(params, 4);
    for t in 0..ticks {
        let mine = p.act(&s, 0, choice, u64::from(t), [0.0; 2]);
        s = game.simulate(&s, &[mine], 1);
        worlds.push(s.world);
        if s.score() != [0, 0] {
            break;
        }
    }
    (worlds, s)
}

fn option(state: &Match, cue: &Cue, id: u16) -> Choice {
    let mut out = Vec::new();
    playbook().options(state, 0, 2, cue, &mut out);
    out.into_iter().find(|c| c.id == id).expect("offered")
}

#[test]
fn a_fake_wind_up_shows_one_side_then_shoots_the_other() {
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.95, -0.45)],
        [0, 0],
    );
    let fake = option(&state, &Cue::default(), HockeyPlaybook::FAKES[0]);
    let (worlds, _) = run_choice(&state, &fake, 240);
    let show = playbook().show_point(&state, 0, 0);
    assert!(
        worlds
            .iter()
            .take(60)
            .any(|w| (w.mallets[0].pos - show).length() < 0.03),
        "never reached the show point"
    );
    let on_shown_side = worlds
        .iter()
        .take(96)
        .filter(|w| {
            let rel = w.mallets[0].pos - w.puck.pos;
            rel.x < 0.0 && rel.y > 0.06
        })
        .count();
    assert!(
        on_shown_side >= 48,
        "the mallet sat on the shown side for only {on_shown_side} ticks"
    );
    let hit = worlds
        .iter()
        .find(|w| w.puck.vel.length() > 0.5)
        .expect("the puck was struck");
    let wanted = unit(Vec2::new(1.0, 0.07) - Vec2::new(-0.4, 0.0));
    let cos = unit(hit.puck.vel).dot(wanted);
    assert!(
        cos > 2.0f64.to_radians().cos_approx(),
        "shown right, so it must go into the left corner: exit {:?}, wanted {wanted:?}",
        hit.puck.vel
    );
}

#[test]
fn a_bank_shot_scores_off_the_rail() {
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.95, 0.45)],
        [0, 0],
    );
    let bank = option(&state, &Cue::default(), HockeyPlaybook::BANK);
    let (worlds, end) = run_choice(&state, &bank, 360);
    assert_eq!(end.score(), [1, 0], "the bank shot did not score");
    let lowest = worlds
        .iter()
        .map(|w| w.puck.pos.y)
        .fold(f64::INFINITY, f64::min);
    assert!(
        lowest < -0.44,
        "it went in without touching the rail: lowest y {lowest}"
    );
}

#[test]
fn a_slow_rally_waits_before_striking() {
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.95, 0.45)],
        [0, 0],
    );
    let contact = |pace: f32| {
        let cue = Cue { pace, twitch: None };
        let strike = option(&state, &cue, HockeyPlaybook::STRIKES[0]);
        let (worlds, _) = run_choice(&state, &strike, 360);
        worlds
            .iter()
            .position(|w| w.puck.vel.length() > 0.5)
            .expect("struck")
    };
    let (fast, slow) = (contact(1.0), contact(0.0));
    assert!(slow >= fast + 60, "fast at {fast}, slow at {slow}");
}

#[test]
fn extreme_params_neither_panic_nor_offer_an_impossible_bank() {
    let dead_rails = Params {
        rail_restitution: 0.0,
        ..Params::default()
    };
    let p = HockeyPlaybook::new(dead_rails.clone());
    let state = Match::from_observed(
        dead_rails,
        [Body::default(); 2],
        World {
            puck: Puck::at_rest(Vec2::new(-0.4, 0.0)),
            mallets: [
                Mallet::at_rest(Vec2::new(-0.8, 0.0)),
                Mallet::at_rest(Vec2::new(0.8, 0.0)),
            ],
        },
        0,
        [0, 0],
        0,
    );
    let mut out = Vec::new();
    p.options(&state, 0, 2, &Cue::default(), &mut out);
    assert!(
        out.iter().all(|c| c.id != HockeyPlaybook::BANK),
        "no bank shot off dead rails"
    );
    assert!(out.iter().any(|c| c.id == HockeyPlaybook::STRIKES[0]));

    let huge = Params {
        goals_to_win: u32::MAX,
        tick_hz: u32::MAX,
        ..Params::default()
    };
    let p = HockeyPlaybook::new(huge.clone());
    let state = Match::from_observed(
        huge,
        [Body::default(); 2],
        World {
            puck: Puck::at_rest(Vec2::new(-0.4, 0.0)),
            mallets: [
                Mallet::at_rest(Vec2::new(-0.8, 0.0)),
                Mallet::at_rest(Vec2::new(0.8, 0.0)),
            ],
        },
        0,
        [u32::MAX - 1, 0],
        0,
    );
    p.options(&state, 0, 2, &Cue::default(), &mut out);
    assert!(out.iter().any(|c| c.id == HockeyPlaybook::HOLD));
    assert_eq!(p.importance(&state, 0), 1.0);
}

#[test]
fn a_goal_starts_a_new_possession() {
    let p = playbook();
    let puck = Vec2::new(-0.4, 0.0);
    let mallets = [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.0)];
    let before = p.possession(&scene(puck, mallets, [0, 0]), 0);
    let after = p.possession(&scene(puck, mallets, [0, 1]), 0);
    assert!(before.is_some() && after.is_some() && before != after);
    assert_eq!(p.possession(&scene(-puck, mallets, [0, 1]), 0), None);
}

#[test]
fn hold_keeps_three_seconds_for_the_attack_before_the_stall_rule() {
    let short = Params {
        stall_seconds: 2.0,
        ..Params::default()
    };
    let p = HockeyPlaybook::new(short.clone());
    let state = Match::from_observed(
        short,
        [Body::default(); 2],
        World {
            puck: Puck::at_rest(Vec2::new(-0.4, 0.0)),
            mallets: [
                Mallet::at_rest(Vec2::new(-0.8, 0.0)),
                Mallet::at_rest(Vec2::new(0.8, 0.0)),
            ],
        },
        0,
        [0, 0],
        0,
    );
    let mut out = Vec::new();
    p.options(&state, 0, 2, &Cue::default(), &mut out);
    assert!(
        out.iter().all(|c| c.id != HockeyPlaybook::HOLD),
        "no hold when the stall rule is 2 s"
    );
}

#[test]
fn a_bank_shot_is_read_where_it_enters_the_goal() {
    let p = playbook();
    for shooter in [0u8, 1] {
        let defender = 1 - shooter;
        let puck = side(shooter, Vec2::new(-0.4, 0.0));
        let mut mallets = [Vec2::ZERO; 2];
        mallets[usize::from(shooter)] = side(shooter, Vec2::new(-0.49, 0.0));
        mallets[usize::from(defender)] = side(shooter, Vec2::new(0.9, 0.3));
        let before = scene(puck, mallets, [0, 0]);
        let aim = p.bank_aim(&before, shooter);
        let mut after = before.clone();
        after.world.puck.vel = unit(aim - puck) * 6.0;
        assert_eq!(
            p.read(&before, &after, shooter).shot,
            Some(1),
            "shooter {shooter}"
        );
    }
}

#[test]
fn every_class_can_be_shown_falsely_and_dashes_need_a_real_twitch() {
    let p = playbook();
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.8, 0.3)],
        [0, 0],
    );
    let mut out = Vec::new();
    p.options(&state, 0, 2, &Cue::default(), &mut out);
    for class in 0..3u8 {
        assert!(
            out.iter().any(|c| c.deceptive && c.shown == Some(class)),
            "no fake shows class {class}: a pattern there could never be broken"
        );
    }
    let fake_bank = out
        .iter()
        .find(|c| c.id == HockeyPlaybook::FAKE_BANK)
        .expect("a fake ending in a bank shot");
    assert!(
        fake_bank.deceptive && fake_bank.real == Some(1) && fake_bank.shown == Some(2),
        "{fake_bank:?}"
    );

    let dashes = |twitch: u8| {
        let mut out = Vec::new();
        p.options(
            &state,
            0,
            2,
            &Cue {
                pace: 1.0,
                twitch: Some(twitch),
            },
            &mut out,
        );
        out.iter()
            .filter(|c| HockeyPlaybook::DASHES.contains(&c.id))
            .filter_map(|c| c.real)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        dashes(1),
        vec![0, 2],
        "away from the centre is either corner"
    );
    assert!(dashes(3).is_empty() && dashes(255).is_empty());
}

#[test]
fn a_fake_bank_shows_a_corner_then_scores_off_the_rail() {
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.95, 0.45)],
        [0, 0],
    );
    let fake = option(&state, &Cue::default(), HockeyPlaybook::FAKE_BANK);
    let (worlds, end) = run_choice(&state, &fake, 480);
    let show = playbook().show_point(&state, 0, 2);
    assert!(
        worlds
            .iter()
            .take(96)
            .any(|w| (w.mallets[0].pos - show).length() < 0.03)
    );
    assert_eq!(end.score(), [1, 0], "the fake bank did not score");
    let lowest = worlds
        .iter()
        .map(|w| w.puck.pos.y)
        .fold(f64::INFINITY, f64::min);
    assert!(
        lowest < -0.44,
        "it went in without touching the rail: lowest y {lowest}"
    );
}

#[test]
fn a_dash_strikes_into_the_corner_away_from_the_twitch() {
    let state = scene(
        Vec2::new(-0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.95, -0.45)],
        [0, 0],
    );
    let cue = Cue {
        pace: 1.0,
        twitch: Some(0),
    };
    let dash = option(&state, &cue, HockeyPlaybook::DASHES[2]);
    let (worlds, _) = run_choice(&state, &dash, 240);
    let hit = worlds
        .iter()
        .find(|w| w.puck.vel.length() > 0.5)
        .expect("the puck was struck");
    assert!(
        worlds
            .iter()
            .position(|w| w.puck.vel.length() > 0.5)
            .unwrap()
            < 60,
        "a dash does not wait"
    );
    let wanted = unit(Vec2::new(1.0, 0.07) - Vec2::new(-0.4, 0.0));
    assert!(
        unit(hit.puck.vel).dot(wanted) > 2.0f64.to_radians().cos_approx(),
        "exit {:?}",
        hit.puck.vel
    );
}

#[test]
fn a_shot_is_read_once_even_when_the_mallet_follows_the_puck() {
    let p = playbook();
    let mallets = [Vec2::new(-0.8, 0.0), Vec2::new(0.49, 0.0)];
    let before = scene(Vec2::new(0.4, 0.0), mallets, [0, 0]);
    let mut struck = before.clone();
    struck.world.puck.vel = Vec2::new(-2.5, 0.0);
    assert_eq!(
        p.read(&before, &struck, 1).shot,
        Some(1),
        "the strike itself is a shot"
    );
    let mut still_close = struck.clone();
    still_close.world.puck.pos = Vec2::new(0.39, 0.0);
    still_close.world.mallets[1].pos = Vec2::new(0.47, 0.0);
    assert_eq!(
        p.read(&struck, &still_close, 1).shot,
        None,
        "the same shot a second time"
    );
}

#[test]
fn a_tiny_rail_restitution_offers_no_bank_and_a_restrike_is_read() {
    for restitution in [1e-310, 1e-200, 0.05] {
        let tiny = Params {
            rail_restitution: restitution,
            ..Params::default()
        };
        let p = HockeyPlaybook::new(tiny.clone());
        let state = Match::from_observed(
            tiny,
            [Body::default(); 2],
            World {
                puck: Puck::at_rest(Vec2::new(-0.4, 0.0)),
                mallets: [
                    Mallet::at_rest(Vec2::new(-0.8, 0.0)),
                    Mallet::at_rest(Vec2::new(0.8, 0.3)),
                ],
            },
            0,
            [0, 0],
            0,
        );
        let mut out = Vec::new();
        p.options(&state, 0, 2, &Cue::default(), &mut out);
        assert!(
            out.iter()
                .all(|c| c.id != HockeyPlaybook::BANK && c.id != HockeyPlaybook::FAKE_BANK),
            "restitution {restitution}"
        );
    }

    let p = playbook();
    let mut before = scene(
        Vec2::new(0.4, 0.0),
        [Vec2::new(-0.8, 0.0), Vec2::new(0.49, 0.0)],
        [0, 0],
    );
    before.world.puck.vel = Vec2::new(-2.5, 0.0);
    let mut after = before.clone();
    after.world.puck.vel = Vec2::new(-6.0, 0.0);
    assert_eq!(
        p.read(&before, &after, 1).shot,
        Some(1),
        "a second strike on a moving puck"
    );
}
