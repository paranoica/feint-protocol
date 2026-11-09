use feint_ai::{Difficulty, Standing};
use feint_compare::{GameReport, honest, measure, to_json};

fn standing(mine: u32, theirs: u32, fakes: u32) -> Standing {
    Standing {
        matches: 10,
        won: 6,
        mine,
        theirs,
        fakes,
    }
}

#[test]
fn the_report_names_each_game_its_ladder_and_the_payoff_of_deception() {
    let game = GameReport {
        id: "duel",
        ladder: [(1, 3), (2, 2), (3, 1), (4, 1), (5, 1)]
            .map(|(m, t)| standing(m, t, 4))
            .to_vec(),
        honest: standing(1, 1, 0),
    };
    assert!((game.payoff() - (5.0 / 6.0 - 0.5)).abs() < 1e-12);
    let json = to_json(40, &[game]);
    assert!(
        json.starts_with("{\"matches\":40,\"games\":[{\"id\":\"duel\""),
        "{json}"
    );
    assert!(json.contains("\"levels\":[{\"level\":1,\"matches\":10,\"won\":6,\"mine\":1,\"theirs\":3,\"share\":0.2500,\"fakes\":0.4000}"), "{json}");
    assert!(json.contains("\"honest\":{\"level\":5,\"matches\":10,\"won\":6,\"mine\":1,\"theirs\":1,\"share\":0.5000,\"fakes\":0.0000}"), "{json}");
    assert!(json.contains("\"payoff\":0.3333"), "{json}");
    assert!(json.ends_with("}]}"));
}

#[test]
fn the_honest_level_five_differs_only_in_deception_and_never_fakes() {
    let five = Difficulty::preset(5).unwrap();
    let plain = honest(five);
    assert_eq!(plain.deception, 0.0);
    assert_eq!(
        Difficulty {
            deception: five.deception,
            ..plain
        },
        five
    );
    let report = measure("duel", 2);
    assert_eq!(report.ladder.len(), 5);
    assert_eq!(report.honest.fakes, 0, "no fakes without deception");
    assert!(report.ladder.iter().all(|s| s.matches == 2));
    assert!(report.ladder[4].mine + report.ladder[4].theirs > 0);
}
