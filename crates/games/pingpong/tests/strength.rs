use feint_ai::{Difficulty, Standing};
use pingpong::human::against_human;

const MATCHES: u64 = 200;

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn the_levels_climb_against_a_human_model() {
    let ladder: Vec<Standing> = (1..=5)
        .map(|l| against_human(Difficulty::preset(l).unwrap(), MATCHES))
        .collect();
    for (level, row) in (1..=5).zip(&ladder) {
        eprintln!(
            "level {level}: won {} of {MATCHES}, points {}:{}, share {:.3}",
            row.won,
            row.mine,
            row.theirs,
            row.share()
        );
    }
    assert!(
        u64::from(ladder[0].won) < MATCHES / 2,
        "the human model must beat level 1"
    );
    for pair in ladder.windows(2) {
        assert!(pair[1].share() > pair[0].share(), "the levels must rise");
    }
    assert!(
        ladder[4].share() >= ladder[1].share() + 0.1,
        "level 5 must be 0.1 above level 2"
    );
}
