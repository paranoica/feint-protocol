use feint_compare::attacks::{Ledger, OUTCOME_TICKS};

const STRIKE: u8 = 1;
const DEFEND: u8 = 2;

#[test]
fn a_goal_goes_to_the_latest_attack_only_and_only_inside_its_window() {
    let mut l = Ledger::default();
    l.start(0);
    l.chose(1, STRIKE, false, Some(1));
    l.end(100);
    l.start(200);
    l.chose(201, STRIKE, false, Some(1));
    l.end(300);
    l.goal(320, true);
    l.start(1000);
    l.chose(1001, STRIKE, false, Some(1));
    l.end(1100);
    l.goal(1100 + OUTCOME_TICKS + 1, true);
    l.goal(1101, false);
    let s = l.finish();
    assert_eq!(s.honest.attempts, 3);
    assert_eq!(s.honest.scored, 1, "one goal, one attack: {s:?}");
    assert_eq!(s.honest.conceded, 1);
    assert_eq!(s.honest.ticks, 300);
}

#[test]
fn defending_spells_are_not_attacks() {
    let mut l = Ledger::default();
    l.start(0);
    l.chose(1, DEFEND, false, None);
    l.end(50);
    l.goal(60, false);
    let s = l.finish();
    assert_eq!((s.honest.attempts, s.fake.attempts), (0, 0));
}

#[test]
fn a_bite_is_the_first_answer_after_the_fake_and_matches_the_last_shown_side() {
    let mut l = Ledger::default();
    l.start(0);
    l.answered(0);
    assert!(l.chose(1, STRIKE, true, None), "a stutter starts a fake");
    assert!(!l.chose(1, STRIKE, true, None), "the same fake goes on");
    l.chose(1, STRIKE, true, Some(0));
    l.answered(0);
    l.answered(2);
    l.end(80);
    l.goal(90, true);

    l.start(200);
    l.chose(201, STRIKE, true, Some(2));
    l.answered(0);
    l.end(260);

    l.start(400);
    l.chose(401, STRIKE, true, None);
    l.end(420);

    l.start(600);
    l.chose(601, STRIKE, true, Some(0));
    l.end(650);
    let s = l.finish();
    assert_eq!(s.fake.attempts, 4);
    assert_eq!(s.fake.read, 2, "{s:?}");
    assert_eq!(s.fake.bit, 1);
    assert_eq!(s.fake.scored_bit, 1);
    assert_eq!(s.fake.scored_unbit, 0);
    assert_eq!(s.unread, 1, "a shown fake nobody answered");
    assert_eq!(s.fake.scored, 1);
}

#[test]
fn a_goal_on_the_tick_a_new_spell_starts_belongs_to_the_old_one_and_so_does_the_old_choice() {
    let mut l = Ledger::default();
    l.start(0);
    l.chose(1, STRIKE, true, Some(0));
    l.end(100);
    l.start(150);
    l.goal(150, false);
    l.chose(150, STRIKE, true, Some(2));
    l.chose(151, DEFEND, false, None);
    l.end(200);
    let s = l.finish();
    assert_eq!(
        s.fake.attempts, 1,
        "the stale fake does not tag the new spell: {s:?}"
    );
    assert_eq!(
        s.fake.conceded, 1,
        "the goal at the switch belongs to the old spell"
    );
    assert_eq!(s.honest.attempts, 0);
}
