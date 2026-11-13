use std::io::Read;
use std::process::ExitCode;

use feint_ai::cfr::{MAX_TABLE_BYTES, Table};
use feint_turn::{TurnPolicy, turn_layout};
use kuhn::solve::{
    Infoset, Strategy, exploitability, family_check, reach, strategy_from_policy,
    strategy_from_table,
};

const MAX_EXPLOITABILITY: f64 = 0.01;
const TOLERANCE: f64 = 0.03;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: kuhn-check <policy.safetensors | strategy table>");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    let read = std::fs::File::open(&path).and_then(|file| {
        file.take(MAX_TABLE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    if let Err(error) = read {
        eprintln!("cannot read {path}: {error}");
        return ExitCode::from(2);
    }
    let (s, missing) = match strategy(&bytes) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };
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
    let pass = missing == 0
        && s.is_probability()
        && exploit <= MAX_EXPLOITABILITY
        && (check.jack_bluff - 1.0 / 3.0).abs() <= TOLERANCE
        && check.deviation <= TOLERANCE;
    println!("exploitability {exploit:.5} chips per hand (limit {MAX_EXPLOITABILITY})");
    println!(
        "alpha {:.4}; second player's jack bets after a pass {:.4} (1/3 within {TOLERANCE}); largest deviation from the family {:.4}",
        check.alpha, check.jack_bluff, check.deviation
    );
    if missing > 0 {
        println!("{missing} information sets missing: a gate needs the whole strategy");
    }
    println!("criterion 2: {}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn strategy(bytes: &[u8]) -> Result<(Strategy, usize), String> {
    if bytes.len() > MAX_TABLE_BYTES {
        return Err(format!("larger than {MAX_TABLE_BYTES} bytes"));
    }
    if bytes.starts_with(b"feint-table-") {
        let text = std::str::from_utf8(bytes).map_err(|_| "a table is not UTF-8".to_string())?;
        let table = Table::read(text, "kuhn-poker", 2).map_err(|e| e.to_string())?;
        let (s, missing) = strategy_from_table(&table);
        for set in &missing {
            println!("missing from the table, read as uniform: {set:?}");
        }
        return Ok((s, missing.len()));
    }
    let mut policy = TurnPolicy::from_safetensors(bytes, &turn_layout("kuhn", 4, 2))
        .map_err(|e| e.to_string())?;
    Ok((strategy_from_policy(&mut policy), 0))
}
