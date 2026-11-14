use boxing::{Body, Bout, BoxingPlaybook, Height, Kind, Params, Stage};
use feint_ai::{Choice, Cue, Playbook};

fn bout(gap: f64) -> Bout {
    let body = Body {
        speed: 1.5,
        hand: 1.0,
    };
    let mut b = Bout::new(Params::default(), [body; 2], 0);
    b.fighters[0].x = -gap / 2.0;
    b.fighters[1].x = gap / 2.0;
    b
}

fn choices(s: &Bout, repertoire: u8) -> Vec<Choice> {
    let mut out = Vec::new();
    BoxingPlaybook::new(Params::default()).options(s, 0, repertoire, &Cue::default(), &mut out);
    out
}

fn values(s: &Bout, c: &Choice, elapsed: u64) -> Vec<f32> {
    BoxingPlaybook::new(Params::default())
        .act(s, 0, c, elapsed, [0.0, 0.0])
        .values
        .unwrap()
}

fn feint(s: &Bout, shown: Height) -> Choice {
    choices(s, 2)
        .into_iter()
        .find(|c| c.deceptive && c.shown == Some(shown.class()))
        .unwrap()
}

#[test]
fn the_repertoire_grows_with_the_level() {
    let s = bout(0.8);
    let ids = |r| choices(&s, r).iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(0), vec![2, 11]);
    assert_eq!(ids(1), vec![1, 2, 10, 11, 12, 13]);
    assert_eq!(ids(2), vec![1, 2, 10, 11, 12, 13, 20, 21]);
    let feints: Vec<Choice> = choices(&s, 2).into_iter().filter(|c| c.deceptive).collect();
    for f in feints {
        assert_ne!(
            f.shown, f.real,
            "a feint shows one height and means the other"
        );
    }
}

#[test]
fn a_shot_is_thrown_only_from_range() {
    let jab = |s: &Bout| choices(s, 1).into_iter().find(|c| c.id == 11).unwrap();
    let far = bout(1.5);
    assert_eq!(values(&far, &jab(&far), 0)[2], 0.0);
    let near = bout(0.7);
    let v = values(&near, &jab(&near), 0);
    assert_eq!(v[2], 1.0, "a jab");
    assert_eq!(v[3], 1.0, "high");
}

#[test]
fn a_feint_cancels_once_the_guard_follows_the_shown_hook() {
    let p = Params::default();
    let mut s = bout(0.75);
    let c = feint(&s, Height::High);
    s.fighters[0].stage = Stage::WindUp {
        kind: Kind::Hook,
        height: Height::High,
        left: p.hook_windup - 30,
    };
    s.fighters[1].guard = Height::Low;
    assert_eq!(values(&s, &c, 30)[4], 0.0, "no bite, the hook goes on");
    s.fighters[1].guard = Height::High;
    assert_eq!(values(&s, &c, 30)[4], 1.0, "they bit: cancel");
    s.fighters[0].stage = Stage::WindUp {
        kind: Kind::Hook,
        height: Height::High,
        left: p.hook_windup - 10,
    };
    assert_eq!(
        values(&s, &c, 10)[4],
        0.0,
        "shown too briefly to cancel yet"
    );
}

#[test]
fn after_the_cancel_the_feint_jabs_the_other_height() {
    let mut s = bout(0.75);
    let c = feint(&s, Height::High);
    s.fighters[0].stage = Stage::Recover { left: 10 };
    s.fighters[0].cancelled = true;
    let v = values(&s, &c, 120);
    assert_eq!([v[2], v[3]], [1.0, -1.0], "a low jab");
    let s = bout(0.6);
    let v = values(&s, &c, 0);
    assert_eq!([v[2], v[3]], [2.0, 1.0], "it opens with the high hook");
}

#[test]
fn the_shown_hook_is_thrown_from_where_it_would_land() {
    let p = Params::default();
    let s = bout((p.hook_reach + p.jab_reach) / 2.0);
    let c = feint(&s, Height::High);
    let v = values(&s, &c, 0);
    assert_eq!(v[2], 0.0, "out of hook reach it steps in first");
    let gap = f64::from(v[0]) - s.fighters[0].x;
    assert!(gap > 0.0, "it steps forward");
    let theirs = s.fighters[1].x;
    assert!(
        theirs - f64::from(v[0]) <= p.hook_reach,
        "to a point inside hook reach"
    );
}

#[test]
fn a_slow_fighter_still_opens_the_feint_with_the_hook() {
    let mut s = bout(1.6);
    s.bodies[0].speed = 1.2;
    let c = feint(&s, Height::High);
    s.fighters[0].x = s.fighters[1].x - 0.6;
    let v = values(&s, &c, 400);
    assert_eq!(
        [v[2], v[3]],
        [2.0, 1.0],
        "the walk took long, but no cancel yet"
    );
    s.fighters[0].cancelled = true;
    let v = values(&s, &c, 400);
    assert_eq!([v[2], v[3]], [1.0, -1.0], "after the cancel it jabs low");
}

#[test]
fn the_feint_plan_does_not_depend_on_when_the_choice_was_made() {
    let mut s = bout(0.6);
    let held = feint(&s, Height::High);
    s.fighters[0].cancelled = true;
    s.fighters[0].feints += 1;
    let fresh = feint(&s, Height::High);
    assert_eq!(values(&s, &held, 0), values(&s, &fresh, 0));
}
