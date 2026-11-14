use feint_ai::Predictor;
use penalty::persons::{Advice, Blind, Habits, Oracle, TEST_PERSONS, digest_at, halves, one_match};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

fn unit(rng: &mut ChaCha8Rng) -> f32 {
    (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
}

#[test]
fn default_habits_draw_exactly_what_the_old_human_drew() {
    let mut old = ChaCha8Rng::seed_from_u64(42);
    let mut new = ChaCha8Rng::seed_from_u64(42);
    let habits = Habits::default();
    for _ in 0..1_000 {
        let pick = (unit(&mut old) * 3.0).min(2.0).floor();
        let corner = (pick - 1.0) * 2.7;
        let t = unit(&mut old);
        let tell = if t < 0.35 {
            1.0
        } else if t < 0.35 + 0.45 {
            -1.0
        } else {
            0.0
        };
        let straightens = unit(&mut old) < 0.25;
        let guesses = unit(&mut old) < 0.5;
        let noise = (unit(&mut old) - 0.5) * 1.0;
        let kick = habits.draw(&mut || unit(&mut new));
        assert_eq!(
            (
                kick.corner,
                kick.tell,
                kick.straightens,
                kick.guesses,
                kick.noise
            ),
            (corner, tell, straightens, guesses, noise)
        );
    }
}

#[test]
fn persons_with_other_corner_weights_kick_other_corners() {
    let left = Habits {
        corner: [8.0, 1.0, 1.0],
        ..Habits::default()
    };
    let right = Habits {
        corner: [1.0, 1.0, 8.0],
        ..Habits::default()
    };
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    let mut count = |h: &Habits| {
        let mut c = [0u32; 3];
        for _ in 0..1_000 {
            let kick = h.draw(&mut || unit(&mut rng));
            c[(kick.corner / 2.7 + 1.0) as usize] += 1;
        }
        c
    };
    let (l, r) = (count(&left), count(&right));
    assert!(
        l[0] > 740 && l[0] < 860,
        "{l:?}: 0.8 of 1 000 within about 4 sd"
    );
    assert!(r[2] > 740 && r[2] < 860, "{r:?}");
    assert!(l[2] < 140 && r[0] < 140);
}

#[test]
fn a_person_is_a_fixed_draw_inside_the_prior() {
    for id in [0, 1, 2, 999, TEST_PERSONS, TEST_PERSONS + 1] {
        let h = Habits::person(id);
        assert_eq!(h, Habits::person(id));
        let total: f32 = h.corner.iter().sum();
        assert!((total - 1.0).abs() < 1e-6, "{h:?}");
        let favourite = h.corner.iter().copied().fold(0.0f32, f32::max);
        assert!((0.4..=0.9).contains(&favourite), "{h:?}");
        assert!(h.honest >= 0.0 && h.fake >= 0.0 && h.honest + h.fake <= 1.0);
        assert!((0.0..=0.5).contains(&h.straighten) && (0.0..=1.0).contains(&h.guess));
        assert!((0.5..=1.5).contains(&h.noise));
    }
    assert_ne!(Habits::person(0), Habits::person(TEST_PERSONS));
}

#[test]
fn the_oracle_answers_from_the_habits() {
    let honest_left = Habits {
        corner: [0.8, 0.1, 0.1],
        honest: 1.0,
        fake: 0.0,
        guess: 1.0,
        follows_lean: false,
        ..Habits::default()
    };
    let o = Oracle::new(honest_left);
    assert_eq!(o.likely_shot(), Some(0));
    assert_eq!(
        o.likely_cover(0),
        Some(2),
        "a guessing keeper dives against the lean"
    );
    assert_eq!(o.likely_cover(2), Some(0));
    assert_eq!(o.likely_real(0), None);
    assert!(!o.knowing_beats_reading());
    assert_eq!(o.their_odds(0), Some(vec![(Some(0), 1.0)]));
    let mixed = Habits {
        corner: [0.5, 0.2, 0.3],
        honest: 0.6,
        fake: 0.3,
        guess: 0.2,
        follows_lean: true,
        ..Habits::default()
    };
    let o = Oracle::new(mixed);
    assert_eq!(o.likely_cover(0), None, "mostly reacts: no guess to cover");
    let odds = o.their_odds(0).unwrap();
    let left = f64::from(0.5f32) * f64::from(0.6f32);
    let right = f64::from(0.3f32) * f64::from(0.3f32);
    let p = |c: u8| {
        odds.iter()
            .find(|(r, _)| *r == Some(c))
            .map_or(0.0, |x| x.1)
    };
    assert!((p(0) - left / (left + right)).abs() < 1e-12);
    assert!((p(2) - right / (left + right)).abs() < 1e-12);
    assert_eq!(p(1), 0.0, "a kick to the centre shows no lean");
    let follows = Habits {
        guess: 0.9,
        follows_lean: true,
        ..mixed
    };
    assert_eq!(Oracle::new(follows).likely_cover(0), Some(0));
}

#[test]
fn blind_advice_knows_nothing() {
    let b = Blind;
    assert_eq!(
        (
            b.likely_shot(),
            b.likely_cover(0),
            b.likely_real(2),
            b.their_odds(0)
        ),
        (None, None, None, None)
    );
    assert!(!b.knowing_beats_reading());
}

#[test]
fn a_harness_match_plays_all_ten_kicks_and_scores_each_role() {
    for id in [TEST_PERSONS, TEST_PERSONS + 1, TEST_PERSONS + 2] {
        let played = one_match(4, Advice::Blind, id);
        assert_eq!(played.kicks.len(), 10, "person {id}");
        let bot_first = id % 2 == 0;
        assert_eq!(played.me == 0, bot_first);
        for (k, kick) in played.kicks.iter().enumerate() {
            assert_eq!(kick.bot_shoots, (k % 2 == 0) == bot_first, "kick {k}");
        }
        let bot_goals = played
            .kicks
            .iter()
            .filter(|k| k.bot_shoots && k.bot_won)
            .count();
        let human_goals = played
            .kicks
            .iter()
            .filter(|k| !k.bot_shoots && !k.bot_won)
            .count();
        let me = usize::from(played.me);
        assert_eq!(
            (bot_goals, human_goals),
            (played.score[me] as usize, played.score[1 - me] as usize)
        );
    }
    let h = halves(4, Advice::Blind, TEST_PERSONS..TEST_PERSONS + 4);
    assert_eq!(h.matches.len(), 4);
    for m in &h.matches {
        let kicks: u32 = m.shooter.iter().chain(&m.keeper).map(|x| x[1]).sum();
        assert_eq!(kicks, 10);
        assert_eq!(m.shooter[0][1] + m.shooter[1][1], 5);
    }
}

#[test]
fn the_halves_statistics_pair_matches_and_persons() {
    let row = |person: u64, first: u32, second: u32| penalty::persons::MatchHalves {
        person,
        shooter: [[first, 5], [second, 5]],
        keeper: [[0, 0], [0, 0]],
    };
    let a = penalty::persons::Halves {
        matches: vec![row(1, 2, 4), row(2, 3, 3), row(3, 1, 4)],
    };
    let (mean, se) = a.difference();
    let d = [0.4, 0.0, 0.6];
    let m = (0.4 + 0.0 + 0.6) / 3.0;
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / 2.0;
    assert!((mean - m).abs() < 1e-12);
    assert!((se - (var / 3.0).sqrt()).abs() < 1e-12);
    let b = penalty::persons::Halves {
        matches: vec![row(3, 2, 2), row(1, 2, 3), row(9, 0, 5)],
    };
    let (paired, _) = a.against(&b);
    assert!(
        (paired - ((0.4 - 0.2) + (0.6 - 0.0)) / 2.0).abs() < 1e-12,
        "only persons in both"
    );
}

#[test]
fn penalty_matches_keep_their_pinned_digests_per_level() {
    let pinned = [
        0x5068_0d58_3177_8ab9_u64,
        0xd85b_b196_d922_3c90,
        0x840b_9408_e7ce_6831,
        0x5e97_008f_01de_313b,
        0x75d3_4b29_f35c_d32f,
    ];
    for (level, want) in (1..=5).zip(pinned) {
        assert_eq!(digest_at(level, 11), want, "level {level}");
    }
}

#[test]
fn a_shooter_who_knows_the_person_scores_more_than_a_blind_one() {
    let persons = TEST_PERSONS..TEST_PERSONS + 150;
    let knowing = halves(4, Advice::Oracle, persons.clone());
    let blind = halves(4, Advice::Blind, persons);
    let shooter = |h: &penalty::persons::Halves| (h.shooter(0) + h.shooter(1)) / 2.0;
    assert!(
        shooter(&knowing) > shooter(&blind) + 0.03,
        "knowing {:.3}, blind {:.3}",
        shooter(&knowing),
        shooter(&blind)
    );
}
