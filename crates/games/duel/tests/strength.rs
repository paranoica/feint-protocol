use duel::human::against_human;
use feint_ai::{Difficulty, Standing};

const MATCHES: u64 = 40;

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn the_levels_climb_against_a_human_model() {
    let share = |s: Standing| s.share();
    let ladder: Vec<Standing> = (1..=5)
        .map(|l| against_human(Difficulty::preset(l).unwrap(), MATCHES))
        .collect();
    for (level, row) in (1..=5).zip(&ladder) {
        eprintln!(
            "level {level}: won {} of {MATCHES}, hits {}:{}, share {:.2}",
            row.won,
            row.mine,
            row.theirs,
            share(*row)
        );
    }
    assert!(ladder[0].won <= 10, "the human model must beat level 1");
    assert!(
        share(ladder[1]) >= 0.4,
        "level 2 must hold its own against the owner-like model"
    );
    assert!(
        share(ladder[1]) <= 0.85,
        "the human model must still hit level 2"
    );
    assert!(
        share(ladder[4]) >= share(ladder[1]),
        "level 5 falls below level 2"
    );
}
