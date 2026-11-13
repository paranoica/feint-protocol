use std::process::ExitCode;

use feint_turn::{TurnPolicy, turn_layout};
use kuhn::solve::{Infoset, exploitability, family_check, reach, strategy_from_policy};

const MAX_EXPLOITABILITY: f64 = 0.01;
const TOLERANCE: f64 = 0.03;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: kuhn-check <policy.safetensors>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::from(2);
        }
    };
    let mut policy = match TurnPolicy::from_safetensors(&bytes, &turn_layout("kuhn", 4, 2)) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };
    let s = strategy_from_policy(&mut policy);
    println!("information set          bet     reach");
    for set in Infoset::all() {
        let card = ["J", "Q", "K"][usize::from(set.card())];
        let history = format!("{:?}", set.history());
        println!(
            "player {} {card} {history:<10} {:.4}  {:.4}",
            set.player(),
            s.get(set),
            reach(&s, set)
        );
    }
    let exploit = exploitability(&s);
    let check = family_check(&s);
    let pass = exploit <= MAX_EXPLOITABILITY
        && (check.jack_bluff - 1.0 / 3.0).abs() <= TOLERANCE
        && check.deviation <= TOLERANCE;
    println!("exploitability {exploit:.5} chips per hand (limit {MAX_EXPLOITABILITY})");
    println!(
        "alpha {:.4}; second player's jack bets after a pass {:.4} (1/3 within {TOLERANCE}); largest deviation from the family {:.4}",
        check.alpha, check.jack_bluff, check.deviation
    );
    println!("criterion 2: {}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
