use feint_ai::{OpponentModel, Profile, ProfileError};

type Breaker = fn(&mut Profile);

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn one_bite_is_laplace_smoothed_over_the_classes() {
    let mut m = OpponentModel::new(3, 0.0);
    m.record_attempt(Some(0), Some(2), Some(0), true, true);
    assert!(close(m.bite_probability(0, 0), 2.0 / 4.0));
    assert!(close(m.bite_probability(0, 1), 1.0 / 4.0));
    assert!(
        close(m.bite_probability(1, 1), 1.0 / 3.0),
        "an unseen row is uniform"
    );
}

#[test]
fn every_event_decays_the_old_ones_by_adaptation() {
    for (adaptation, older, newer) in [(0.0, 1.0, 1.0), (1.0, 0.9, 1.0)] {
        let mut m = OpponentModel::new(3, adaptation);
        m.record_attempt(Some(0), None, Some(0), false, false);
        m.record_attempt(Some(0), None, Some(2), false, false);
        let total = older + newer + 3.0;
        assert!(
            close(m.bite_probability(0, 0), (older + 1.0) / total),
            "adaptation {adaptation}"
        );
        assert!(
            close(m.bite_probability(0, 2), (newer + 1.0) / total),
            "adaptation {adaptation}"
        );
    }
}

#[test]
fn rows_abstain_below_three_events() {
    let mut m = OpponentModel::new(3, 0.3);
    for _ in 0..2 {
        m.record_attempt(Some(1), Some(1), Some(2), false, false);
    }
    assert_eq!(m.likely_cover(1), None);
    assert_eq!(m.likely_real(1), None);
    m.record_attempt(Some(1), Some(1), Some(2), false, false);
    assert_eq!(m.likely_cover(1), Some(2));
    assert_eq!(m.likely_real(1), Some(1));
    assert_eq!(m.likely_cover(0), None, "another row has no evidence");
}

#[test]
fn shots_and_fakes_are_counted() {
    let mut m = OpponentModel::new(3, 0.0);
    assert_eq!(m.likely_shot(), None);
    assert_eq!(m.fake_win_rate(), None);
    for class in [2, 2, 0] {
        m.record_shot(class);
    }
    assert_eq!(m.likely_shot(), Some(2));
    m.record_attempt(Some(0), Some(2), Some(0), true, true);
    m.record_attempt(Some(2), Some(0), Some(0), true, false);
    m.record_attempt(Some(1), Some(1), Some(1), false, false);
    assert!(close(m.fake_win_rate().unwrap(), 0.5));
}

#[test]
fn reset_forgets_everything() {
    let mut m = OpponentModel::new(3, 0.0);
    for _ in 0..3 {
        m.record_shot(1);
        m.record_attempt(Some(0), Some(0), Some(0), true, true);
    }
    m.reset();
    assert_eq!(m.likely_shot(), None);
    assert_eq!(m.likely_cover(0), None);
    assert_eq!(m.fake_win_rate(), None);
}

fn trained() -> OpponentModel {
    let mut m = OpponentModel::new(3, 0.6);
    for (s, r, c) in [(0, 2, 0), (0, 2, 0), (2, 2, 1), (0, 0, 2)] {
        m.record_attempt(Some(s), Some(r), Some(c), s != r, c != r);
    }
    m.record_shot(1);
    m.record_shot_after(2, &[0]);
    m
}

#[test]
fn profile_round_trips() {
    let m = trained();
    let profile = m.export("airhockey");
    assert_eq!(profile.game_id, "airhockey");
    assert_eq!(profile.classes, 3);
    let mut back = OpponentModel::new(3, 0.6);
    back.import(&profile, "airhockey").expect("a valid profile");
    assert_eq!(back, m);
}

