use airhockey::analysis::{Attempt, Frame, Tally, attempts, tally};
use airhockey::{Params, Vec2};
use feint_protocol::ActionT;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn at(t: u64) -> Frame {
    let away = v(0.5, 0.0);
    let rest = v(-0.4, 0.0);
    let touch = v(-0.49, 0.0);
    let behind = v(-0.6, 0.0);
    let hit = |at: u64| if t == at { rest } else { away };
    let (puck, m0, m1) = match t {
        0..=60 => (rest, v(-0.53, 0.10), v(0.9, 0.0)),
        61..=75 => (rest, behind, v(0.9, if t < 75 { 0.0 } else { -0.06 })),
        76..=299 => (
            if t == 76 || t == 110 { rest } else { away },
            touch,
            v(0.9, if t == 76 { -0.06 } else { 0.06 }),
        ),
        300..=320 => (rest, v(-0.3, 0.12), v(0.9, 0.0)),
        321..=349 => (rest, behind, v(0.9, 0.0)),
        350..=499 => (
            if t == 350 { rest } else { v(0.5, -0.2) },
            touch,
            v(0.9, 0.0),
        ),
        500..=560 => (
            rest,
            v(-0.53, -0.10),
            v(0.9, if t < 520 { 0.0 } else { -0.06 }),
        ),
        561..=579 => (rest, behind, v(0.9, -0.06)),
        580..=699 => (hit(580), touch, v(0.9, -0.06)),
        700..=739 => (
            v(0.4, 0.0),
            v(-0.9, if t < 720 { 0.0 } else { 0.06 }),
            v(0.53, -0.10),
        ),
        740..=949 => (
            if t == 740 { v(0.4, 0.0) } else { v(-0.5, 0.0) },
            v(-0.9, 0.06),
            v(0.49, 0.0),
        ),
        950..=999 => (rest, behind, v(0.9, 0.0)),
        1000..=1099 => (hit(1000), touch, v(0.9, 0.0)),
        1100..=1119 => (rest, behind, v(0.9, 0.0)),
        1120..=1300 => (hit(1120), touch, v(0.9, 0.0)),
        _ => (rest, behind, v(0.9, 0.0)),
    };
    let score = [
        u32::from(t >= 100) + u32::from(t >= 900) + u32::from(t >= 1200),
        u32::from(t >= 780),
    ];
    Frame {
        tick: t,
        puck,
        mallets: [m0, m1],
        score,
    }
}

fn run(
    player: u8,
    ticks: std::ops::RangeInclusive<u64>,
    intent: u8,
    deceptive: bool,
) -> Vec<ActionT> {
    ticks
        .step_by(4)
        .map(|tick| {
            let mut a = ActionT::default();
            a.tick = tick;
            a.player = player;
            a.intent = intent;
            a.deceptive = deceptive;
            a
        })
        .collect()
}

#[test]
fn the_analyser_counts_shifts_and_goals_from_logged_fields() {
    let frames: Vec<Frame> = (0..=1400).map(at).collect();
    let mut actions = Vec::new();
    actions.extend(run(0, 0..=72, 1, true));
    actions.extend(run(0, 76..=100, 1, false));
    actions.extend(run(0, 104..=124, 2, false));
    actions.extend(run(0, 300..=400, 1, false));
    actions.extend(run(0, 500..=600, 1, true));
    actions.extend(run(1, 700..=760, 1, true));
    actions.extend(run(0, 952..=992, 1, true));
    actions.extend(run(0, 1100..=1140, 1, false));
    actions.sort_by_key(|a| a.tick);

    let found = attempts(&Params::default(), &frames, &actions);
    let of = |player: u8, start: u64| -> Attempt {
        *found
            .iter()
            .find(|a| a.player == player && a.start == start)
            .expect("attempt")
    };
    let a = of(0, 0);
    assert!(
        a.deceptive && a.shown == Some(0) && a.contact == Some(76) && a.shifted && a.scored,
        "contact on the tick of the next action: {a:?}"
    );
    let tail = of(0, 76);
    assert!(tail.contact.is_none() && !tail.scored, "{tail:?}");
    let b = of(0, 300);
    assert!(
        !b.deceptive && b.shown.is_none() && b.contact == Some(350) && !b.shifted && !b.scored,
        "{b:?}"
    );
    let c = of(0, 500);
    assert!(
        c.shown == Some(2) && !c.shifted && !c.scored,
        "wrong-way shift, late goal: {c:?}"
    );
    let d = of(1, 700);
    assert!(
        d.shown == Some(0) && d.shifted && d.scored,
        "player 1 frame: {d:?}"
    );
    let e = of(0, 952);
    assert!(
        e.deceptive && e.shown.is_none() && e.contact == Some(1000) && !e.scored,
        "the goal belongs to the later touch: {e:?}"
    );
    let f = of(0, 1100);
    assert!(!f.deceptive && f.scored, "{f:?}");
    assert_eq!(
        found.len(),
        7,
        "the defend run is not an attempt: {found:?}"
    );

    let zero = Tally {
        fakes: 2,
        shifted: 1,
        fake_goals: 1,
        honest: 2,
        honest_goals: 1,
    };
    assert_eq!(tally(&found, 0), zero);
    let one = Tally {
        fakes: 1,
        shifted: 1,
        fake_goals: 1,
        honest: 0,
        honest_goals: 0,
    };
    assert_eq!(tally(&found, 1), one);
    assert!(
        attempts(&Params::default(), &[], &actions)
            .iter()
            .all(|a| !a.scored && a.shown.is_none() && a.contact.is_none())
    );
    assert!(attempts(&Params::default(), &frames, &[]).is_empty());
}

