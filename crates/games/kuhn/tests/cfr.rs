use feint_ai::cfr::{Solver, Table};
use kuhn::Kuhn;
use kuhn::solve::{Infoset, exploitability, family, family_check, strategy_from_table, table_key};

fn solved(iterations: u64, seed: u64) -> Table {
    let mut solver = Solver::new(Kuhn::new(), seed).unwrap();
    solver.run(iterations).unwrap();
    solver.table()
}

#[test]
#[ignore = "about 30 s in debug; run in release by kuhn-cfr-gate"]
fn mccfr_on_kuhn_meets_criterion_2() {
    let (s, missing) = strategy_from_table(&solved(1_000_000, 1));
    assert!(missing.is_empty(), "{missing:?}");
    let exploit = exploitability(&s);
    let check = family_check(&s);
    assert!(exploit <= 0.002, "exploitability {exploit}");
    assert!(
        (check.jack_bluff - 1.0 / 3.0).abs() <= 0.01,
        "jack bluff {}",
        check.jack_bluff
    );
    assert!(check.deviation <= 0.01, "deviation {}", check.deviation);
    assert!(
        (0.0..=1.0 / 3.0).contains(&check.alpha),
        "alpha {}",
        check.alpha
    );
}

#[test]
fn a_short_mccfr_run_on_kuhn_is_already_close() {
    let table = solved(20_000, 1);
    assert_eq!(table.game, "kuhn-poker");
    assert_eq!(table.rows.len(), 12);
    let (s, missing) = strategy_from_table(&table);
    assert!(missing.is_empty(), "{missing:?}");
    let exploit = exploitability(&s);
    assert!(exploit <= 0.015, "exploitability {exploit}");
    let back = Table::read(&table.write(), "kuhn-poker", 2).unwrap();
    assert_eq!(back, table);
}

#[test]
fn a_table_missing_a_set_is_read_as_uniform_there() {
    let truth = family(1.0 / 6.0);
    let mut text = String::from("feint-table-v1 kuhn-poker 2\n");
    let kept: Vec<Infoset> = Infoset::all().into_iter().skip(1).collect();
    for &set in &kept {
        let bet = truth.get(set);
        let hex: String = table_key(set).iter().map(|b| format!("{b:02x}")).collect();
        text += &format!("{hex} {} {bet}\n", 1.0 - bet);
    }
    let table = Table::read(&text, "kuhn-poker", 2).unwrap();
    let (s, missing) = strategy_from_table(&table);
    let first = Infoset::all()[0];
    assert_eq!(missing, vec![first]);
    assert_eq!(s.get(first), 0.5);
    for set in kept {
        assert!((s.get(set) - truth.get(set)).abs() < 1e-12, "{set:?}");
    }
}