#[test]
fn profile_import_rejects_bad_data() {
    let good = trained().export("airhockey");
    let mut target = OpponentModel::new(3, 0.6);
    let broken: [(Breaker, ProfileError); 9] = [
        (|p| p.game_id = "poker".into(), ProfileError::OtherGame),
        (|p| p.classes = 4, ProfileError::WrongSize),
        (
            |p| p.bite.pop().map(drop).unwrap_or(()),
            ProfileError::WrongSize,
        ),
        (|p| p.shots[0] = f64::NAN, ProfileError::BadCount),
        (|p| p.mine[1] = -1.0, ProfileError::BadCount),
        (|p| p.fake_tries = 1.0e9, ProfileError::BadCount),
        (
            |p| p.theirs.pop().map(drop).unwrap_or(()),
            ProfileError::WrongSize,
        ),
        (|p| p.theirs_events.clear(), ProfileError::WrongSize),
        (|p| p.theirs[3] = f64::INFINITY, ProfileError::BadCount),
    ];
    for (i, (breaks, why)) in broken.iter().enumerate() {
        let mut p = good.clone();
        breaks(&mut p);
        assert_eq!(target.import(&p, "airhockey"), Err(*why), "case {i}");
    }
    assert_eq!(
        target,
        OpponentModel::new(3, 0.6),
        "a refused profile changed the model"
    );
}

#[test]
fn ties_assume_the_opponent_bites() {
    let mut m = OpponentModel::new(3, 0.0);
    for covered in [0, 2, 0, 2] {
        m.record_attempt(Some(2), Some(0), Some(covered), true, covered != 0);
    }
    assert_eq!(
        m.likely_cover(2),
        Some(2),
        "a tie between covering 0 and the shown 2"
    );
    let mut mix = OpponentModel::new(3, 0.0);
    for real in [0, 2, 0, 2] {
        mix.record_attempt(Some(2), Some(real), None, real != 2, false);
    }
    assert_eq!(mix.likely_real(2), Some(2));
}

#[test]
fn imported_event_counts_without_counts_behind_them_do_not_steer() {
    let mut m = OpponentModel::new(3, 0.0);
    let mut p = m.export("g");
    p.bite_events = vec![u32::MAX; 3];
    p.mine_events = vec![u32::MAX; 3];
    p.shot_events = u32::MAX;
    m.import(&p, "g").unwrap();
    assert_eq!(m.likely_cover(0), None);
    assert_eq!(m.likely_real(2), None);
    assert_eq!(m.likely_shot(), None);
    m.record_attempt(Some(0), Some(2), Some(1), true, true);
    assert_eq!(m.likely_cover(0), Some(1), "real evidence still counts");
}

#[test]
fn a_shot_outside_the_classes_is_no_event() {
    let mut m = OpponentModel::new(3, 1.0);
    m.record_shot(0);
    m.record_shot(7);
    assert_eq!(m.export("g").shots[0], 1.0);
    assert_eq!(m.export("g").shot_events, 1);
}

#[test]
fn a_model_full_to_the_cap_still_imports_its_own_export() {
    let mut m = OpponentModel::new(3, 0.0);
    for _ in 0..1_000_001 {
        m.record_shot(1);
    }
    let p = m.export("g");
    assert_eq!(OpponentModel::new(3, 0.0).import(&p, "g"), Ok(()));
}

#[test]
fn an_attempt_with_nothing_to_record_does_not_decay() {
    let mut m = OpponentModel::new(3, 1.0);
    m.record_shot(0);
    m.record_attempt(None, None, Some(9), false, false);
    assert_eq!(m.export("g").shots[0], 1.0);
}

