use feint_compare::trained::{FakesRun, fakes_run};

fn run(args: &[&str]) -> Option<FakesRun> {
    fakes_run(&args.iter().map(|a| (*a).to_string()).collect::<Vec<_>>())
}

#[test]
fn one_path_writes_the_shipped_report_there() {
    assert_eq!(
        run(&["web/public/fakes.json"]),
        Some(FakesRun::Shipped("web/public/fakes.json".into()))
    );
}

#[test]
fn of_reports_a_checkpoint_file_and_writes_nothing() {
    assert_eq!(
        run(&["--of", "runs/fakes/policy-00734.safetensors"]),
        Some(FakesRun::Of("runs/fakes/policy-00734.safetensors".into()))
    );
}

#[test]
fn a_checkpoint_after_the_output_path_is_refused() {
    assert_eq!(
        run(&[
            "web/public/fakes.json",
            "runs/fakes/policy-00734.safetensors"
        ]),
        None
    );
}

#[test]
fn nothing_a_bare_of_and_extra_arguments_are_refused() {
    assert_eq!(run(&[]), None);
    assert_eq!(run(&["--of"]), None);
    assert_eq!(run(&["--of", "a.safetensors", "b.safetensors"]), None);
    assert_eq!(run(&["--of", "--of"]), None);
}
