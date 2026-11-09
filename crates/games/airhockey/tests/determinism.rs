mod sequence;
use sequence::Lcg;

use airhockey::{Body, Input, Match, MatchEvent, Params, World};

/// Scripted play: each player sends a new target at irregular ticks, sometimes aimed at the
/// puck so goals happen. Returns the inputs it sent and the digest after every tick.
fn play(seed: u64, ticks: u64, tweak_at: Option<u64>) -> (Match, Vec<Input>, Vec<u64>) {
    let mut m = Match::new(Params::default(), [Body::default(); 2], seed);
    let mut rng = Lcg(seed ^ 0x5EED);
    let mut inputs = Vec::new();
    let mut digests = Vec::new();
    let mut events = Vec::new();
    for t in 0..ticks {
        for player in 0..2u8 {
            if rng.next().is_multiple_of(7) {
                let puck = m.world.puck.pos;
                let target = if rng.next().is_multiple_of(2) {
                    [puck.x as f32, puck.y as f32]
                } else {
                    [rng.range(-1.0, 1.0) as f32, rng.range(-0.5, 0.5) as f32]
                };
                let input = Input {
                    tick: t,
                    player,
                    target,
                };
                if m.apply(input).is_ok() {
                    inputs.push(input);
                }
            }
        }
        if tweak_at == Some(t) {
            let input = Input {
                tick: t,
                player: 0,
                target: [-0.3, 0.2],
            };
            m.apply(input).expect("the tweak applies");
            inputs.push(input);
        }
        events.clear();
        m.step(&mut events);
        digests.push(m.digest());
    }
    (m, inputs, digests)
}

fn bits(world: &World) -> Vec<u64> {
    let mut out = Vec::new();
    let mut push = |v: airhockey::Vec2| {
        out.push(v.x.to_bits());
        out.push(v.y.to_bits());
    };
    push(world.puck.pos);
    push(world.puck.vel);
    for mallet in &world.mallets {
        push(mallet.pos);
        push(mallet.vel);
        push(mallet.target);
    }
    out
}

#[test]
fn same_seed_and_inputs_are_bit_identical() {
    let (a, inputs_a, digests_a) = play(42, 100_000, None);
    let (b, inputs_b, digests_b) = play(42, 100_000, None);
    assert_eq!(inputs_a, inputs_b);
    assert_eq!(digests_a, digests_b);
    assert_eq!(bits(&a.world), bits(&b.world));
    assert_eq!(a.score(), b.score());
    assert!(
        a.score().iter().sum::<u32>() > 0,
        "no goal in 100k ticks: weak script"
    );
}

#[test]
fn different_inputs_diverge() {
    let (a, _, _) = play(42, 20_000, None);
    let (b, _, _) = play(42, 20_000, Some(500));
    assert_ne!(a.digest(), b.digest());
    assert_ne!(bits(&a.world), bits(&b.world));
}

#[test]
fn different_seeds_diverge() {
    let (a, _, _) = play(1, 5_000, None);
    let (b, _, _) = play(2, 5_000, None);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn replay_of_recorded_inputs_reaches_the_same_digest_every_tick() {
    let (original, inputs, digests) = play(7, 30_000, None);
    assert!(
        digests.windows(2).filter(|w| w[0] != w[1]).count() > 1_000,
        "the digest hardly changes: a replay check on it proves nothing"
    );
    let mut replay = Match::new(Params::default(), [Body::default(); 2], original.seed());
    let mut pending = inputs.iter().peekable();
    let mut events: Vec<MatchEvent> = Vec::new();
    for (t, expected) in digests.iter().enumerate() {
        while let Some(input) = pending.next_if(|i| i.tick == t as u64) {
            replay.apply(*input).expect("a recorded input replays");
        }
        events.clear();
        replay.step(&mut events);
        assert_eq!(replay.digest(), *expected, "replay diverged at tick {t}");
    }
    assert!(pending.next().is_none());
    assert_eq!(bits(&replay.world), bits(&original.world));
}

#[test]
fn digest_sees_every_world_field() {
    let base = Match::new(Params::default(), [Body::default(); 2], 3);
    let d0 = base.digest();
    let nudges: [fn(&mut World); 8] = [
        |w| w.puck.pos.x += 1e-12,
        |w| w.puck.vel.y += 1e-12,
        |w| w.mallets[0].pos.y += 1e-12,
        |w| w.mallets[0].vel.x += 1e-12,
        |w| w.mallets[0].target.x += 1e-12,
        |w| w.mallets[1].pos.x += 1e-12,
        |w| w.mallets[1].vel.y += 1e-12,
        |w| w.mallets[1].target.y += 1e-12,
    ];
    for (i, nudge) in nudges.iter().enumerate() {
        let mut m = base.clone();
        nudge(&mut m.world);
        assert_ne!(m.digest(), d0, "nudge {i} not seen by the digest");
    }
}
