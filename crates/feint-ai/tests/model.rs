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
    let broken: [(Breaker, ProfileError); 6] = [
        (|p| p.game_id = "poker".into(), ProfileError::OtherGame),
        (|p| p.classes = 4, ProfileError::WrongSize),
        (
            |p| p.bite.pop().map(drop).unwrap_or(()),
            ProfileError::WrongSize,
        ),
        (|p| p.shots[0] = f64::NAN, ProfileError::BadCount),
        (|p| p.mine[1] = -1.0, ProfileError::BadCount),
        (|p| p.fake_tries = 1.0e9, ProfileError::BadCount),
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
