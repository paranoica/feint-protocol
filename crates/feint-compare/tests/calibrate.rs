use feint_compare::calibrate::{Calibration, Shown, held_shows, pair};

#[test]
fn a_detection_pairs_with_a_labelled_fake_twelve_ticks_away_not_thirteen() {
    assert_eq!(pair(&[100], &[112]), (1, 1));
    assert_eq!(pair(&[100], &[88]), (1, 1));
    assert_eq!(pair(&[100], &[113]), (0, 0));
    assert_eq!(pair(&[100], &[87]), (0, 0));
}

#[test]
fn pairing_counts_each_side_on_its_own() {
    assert_eq!(
        pair(&[100, 500], &[105]),
        (1, 1),
        "one detection, two labels"
    );
    assert_eq!(
        pair(&[100], &[95, 105, 900]),
        (2, 1),
        "three detections, one label"
    );
    assert_eq!(pair(&[], &[1]), (0, 0));
    assert_eq!(pair(&[1], &[]), (0, 0));
}

#[test]
fn held_shows_split_on_a_gap_or_a_new_class() {
    let w = |tick, class| Shown { tick, class };
    let runs = held_shows(&[w(1, 0), w(2, 0), w(3, 0), w(5, 0), w(6, 2), w(7, 2)]);
    assert_eq!(runs, vec![(1, 3, 0), (5, 5, 0), (6, 7, 2)]);
    assert!(held_shows(&[]).is_empty());
}

#[test]
fn rates_and_ratios_of_an_empty_calibration_are_defined() {
    let c = Calibration::default();
    assert_eq!(c.labelled_rate(), 0.0);
    assert_eq!(c.precision(), None);
    assert_eq!(c.recall(), None);
    let c = Calibration {
        reached: 5,
        agreed: 4,
        unshot: 0,
        matches: 4,
        labelled: 10,
        detected: 8,
        found: 6,
        caught: 7,
        botched: 3,
    };
    assert_eq!(c.labelled_rate(), 2.5);
    assert_eq!(c.detected_rate(), 2.0);
    assert_eq!(c.precision(), Some(0.75));
    assert_eq!(c.recall(), Some(0.7));
    assert_eq!(c.agreement(), Some(0.8));
    assert_eq!(Calibration::default().agreement(), None);
}

mod film {
    use airhockey::Vec2;
    use airhockey::analysis::Frame;

    pub const PUCK: Vec2 = Vec2::new(-0.5, 0.0);

    fn unit(v: Vec2) -> Vec2 {
        v * (1.0 / v.length())
    }

    pub fn fake() -> Vec<Frame> {
        let far = Vec2::new(0.9, -0.4);
        let mut out = Vec::new();
        let mut push = |puck: Vec2, mallet: Vec2| {
            let tick = out.len() as u64;
            out.push(Frame {
                tick,
                puck,
                mallets: [mallet, far],
                score: [0, 0],
            });
        };
        for _ in 0..30 {
            push(PUCK, Vec2::new(-0.9, 0.0));
        }
        let show = PUCK - unit(Vec2::new(1.5, -0.3)) * 0.32;
        for _ in 0..96 {
            push(PUCK, show);
        }
        let dir = unit(Vec2::new(1.5, 0.084));
        let mallet = PUCK - dir * 0.12;
        let mut at = PUCK;
        push(at, mallet);
        for _ in 0..60 {
            at += dir * (3.0 / 240.0);
            push(at, mallet);
        }
        out
    }
}

fn judged(shows: &[Shown]) -> Calibration {
    use feint_ai::{RunHeader, RunReport};
    use feint_compare::calibrate::judge;
    use feint_compare::play::{Played, human_params};
    let played = Played {
        seat: 0,
        frames: film::fake(),
        report: RunReport {
            header: RunHeader {
                game_id: "airhockey".into(),
                game_version: airhockey::GAME_VERSION,
                seed: 0,
                bodies: Vec::new(),
                levels: [0, 0],
                opponent: feint_ai::Opponent::HandMade,
            },
            score: vec![0, 0],
            ticks: 187,
            finished: false,
            actions: Vec::new(),
            digest: 0,
        },
        bot: (),
    };
    judge(&human_params(), &played, shows)
}

fn run(from: u64, to: u64, class: u8) -> Vec<Shown> {
    (from..=to).map(|tick| Shown { tick, class }).collect()
}

#[test]
fn a_held_live_fake_is_labelled_and_paired_with_the_detection() {
    let c = judged(&run(30, 125, 0));
    assert_eq!(
        (c.labelled, c.detected, c.found, c.caught, c.botched),
        (1, 1, 1, 1, 0),
        "{c:?}"
    );
    let c = judged(&run(78, 125, 0));
    assert_eq!(
        c.labelled, 1,
        "48 ticks held up to the contact at 126: {c:?}"
    );
}

#[test]
fn a_fake_too_short_or_dropped_before_the_contact_is_not_labelled() {
    let c = judged(&run(79, 125, 0));
    assert_eq!(
        (c.labelled, c.detected, c.found),
        (0, 1, 0),
        "47 ticks: {c:?}"
    );
    let c = judged(&run(30, 101, 0));
    assert_eq!(c.labelled, 0, "it ended 25 ticks before the contact: {c:?}");
    assert_eq!(c.unshot, 1, "the shot came 25 ticks after its end: {c:?}");
    let c = judged(&run(30, 102, 0));
    assert_eq!(c.unshot, 0, "the shot came 24 ticks after its end: {c:?}");
    let c = judged(&run(30, 102, 0));
    assert_eq!(c.labelled, 1, "it ended 24 ticks before the contact: {c:?}");
}

#[test]
fn a_show_to_the_side_the_puck_went_is_botched() {
    let c = judged(&run(30, 125, 2));
    assert_eq!((c.labelled, c.botched), (0, 1), "{c:?}");
}

#[test]
fn a_fake_started_after_the_contact_is_not_labelled() {
    let c = judged(&run(130, 300, 0));
    assert_eq!(c.labelled, 0, "{c:?}");
}
