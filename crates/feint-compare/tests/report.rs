use feint_ai::{Difficulty, LongGame, Standing};
use feint_compare::{GameReport, honest, measure, to_json};

fn standing(mine: u32, theirs: u32, fakes: u32) -> Standing {
    Standing {
        matches: 10,
        won: 6,
        mine,
        theirs,
        fakes,
        unfinished: 0,
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
fn the_honest_level_five_loses_only_its_fakes_and_its_long_game_breaks() {
    let five = Difficulty::preset(5).unwrap();
    let plain = honest(five);
    assert_eq!((plain.deception, plain.long_game), (0.0, LongGame::None));
    assert_eq!(
        Difficulty {
            deception: five.deception,
            long_game: five.long_game,
            ..plain
        },
        five
    );
    for id in feint_compare::GAMES {
        let report = measure(id, 4);
        assert_eq!(report.ladder.len(), 5);
        assert_eq!(
            report.honest.fakes, 0,
            "{id}: no deceptive move without deception"
        );
        assert!(report.ladder[4].fakes > 0, "{id}: level 5 does fake");
        assert!(report.ladder.iter().all(|s| s.matches == 4), "{id}");
    }
}
