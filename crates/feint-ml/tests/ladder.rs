use airhockey::Params;
use airhockey::human::against_human_with;
use feint_ai::{Difficulty, Standing};
use feint_ml::registry::for_level;
use feint_ml::trained::{TrainedPilot, trained_pilot};
use feint_protocol::GameSpecT;

const MATCHES: u64 = 200;

fn standing(level: u8) -> Standing {
    let body = |spec: &GameSpecT| Difficulty::preset(level).unwrap().body(spec);
    against_human_with(
        MATCHES,
        |spec: &GameSpecT, _: &Params, _: Option<&TrainedPilot>| {
            let pilot = trained_pilot(level, for_level(level).unwrap().id, spec).unwrap();
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

#[test]
#[ignore = "release only: FEINT_LADDER_DIR=<folder> cargo test --release -p feint-ml --test ladder screen"]
fn screen_candidates_against_the_human_model() {
    let dir =
        std::env::var("FEINT_LADDER_DIR").expect("FEINT_LADDER_DIR names a folder of candidates");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "safetensors"))
        .collect();
    files.sort();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let rows = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..7 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let mlp = feint_ml::mlp::Mlp::from_safetensors(&std::fs::read(path).unwrap())
                        .unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
                    let shares: Vec<f64> = (1..=4)
                        .map(|level| {
                            let body =
                                |spec: &GameSpecT| Difficulty::preset(level).unwrap().body(spec);
                            against_human_with(
                                40,
                                |spec: &GameSpecT, _: &Params, _: Option<&TrainedPilot>| {
                                    let pilot = feint_ml::trained::trained_pilot_from(
                                        level,
                                        mlp.clone(),
                                        spec,
                                    )
                                    .unwrap();
                                    (pilot, body(spec))
                                },
                            )
                            .share()
                        })
                        .collect();
                    rows.lock().unwrap().push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        shares,
                    ));
                }
            });
        }
    });
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, shares) in rows {
        eprintln!(
            "{name}: {}",
            shares
                .iter()
                .map(|s| format!("{s:.3}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}
