use airhockey::Params;
use airhockey::human::against_human_with;
use feint_ai::{Difficulty, Standing};
use feint_ml::registry::CHECKPOINTS;
use feint_ml::trained::{TrainedPilot, trained_pilot};
use feint_protocol::GameSpecT;

const MATCHES: u64 = 200;

fn standing(level: u8) -> Standing {
    let body = |spec: &GameSpecT| Difficulty::preset(level).unwrap().body(spec);
    against_human_with(
        MATCHES,
        |spec: &GameSpecT, _: &Params, _: Option<&TrainedPilot>| {
            let pilot = trained_pilot(level, CHECKPOINTS[0].id, spec).unwrap();
            (pilot, body(spec))
        },
    )
}

#[test]
#[ignore = "release only: bedrock cmd ml-ladder"]
fn trained_levels_climb_against_the_human_model() {
    let ladder: Vec<Standing> = (1..=5).map(standing).collect();
    for (level, row) in (1..=5).zip(&ladder) {
        let share = row.share();
        let goals = f64::from(row.mine + row.theirs);
        let error = (share * (1.0 - share) / goals).sqrt();
        eprintln!(
            "trained level {level}: won {} of {}, unfinished {}, goals {}:{}, share {share:.3} ± {error:.3}",
            row.won, row.matches, row.unfinished, row.mine, row.theirs
        );
    }
    for pair in ladder.windows(2) {
        assert!(
            pair[1].share() > pair[0].share(),
            "a trained level is not stronger than the one below: {pair:?}"
        );
    }
    assert!(
        ladder[4].share() >= ladder[0].share() + 0.1,
        "trained level 5 is not clearly above level 1"
    );
}
