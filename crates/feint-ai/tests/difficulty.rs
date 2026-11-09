use feint_ai::{Difficulty, LongGame, Memory};
use feint_protocol::{DimT, GameSpecT};

#[test]
fn presets_follow_the_spec_table() {
    let levels: Vec<Difficulty> = (1..=5).map(|l| Difficulty::preset(l).unwrap()).collect();
    let pick = |f: fn(&Difficulty) -> f64| levels.iter().map(f).collect::<Vec<_>>();
    assert_eq!(
        levels.iter().map(|d| d.reaction_ms).collect::<Vec<_>>(),
        vec![400, 300, 220, 170, 150]
    );
    assert_eq!(pick(|d| d.physique), vec![0.4, 0.6, 0.75, 0.9, 1.0]);
    assert_eq!(pick(|d| d.spread), vec![0.12, 0.07, 0.045, 0.025, 0.01]);
    assert_eq!(
        levels.iter().map(|d| d.depth).collect::<Vec<_>>(),
        vec![0, 0, 1, 2, 3]
    );
    assert_eq!(pick(|d| d.deception), vec![0.0, 0.0, 0.1, 0.4, 0.7]);
    assert_eq!(pick(|d| d.adaptation), vec![0.0, 0.0, 0.3, 0.6, 1.0]);
    assert_eq!(
        levels.iter().map(|d| d.memory).collect::<Vec<_>>(),
        vec![
            Memory::None,
            Memory::None,
            Memory::Match,
            Memory::Match,
            Memory::MatchAndProfile
        ]
    );
    assert_eq!(
        levels.iter().map(|d| d.long_game).collect::<Vec<_>>(),
        vec![
            LongGame::None,
            LongGame::None,
            LongGame::None,
            LongGame::Simple,
            LongGame::Full
        ]
    );
    assert_eq!(
        levels.iter().map(|d| d.anticipate).collect::<Vec<_>>(),
        vec![false, true, true, true, true]
    );
    assert_eq!(
        levels.iter().map(|d| d.repertoire).collect::<Vec<_>>(),
        vec![0, 1, 2, 2, 2]
    );
    assert_eq!(Difficulty::preset(0), None);
    assert_eq!(Difficulty::preset(6), None);
}

fn spec_with_body(dims: &[(f32, f32)]) -> GameSpecT {
    let mut spec = GameSpecT::default();
    spec.body_params = Some(
        dims.iter()
            .map(|&(min, max)| {
                let mut d = DimT::default();
                d.min = min;
                d.max = max;
                d
            })
            .collect(),
    );
    spec
}

#[test]
fn body_is_min_plus_range_times_physique() {
    let spec = spec_with_body(&[(0.0, 12.0), (0.0, 200.0), (1.0, 3.0)]);
    let level = |l| Difficulty::preset(l).unwrap();
    assert_eq!(level(1).body(&spec).values, Some(vec![4.8, 80.0, 1.8]));
    assert_eq!(level(5).body(&spec).values, Some(vec![12.0, 200.0, 3.0]));
    let mut idle = level(1);
    idle.physique = 0.0;
    assert_eq!(idle.body(&spec).values, Some(vec![0.0, 0.0, 1.0]));
    assert_eq!(level(3).body(&GameSpecT::default()).values, Some(vec![]));
}