#[test]
fn the_theory_that_named_the_cover_more_often_wins() {
    let mut fresh = OpponentModel::new(3, 1.0);
    assert!(
        !fresh.knowing_beats_reading(),
        "with no record, what was seen comes first"
    );
    for _ in 0..3 {
        fresh.record_attempt(Some(0), Some(0), Some(1), false, false);
    }
    let mut reader = fresh.clone();
    for _ in 0..6 {
        reader.record_attempt(Some(0), Some(0), Some(1), false, false);
    }
    assert!(
        !reader.knowing_beats_reading(),
        "a keeper who keeps covering the other side is read, not known"
    );
    let mut learner = fresh.clone();
    for _ in 0..6 {
        learner.record_attempt(Some(0), Some(0), Some(0), false, false);
    }
    assert!(
        learner.knowing_beats_reading(),
        "a keeper who learned the striker's habit is named by the habit first"
    );
    let mut blind = fresh;
    for _ in 0..6 {
        blind.record_attempt(Some(0), Some(0), None, false, false);
    }
    assert!(
        !blind.knowing_beats_reading(),
        "attempts nobody answered say nothing"
    );
}

#[test]
fn a_loaded_profile_starts_the_theories_afresh() {
    let mut learner = OpponentModel::new(3, 1.0);
    for _ in 0..3 {
        learner.record_attempt(Some(0), Some(0), Some(1), false, false);
    }
    for _ in 0..6 {
        learner.record_attempt(Some(0), Some(0), Some(0), false, false);
    }
    assert!(learner.knowing_beats_reading());
    let other = OpponentModel::new(3, 1.0).export("pick");
    learner.import(&other, "pick").unwrap();
    assert!(
        !learner.knowing_beats_reading(),
        "the theory that fitted the last opponent was kept for the next one"
    );
}

#[test]
fn the_opponents_signals_are_tied_to_where_it_really_went() {
    let mut m = OpponentModel::new(3, 0.0);
    let likeliest = |m: &OpponentModel, s: u8| {
        m.their_odds(s).map(|odds| {
            odds.into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k)
        })
    };
    assert_eq!(m.their_odds(0), None);
    for _ in 0..2 {
        m.record_shot_after(2, &[0]);
    }
    assert_eq!(m.their_odds(0), None, "two kicks are not evidence");
    m.record_shot_after(2, &[0, 1]);
    assert_eq!(
        likeliest(&m, 0),
        Some(Some(Some(2))),
        "showing left, it kicks right"
    );
    assert_eq!(m.their_odds(1), None);
    assert_eq!(m.their_odds(2), None, "nothing was shown on the right");
    assert_eq!(m.likely_shot(), Some(2), "every one of them is also a shot");
    m.record_shot_after(0, &[]);
    assert_eq!(
        m.export("pick").theirs_events,
        vec![3, 1, 0],
        "a shot with nothing shown ties no signal"
    );
    m.record_shot_after(1, &[7]);
    assert_eq!(m.export("pick").theirs_events, vec![3, 1, 0]);
}

#[test]
fn a_profile_from_before_the_signal_table_still_loads() {
    let mut old = trained().export("airhockey");
    old.theirs.clear();
    old.theirs_events.clear();
    let mut target = trained();
    target
        .import(&old, "airhockey")
        .expect("an older profile loads");
    let back = target.export("airhockey");
    assert_eq!(back.theirs, vec![0.0; 9]);
    assert_eq!(back.theirs_events, vec![0; 3]);
    assert_eq!(back.bite, old.bite);
}

#[test]
fn old_signals_fade_like_every_other_count() {
    let mut m = OpponentModel::new(3, 1.0);
    m.record_shot_after(2, &[0]);
    m.record_shot(1);
    assert!(close(m.export("pick").theirs[2], 0.9));
}

