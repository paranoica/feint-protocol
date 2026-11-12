use airhockey::Vec2;
use airhockey::analysis::Frame;
use feint_compare::detect::Shot;
use feint_compare::trained::leaned_to_shown;

fn frames(defender_y_before: f64, defender_y_at: f64, defender: usize) -> Vec<Frame> {
    let at = |tick: u64, y: f64| {
        let mut mallets = [Vec2::new(-0.5, 0.0), Vec2::new(0.9, 0.0)];
        mallets[defender].y = y;
        Frame {
            tick,
            puck: Vec2::new(0.0, 0.0),
            mallets,
            score: [0, 0],
        }
    };
    vec![at(99, defender_y_before), at(100, defender_y_at)]
}

fn shot(player: u8, shown: Option<u8>) -> Shot {
    Shot {
        player,
        from: 0,
        contact: 100,
        shown,
        realised: 0,
        scored: false,
    }
}

#[test]
fn the_defender_leans_to_the_shown_corner_at_contact() {
    assert!(leaned_to_shown(&frames(-0.1, 0.1, 1), &shot(0, Some(2))));
    assert!(!leaned_to_shown(&frames(0.1, -0.1, 1), &shot(0, Some(2))));
    assert!(!leaned_to_shown(&frames(0.1, 0.0, 1), &shot(0, Some(2))));
    assert!(leaned_to_shown(&frames(0.0, -0.1, 1), &shot(0, Some(0))));
}

#[test]
fn seat_one_attacks_the_mirrored_table() {
    assert!(leaned_to_shown(&frames(0.0, -0.1, 0), &shot(1, Some(2))));
    assert!(!leaned_to_shown(&frames(0.0, 0.1, 0), &shot(1, Some(2))));
}

#[test]
fn a_shot_with_nothing_shown_never_counts() {
    assert!(!leaned_to_shown(&frames(0.1, 0.1, 1), &shot(0, None)));
    assert!(!leaned_to_shown(&[], &shot(0, Some(2))));
}