#[test]
fn ticks_at_the_end_of_time_and_unsorted_logs_are_handled() {
    let late = run(0, u64::MAX - 10..=u64::MAX - 10, 1, true);
    let found = attempts(&Params::default(), &[], &late);
    assert_eq!(found.len(), 1);
    assert!(found[0].contact.is_none() && !found[0].scored);

    let frames: Vec<Frame> = (0..=1400).map(at).collect();
    let mut actions = run(0, 0..=72, 1, true);
    actions.extend(run(0, 300..=400, 1, false));
    let sorted = attempts(&Params::default(), &frames, &actions);
    let mut reversed_frames = frames.clone();
    reversed_frames.reverse();
    let mut reversed_actions = actions.clone();
    reversed_actions.reverse();
    assert_eq!(
        attempts(&Params::default(), &reversed_frames, &reversed_actions),
        sorted
    );
}

fn quiet(t: u64, puck: Vec2, m0: Vec2, m1: Vec2, score: [u32; 2]) -> Frame {
    Frame {
        tick: t,
        puck,
        mallets: [m0, m1],
        score,
    }
}

#[test]
fn contact_is_looked_for_until_the_next_action_however_late() {
    let frames: Vec<Frame> = (0..=200)
        .map(|t| {
            let m0 = if t == 50 { v(-0.49, 0.0) } else { v(-0.8, 0.0) };
            quiet(t, v(-0.4, 0.0), m0, v(0.9, 0.0), [0, 0])
        })
        .collect();
    let mut actions = run(0, 0..=0, 1, false);
    actions.extend(run(0, 100..=100, 2, false));
    let found = attempts(&Params::default(), &frames, &actions);
    assert_eq!(found[0].contact, Some(50));
}

#[test]
fn a_goal_is_credited_only_when_the_attempt_touched_last() {
    let touch = v(-0.49, 0.0);
    let score = |t: u64| [u32::from(t >= 100), 0];
    let log = |later: Option<(u64, usize)>| -> Vec<Frame> {
        (0..=200)
            .map(|t| {
                let mut f = quiet(t, v(-0.4, 0.0), v(-0.8, 0.0), v(0.9, 0.0), score(t));
                if t == 10 {
                    f.mallets[0] = touch;
                }
                if let Some((at, who)) = later
                    && t == at
                {
                    f.mallets[who] = v(-0.4, 0.09);
                }
                f
            })
            .collect()
    };
    let actions = run(0, 0..=8, 1, true);
    let credited = |frames: &[Frame]| attempts(&Params::default(), frames, &actions)[0].scored;
    assert!(credited(&log(None)), "the fake touched last");
    assert!(
        !credited(&log(Some((60, 1)))),
        "the opponent touched it after the fake"
    );
    assert!(
        !credited(&log(Some((60, 0)))),
        "a later touch outside the attempt"
    );
}

fn scripted(goal_at: u64, touches: &[(u64, usize)]) -> Vec<Frame> {
    (0..=500)
        .map(|t| {
            let mut f = quiet(
                t,
                v(-0.4, 0.0),
                v(-0.8, 0.0),
                v(0.9, 0.0),
                [u32::from(t >= goal_at), 0],
            );
            for &(at, who) in touches {
                if t == at {
                    f.mallets[who] = if who == 0 {
                        v(-0.49, 0.0)
                    } else {
                        v(-0.4, 0.09)
                    };
                }
            }
            f
        })
        .collect()
}

#[test]
fn touches_are_episodes_owned_by_the_toucher() {
    let held: Vec<(u64, usize)> = (76..=80).map(|t| (t, 0)).collect();
    let mut actions = run(0, 0..=72, 1, true);
    actions.extend(run(0, 78..=100, 1, false));
    let found = attempts(&Params::default(), &scripted(120, &held), &actions);
    let (fake, tail) = (found[0], found[1]);
    assert!(fake.contact == Some(76) && fake.scored, "{fake:?}");
    assert!(
        tail.contact.is_none() && !tail.scored,
        "the tail started inside the fake's touch: {tail:?}"
    );

    let honest = run(0, 0..=50, 1, false);
    let found = attempts(
        &Params::default(),
        &scripted(500, &[(20, 1), (30, 0)]),
        &honest,
    );
    assert_eq!(
        found[0].contact,
        Some(30),
        "the opponent's touch is not the attempt's contact"
    );

    let both = attempts(
        &Params::default(),
        &scripted(100, &[(10, 0), (10, 1)]),
        &run(0, 0..=8, 1, true),
    );
    assert!(
        !both[0].scored,
        "a touch shared with the opponent credits no one"
    );

    let late = attempts(
        &Params::default(),
        &scripted(400, &[(10, 0)]),
        &run(0, 0..=8, 1, true),
    );
    assert!(
        late[0].contact == Some(10) && !late[0].scored,
        "a goal 390 ticks after the touch: {:?}",
        late[0]
    );
}

#[test]
fn the_last_attempt_in_a_log_looks_for_contact_to_the_end() {
    let frames: Vec<Frame> = (0..=200)
        .map(|t| {
            let m0 = if t == 50 { v(-0.49, 0.0) } else { v(-0.8, 0.0) };
            quiet(t, v(-0.4, 0.0), m0, v(0.9, 0.0), [0, 0])
        })
        .collect();
    let found = attempts(&Params::default(), &frames, &run(0, 0..=0, 1, false));
    assert_eq!(found[0].contact, Some(50));
}
