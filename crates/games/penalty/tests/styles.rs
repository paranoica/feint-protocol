use penalty::persons::{Habits, TEST_PERSONS};
use penalty::styles::{KeeperStyle, KickSeen, Mind, REACT, ShooterStyle};

const EPS: f64 = 1e-12;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

fn bot_kick(lean: Option<u8>, shot: u8, dive: Option<u8>, goal: bool) -> KickSeen {
    KickSeen {
        person_shot: false,
        lean,
        shot: Some(shot),
        dive,
        goal,
    }
}

fn person_kick(shot: u8, dive: Option<u8>, goal: bool) -> KickSeen {
    KickSeen {
        person_shot: true,
        lean: None,
        shot: Some(shot),
        dive,
        goal,
    }
}

fn keeper(style: KeeperStyle) -> Mind {
    Mind::new(style, ShooterStyle::Static, Habits::default())
}

fn shooter(style: ShooterStyle) -> Mind {
    Mind::new(KeeperStyle::Static, style, Habits::default())
}

#[test]
fn a_trusting_keeper_follows_the_lean_after_honest_kicks() {
    let style = KeeperStyle::Trust {
        rate: 0.4,
        guess: 1.0,
        start: 0.5,
    };
    let mut honest = keeper(style);
    let mut fooled = keeper(style);
    for _ in 0..4 {
        honest.learn(bot_kick(Some(0), 0, None, true));
        fooled.learn(bot_kick(Some(0), 2, None, true));
    }
    let up = 1.0 - 0.5 * 0.6f64 * 0.6 * 0.6 * 0.6;
    let down = 0.5 * 0.6f64 * 0.6 * 0.6 * 0.6;
    let a = honest.keeper_odds(Some(2));
    assert!(close(a[2], up * up), "{a:?}");
    assert!(close(a[0], (1.0 - up) * (1.0 - up)), "{a:?}");
    assert!(close(a[REACT], 1.0 - up * up - (1.0 - up) * (1.0 - up)));
    let b = fooled.keeper_odds(Some(2));
    assert!(close(b[2], down * down), "{b:?}");
    assert!(close(b[0], (1.0 - down) * (1.0 - down)), "{b:?}");
    assert_eq!(honest.keeper_odds(None), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_shot_down_the_middle_moves_no_trust() {
    let style = KeeperStyle::Trust {
        rate: 0.4,
        guess: 0.8,
        start: 0.5,
    };
    let mut m = keeper(style);
    let before = m.keeper_odds(Some(0));
    m.learn(bot_kick(None, 1, None, true));
    m.learn(bot_kick(Some(0), 1, None, true));
    assert_eq!(m.keeper_odds(Some(0)), before);
}

#[test]
fn an_anti_recency_keeper_dives_away_from_a_repeated_corner() {
    let style = KeeperStyle::AntiRecency {
        decay: 0.7,
        guess: 1.0,
    };
    let mut m = keeper(style);
    for _ in 0..3 {
        m.learn(bot_kick(Some(0), 0, None, true));
    }
    let w0 = 0.7f64 * 0.7 * 0.7 + 0.7 * 0.7 + 0.7 + 1.0;
    let w = 0.7f64 * 0.7 * 0.7;
    let total = w0 + 2.0 * w;
    let odds = m.keeper_odds(Some(0));
    assert!(close(odds[0], (total - w0) / (2.0 * total)), "{odds:?}");
    assert!(close(odds[2], (total - w) / (2.0 * total)), "{odds:?}");
    assert!(close(odds[REACT], 0.0));
    assert!(odds[2] > odds[0] * 4.0);
}

#[test]
fn a_recency_keeper_expects_the_corner_it_saw_most() {
    let style = KeeperStyle::Recency {
        decay: 0.5,
        guess: 0.6,
    };
    let mut m = keeper(style);
    m.learn(bot_kick(Some(2), 2, None, true));
    m.learn(bot_kick(None, 1, None, false));
    let w = [0.25, 1.25, 0.75];
    let total = 2.25;
    let odds = m.keeper_odds(None);
    for c in 0..3 {
        assert!(close(odds[c], 0.6 * w[c] / total), "{odds:?}");
    }
    assert!(close(odds[REACT], 0.4));
}

#[test]
fn a_win_stay_shooter_repeats_after_a_goal() {
    let style = ShooterStyle::WinStay { stay: 0.8 };
    let mut scored = shooter(style);
    scored.learn(person_kick(2, Some(0), true));
    let a = scored.shooter_odds();
    assert!(
        close(a[2], 0.8) && close(a[0], 0.1) && close(a[1], 0.1),
        "{a:?}"
    );
    let mut saved = shooter(style);
    saved.learn(person_kick(2, Some(2), false));
    let b = saved.shooter_odds();
    assert!(
        close(b[2], 0.2) && close(b[0], 0.4) && close(b[1], 0.4),
        "{b:?}"
    );
}

#[test]
fn a_win_shift_shooter_does_the_reverse() {
    let style = ShooterStyle::WinShift { stay: 0.7 };
    let mut scored = shooter(style);
    scored.learn(person_kick(0, None, true));
    let a = scored.shooter_odds();
    assert!(
        close(a[0], 0.3) && close(a[1], 0.35) && close(a[2], 0.35),
        "{a:?}"
    );
}

#[test]
fn an_avoiding_shooter_shoots_away_from_the_last_dive() {
    let style = ShooterStyle::AvoidDive { avoid: 0.7 };
    let mut m = shooter(style);
    m.learn(person_kick(1, Some(2), true));
    let odds = m.shooter_odds();
    let kept = 1.0 - 0.7;
    let total = 1.0 + 1.0 + kept;
    assert!(
        close(odds[2], kept / total) && close(odds[0], 1.0 / total),
        "{odds:?}"
    );
}

#[test]
fn a_keeper_style_ignores_its_own_shots_and_a_shooter_style_the_bots() {
    let mut k = keeper(KeeperStyle::Recency {
        decay: 0.5,
        guess: 1.0,
    });
    let before = k.keeper_odds(None);
    k.learn(person_kick(0, Some(0), false));
    assert_eq!(k.keeper_odds(None), before);
    let mut s = shooter(ShooterStyle::WinStay { stay: 0.9 });
    let before = s.shooter_odds();
    s.learn(bot_kick(Some(0), 0, Some(0), false));
    assert_eq!(s.shooter_odds(), before);
}

#[test]
fn a_static_mind_answers_like_its_habits() {
    let habits = Habits {
        corner: [2.0, 1.0, 1.0],
        guess: 0.3,
        follows_lean: false,
        ..Habits::default()
    };
    let mut m = Mind::new(KeeperStyle::Static, ShooterStyle::Static, habits);
    for _ in 0..3 {
        m.learn(bot_kick(Some(0), 0, None, true));
        m.learn(person_kick(0, Some(0), false));
    }
    let s = m.shooter_odds();
    assert!(
        close(s[0], 0.5) && close(s[1], 0.25) && close(s[2], 0.25),
        "{s:?}"
    );
    let k = m.keeper_odds(Some(0));
    let g = f64::from(0.3f32);
    assert!(
        close(k[2], g) && close(k[REACT], 1.0 - g) && close(k[0], 0.0),
        "{k:?}"
    );
    assert_eq!(m.keeper_odds(None), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn every_answer_is_a_distribution() {
    for id in (0..400).chain(TEST_PERSONS..TEST_PERSONS + 400) {
        for mut m in [Mind::person(id), Mind::held_out(id)] {
            for k in 0..12u8 {
                for lean in [None, Some(0), Some(2)] {
                    let odds = m.keeper_odds(lean);
                    assert!(
                        odds.iter().all(|&p| (0.0..=1.0).contains(&p)),
                        "{id} {odds:?}"
                    );
                    assert!(
                        (odds.iter().sum::<f64>() - 1.0).abs() < 1e-9,
                        "{id} {odds:?}"
                    );
                }
                let s = m.shooter_odds();
                assert!((s.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{id} {s:?}");
                let c = k % 3;
                m.learn(bot_kick(
                    Some(2 * (k % 2)),
                    c,
                    Some((k + 1) % 3),
                    k % 4 == 0,
                ));
                m.learn(person_kick(c, Some(k % 3), k % 3 == 1));
            }
        }
    }
}

#[test]
fn held_out_styles_are_not_in_the_training_prior() {
    let training = |m: &Mind| {
        matches!(
            m.keeper,
            KeeperStyle::Static
                | KeeperStyle::Trust { .. }
                | KeeperStyle::Recency { .. }
                | KeeperStyle::AntiRecency { .. }
        ) && matches!(
            m.shooter,
            ShooterStyle::Static
                | ShooterStyle::WinStay { .. }
                | ShooterStyle::WinShift { .. }
                | ShooterStyle::AvoidDive { .. }
        )
    };
    for id in (0..2_000).chain(TEST_PERSONS..TEST_PERSONS + 2_000) {
        assert!(training(&Mind::person(id)), "{id}");
        let h = Mind::held_out(id);
        assert!(
            matches!(
                h.keeper,
                KeeperStyle::Fictitious { .. }
                    | KeeperStyle::Learner { .. }
                    | KeeperStyle::Counter { .. }
            ) && matches!(h.shooter, ShooterStyle::Markov),
            "{id} {h:?}"
        );
    }
}

#[test]
fn the_style_prior_has_the_frozen_shares() {
    let n = 20_000u64;
    let mut keep = [0u32; 4];
    let mut shoot = [0u32; 4];
    for id in 0..n {
        let m = Mind::person(id);
        keep[match m.keeper {
            KeeperStyle::Static => 0,
            KeeperStyle::Trust { .. } => 1,
            KeeperStyle::Recency { .. } => 2,
            _ => 3,
        }] += 1;
        shoot[match m.shooter {
            ShooterStyle::Static => 0,
            ShooterStyle::WinStay { .. } => 1,
            ShooterStyle::WinShift { .. } => 2,
            _ => 3,
        }] += 1;
    }
    let check = |counts: [u32; 4], shares: [f64; 4]| {
        for (c, p) in counts.iter().zip(shares) {
            let se = (p * (1.0 - p) / n as f64).sqrt();
            let got = f64::from(*c) / n as f64;
            assert!((got - p).abs() < 4.0 * se, "{counts:?} {shares:?}");
        }
    };
    check(keep, [0.35, 0.25, 0.15, 0.25]);
    check(shoot, [0.35, 0.30, 0.15, 0.20]);
}

#[test]
fn a_person_keeps_the_habits_of_stage_four() {
    for id in [0, 1, 7, TEST_PERSONS, TEST_PERSONS + 3] {
        assert_eq!(Mind::person(id).habits(), &Habits::person(id));
        assert_eq!(Mind::held_out(id).habits(), &Habits::person(id));
    }
}

#[test]
fn styles_differ_between_persons_and_repeat_for_one() {
    assert_eq!(Mind::person(5), Mind::person(5));
    let distinct: std::collections::BTreeSet<String> = (0..50)
        .map(|id| format!("{:?}", Mind::person(id).keeper))
        .collect();
    assert!(distinct.len() > 30);
}
