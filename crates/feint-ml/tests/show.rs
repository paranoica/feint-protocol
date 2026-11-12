use airhockey::{Params, Vec2};
use feint_ml::show::{SHOW_HOLD_TICKS, shown_side};

fn behind(puck: Vec2, slope: f64, distance: f64) -> Vec2 {
    let norm = (1.0 + slope * slope).sqrt();
    puck - Vec2::new(distance / norm, slope * distance / norm)
}

#[test]
fn a_wind_up_toward_a_corner_shows_that_corner() {
    let p = Params::default();
    let puck = Vec2::new(0.0, 0.0);
    assert_eq!(shown_side(&p, puck, behind(puck, 0.3, 0.3), 0), Some(2));
    assert_eq!(shown_side(&p, puck, behind(puck, -0.3, 0.3), 0), Some(0));
}

#[test]
fn the_aim_must_land_between_0_15_and_0_45_off_centre() {
    let p = Params::default();
    let puck = Vec2::new(0.0, 0.0);
    for (aim, expected) in [(0.14, None), (0.16, Some(2)), (0.44, Some(2)), (0.46, None)] {
        assert_eq!(
            shown_side(&p, puck, behind(puck, aim, 0.3), 0),
            expected,
            "aim {aim}"
        );
    }
}

#[test]
fn the_mallet_must_be_behind_within_60_degrees_of_the_aim() {
    let p = Params::default();
    let puck = Vec2::new(0.9, 0.0);
    assert_eq!(shown_side(&p, puck, behind(puck, 1.6, 0.3), 0), Some(2));
    assert_eq!(shown_side(&p, puck, behind(puck, 1.9, 0.3), 0), None);
}

#[test]
fn the_mallet_must_stand_0_22_to_0_5_from_the_puck() {
    let p = Params::default();
    let puck = Vec2::new(0.0, 0.0);
    for (distance, expected) in [(0.21, None), (0.23, Some(2)), (0.49, Some(2)), (0.51, None)] {
        assert_eq!(
            shown_side(&p, puck, behind(puck, 0.3, distance), 0),
            expected,
            "{distance}"
        );
    }
}

#[test]
fn seat_one_attacks_the_other_way() {
    let p = Params::default();
    let puck = Vec2::new(0.0, 0.0);
    let mallet = behind(puck, 0.3, 0.3);
    assert_eq!(shown_side(&p, -puck, -mallet, 1), Some(2));
    assert_eq!(shown_side(&p, puck, mallet, 1), None);
}

#[test]
fn a_show_is_held_for_125_ms() {
    assert_eq!(SHOW_HOLD_TICKS, 30);
}
