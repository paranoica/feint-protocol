use feint_ai::{BadCadence, Cadence, ReactionBuffer, delay_ticks};
use feint_protocol::ObservationT;

fn obs(tick: u64) -> ObservationT {
    let mut o = ObservationT::default();
    o.tick = tick;
    o.state = Some(vec![tick as f32; 3]);
    o
}

#[test]
fn delay_is_the_rounded_reaction_in_ticks() {
    let at_240 = [400, 300, 220, 170, 150].map(|ms| delay_ticks(ms, 240));
    assert_eq!(at_240, [96, 72, 53, 41, 36]);
    assert_eq!(delay_ticks(0, 240), 0);
    assert_eq!(delay_ticks(2, 240), 0, "0.48 ticks rounds down");
    assert_eq!(delay_ticks(3, 240), 1, "0.72 ticks rounds up");
    let max = u64::from(u32::MAX);
    assert_eq!(
        delay_ticks(u32::MAX, u32::MAX) as u64,
        (max * max + 500) / 1000
    );
}

#[test]
fn reaction_buffer_returns_the_observation_n_ticks_old() {
    let mut zero = ReactionBuffer::new(0);
    assert!(zero.delayed(0).is_none() && zero.newest().is_none());
    zero.push(&obs(5));
    assert_eq!(zero.delayed(5).map(|o| o.tick), Some(5));

    let mut buf = ReactionBuffer::new(3);
    for t in 0..3 {
        buf.push(&obs(t));
        assert!(
            buf.delayed(t).is_none(),
            "saw the world before 3 ticks passed (t = {t})"
        );
        assert_eq!(buf.newest().map(|o| o.tick), Some(t));
    }
    buf.push(&obs(3));
    assert_eq!(buf.delayed(3), Some(&obs(0)));
    for t in 4..=10 {
        buf.push(&obs(t));
    }
    assert_eq!(buf.delayed(10), Some(&obs(7)));
    assert_eq!(buf.newest(), Some(&obs(10)));

    buf.clear();
    assert!(buf.delayed(10).is_none() && buf.newest().is_none());
    buf.push(&obs(20));
    assert!(buf.delayed(20).is_none(), "old observations survived clear");
}

#[test]
fn decision_ticks_follow_the_formula() {
    let count = |c: Cadence, n: u64| (0..n).filter(|&t| c.is_decision(t)).count();

    let sixty = Cadence::new(240, 60).unwrap();
    let ticks: Vec<u64> = (0..17).filter(|&t| sixty.is_decision(t)).collect();
    assert_eq!(ticks, vec![0, 4, 8, 12, 16]);
    assert_eq!(count(sixty, 240), 60);

    assert_eq!(count(Cadence::new(240, 240).unwrap(), 240), 240);

    let fifty = Cadence::new(240, 50).unwrap();
    let ticks: Vec<u64> = (0..240).filter(|&t| fifty.is_decision(t)).collect();
    assert_eq!(ticks.len(), 50);
    let gaps: Vec<u64> = ticks.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().all(|&g| g == 4 || g == 5), "{gaps:?}");

    assert!(
        Cadence::new(240, 60)
            .unwrap()
            .is_decision(u64::MAX - u64::MAX % 4)
    );
    for (tick_hz, decision_hz) in [(240, 0), (240, 241), (0, 0), (0, 60)] {
        assert_eq!(
            Cadence::new(tick_hz, decision_hz),
            Err(BadCadence),
            "{decision_hz}/{tick_hz}"
        );
    }
}

#[test]
fn delay_is_measured_in_ticks_not_in_calls() {
    let mut buf = ReactionBuffer::new(72);
    for t in (0..=400).step_by(4) {
        buf.push(&obs(t));
    }
    assert_eq!(buf.delayed(400).map(|o| o.tick), Some(328));
    let mut gap = ReactionBuffer::new(3);
    for t in [0, 1, 2, 1_000_000] {
        gap.push(&obs(t));
    }
    assert_eq!(gap.delayed(1_000_000).map(|o| o.tick), Some(2));
}

#[test]
fn a_repeated_tick_keeps_its_newest_copy_and_evicts_nothing() {
    let tagged = |tick: u64, tag: f32| {
        let mut o = obs(tick);
        o.state = Some(vec![tag]);
        o
    };
    let mut buf = ReactionBuffer::new(2);
    for o in [tagged(5, 1.0), tagged(5, 2.0), tagged(7, 3.0)] {
        buf.push(&o);
    }
    assert_eq!(buf.delayed(7), Some(&tagged(5, 2.0)));

    let mut back = ReactionBuffer::new(2);
    for o in [tagged(5, 1.0), tagged(7, 3.0), tagged(5, 2.0)] {
        back.push(&o);
    }
    assert_eq!(back.delayed(7), Some(&tagged(5, 2.0)));

    let mut repeats = ReactionBuffer::new(3);
    for t in 0..=3 {
        repeats.push(&obs(t));
    }
    for _ in 0..4 {
        repeats.push(&obs(3));
    }
    assert_eq!(repeats.delayed(3).map(|o| o.tick), Some(0));
}
