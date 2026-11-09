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
        u32::from(t >= 150) + u32::from(t >= 900) + u32::from(t >= 1200),
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
