use feint_ai::{Difficulty, Standing};
use penalty::human::against_human;

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
            "level {level}: won {} of {MATCHES}, goals {}:{}, share {:.2}",
            row.won,
            row.mine,
            row.theirs,
            share(*row)
        );
    }
    assert!(ladder[0].won <= 10, "the human model must beat level 1");
    assert!(ladder[1].won >= 24, "level 2 must win most shootouts");
    assert!(
        share(ladder[4]) >= share(ladder[1]) + 0.05,
        "level 5 is not clearly above level 2"
    );
}