#[test]
fn the_odds_of_a_signal_are_smoothed_and_drop_what_is_too_rare() {
    let mut m = OpponentModel::new(3, 0.0);
    for _ in 0..2 {
        m.record_shot_after(2, &[0]);
    }
    assert_eq!(m.their_odds(0), None, "two kicks are not evidence");
    m.record_shot_after(0, &[0]);
    let odds = m.their_odds(0).unwrap();
    assert_eq!(
        odds.iter().map(|&(k, _)| k).collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2), None]
    );
    let p: Vec<f64> = odds.iter().map(|&(_, p)| p).collect();
    assert!(
        close(p[0], 2.0 / 7.0)
            && close(p[1], 1.0 / 7.0)
            && close(p[2], 3.0 / 7.0)
            && close(p[3], 1.0 / 7.0),
        "{p:?}"
    );
    for _ in 0..30 {
        m.record_shot_after(2, &[0]);
    }
    let odds = m.their_odds(0).unwrap();
    assert_eq!(
        odds.iter().map(|&(k, _)| k).collect::<Vec<_>>(),
        vec![Some(0), Some(2)],
        "an unseen class under the floor is dropped, a seen one stays however rare: {odds:?}"
    );
    assert!(odds[0].1 < 0.1, "{odds:?}");
    assert!(
        close(odds[0].1 + odds[1].1, 1.0),
        "what is kept sums to one"
    );
    assert_eq!(m.their_odds(7), None);
}

#[test]
fn a_show_without_a_shot_is_an_outcome_of_its_own() {
    let mut m = OpponentModel::new(3, 0.0);
    m.record_bluff(&[]);
    assert_eq!(
        m.export("pick").theirs_events,
        vec![0, 0, 0],
        "nothing shown, nothing learned"
    );
    for _ in 0..3 {
        m.record_bluff(&[2, 9]);
    }
    let odds = m.their_odds(2).unwrap();
    let total = 3.0 + 3.0 + 1.0;
    assert_eq!(odds.len(), 4, "{odds:?}");
    let none = odds.iter().find(|(k, _)| k.is_none()).unwrap().1;
    assert!(close(none, 4.0 / total), "{odds:?}");
    assert_eq!(m.export("pick").bluffs, vec![0.0, 0.0, 3.0]);
    assert_eq!(m.likely_shot(), None, "a bluff is not a shot");
    let mut decaying = OpponentModel::new(3, 1.0);
    decaying.record_bluff(&[0]);
    decaying.record_shot(1);
    assert!(close(decaying.export("pick").bluffs[0], 0.9));
}

#[test]
fn a_profile_from_before_the_bluffs_still_loads() {
    let mut m = trained();
    m.record_bluff(&[1]);
    let mut old = m.export("airhockey");
    old.bluffs.clear();
    let mut target = trained();
    target.record_bluff(&[0]);
    target
        .import(&old, "airhockey")
        .expect("an older profile loads");
    assert_eq!(target.export("airhockey").bluffs, vec![0.0; 3]);
    let mut wrong = m.export("airhockey");
    wrong.bluffs.pop();
    assert_eq!(
        target.import(&wrong, "airhockey"),
        Err(ProfileError::WrongSize)
    );
    let mut bad = m.export("airhockey");
    bad.bluffs[0] = f64::NAN;
    assert_eq!(
        target.import(&bad, "airhockey"),
        Err(ProfileError::BadCount)
    );
}

#[test]
fn the_theories_travel_with_the_players_profile() {
    let mut learner = OpponentModel::new(3, 1.0);
    for _ in 0..3 {
        learner.record_attempt(Some(0), Some(0), Some(1), false, false);
    }
    for _ in 0..6 {
        learner.record_attempt(Some(0), Some(0), Some(0), false, false);
    }
    assert!(learner.knowing_beats_reading());
    let profile = learner.export("pick");
    assert!(profile.knowing > profile.reading, "{profile:?}");
    let mut next_match = OpponentModel::new(3, 1.0);
    next_match.import(&profile, "pick").unwrap();
    assert!(
        next_match.knowing_beats_reading(),
        "the same player's theory was forgotten between matches"
    );
    let back = feint_ai::decode_profile(&feint_ai::encode_profile(&profile)).unwrap();
    assert_eq!(back, profile);
    let mut bad = profile.clone();
    bad.knowing = -1.0;
    assert_eq!(next_match.import(&bad, "pick"), Err(ProfileError::BadCount));
}
