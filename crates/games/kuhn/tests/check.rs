use std::path::PathBuf;
use std::process::Command;

use kuhn::solve::{Infoset, family, table_key};

fn table(alpha: f64, skip: Option<Infoset>) -> String {
    let truth = family(alpha);
    let mut text = String::from("feint-table-v1 kuhn-poker 2\n");
    for set in Infoset::all().into_iter().filter(|&set| Some(set) != skip) {
        let bet = truth.get(set);
        let hex: String = table_key(set).iter().map(|b| format!("{b:02x}")).collect();
        text += &format!("{hex} {} {bet}\n", 1.0 - bet);
    }
    text
}

fn check(name: &str, text: &str) -> (i32, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("{}-{name}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kuhn_check"))
        .arg(&path)
        .output()
        .unwrap();
    let all =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), all)
}

#[test]
fn an_equilibrium_table_passes_kuhn_check() {
    let (code, out) = check("full.table", &table(1.0 / 6.0, None));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("criterion 2: PASS"), "{out}");
}

#[test]
fn a_table_missing_a_set_fails_kuhn_check_whatever_it_scores() {
    let king = Infoset::opening(2);
    assert_eq!(family(1.0 / 6.0).get(king), 0.5, "uniform is right here");
    let (code, out) = check("missing.table", &table(1.0 / 6.0, Some(king)));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("missing from the table"), "{out}");
    assert!(out.contains("criterion 2: FAIL"), "{out}");
}

#[test]
fn a_table_of_another_version_is_refused_by_its_header() {
    let text = table(1.0 / 6.0, None).replacen("feint-table-v1", "feint-table-v2", 1);
    let (code, out) = check("v2.table", &text);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("feint-table-v1 header"), "{out}");
}
