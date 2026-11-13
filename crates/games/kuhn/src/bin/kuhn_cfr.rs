use std::process::ExitCode;

use feint_ai::cfr::Solver;
use kuhn::Kuhn;
use kuhn::solve::{exploitability, family_check, strategy_from_table};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(iterations), Some(seed), Some(out)) = (
        args.first().and_then(|v| v.parse::<u64>().ok()),
        args.get(1).and_then(|v| v.parse::<u64>().ok()),
        args.get(2),
    ) else {
        eprintln!("usage: kuhn-cfr <iterations> <seed> <out.table>");
        return ExitCode::from(2);
    };
    let mut solver = match Solver::new(Kuhn::new(), seed) {
        Ok(solver) => solver,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = solver.run(iterations) {
        eprintln!("{error}");
        return ExitCode::from(1);
    }
    let table = solver.table();
    if let Err(error) = std::fs::write(out, table.write()) {
        eprintln!("cannot write {out}: {error}");
        return ExitCode::from(1);
    }
    let (s, missing) = strategy_from_table(&table);
    let check = family_check(&s);
    println!(
        "{iterations} iterations, seed {seed}: {} sets, {} missing; exploitability {:.5}; alpha {:.4}, jack bluff {:.4}, deviation {:.4}",
        table.rows.len(),
        missing.len(),
        exploitability(&s),
        check.alpha,
        check.jack_bluff,
        check.deviation
    );
    ExitCode::SUCCESS
}
