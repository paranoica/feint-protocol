use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use feint_ai::Difficulty;
use penalty::context::{KICKS, KickToken, TARGETS, TOKEN_WIDTH, npy_f32, npy_i8};
use penalty::persons::{Advice, Habits, match_tokens};

fn habit_row(h: &Habits) -> [f32; 9] {
    [
        h.corner[0],
        h.corner[1],
        h.corner[2],
        h.honest,
        h.fake,
        h.straighten,
        h.guess,
        f32::from(u8::from(h.follows_lean)),
        h.noise,
    ]
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out, first, count, threads] = args.as_slice() else {
        eprintln!("usage: context-data <out dir> <first person> <persons> <threads>");
        return ExitCode::FAILURE;
    };
    let (Ok(first), Ok(count), Ok(threads)) = (
        first.parse::<u64>(),
        count.parse::<u64>(),
        threads.parse::<u64>(),
    ) else {
        eprintln!("first, persons and threads are numbers");
        return ExitCode::FAILURE;
    };
    let d = Difficulty::preset(4).expect("level 4 exists");
    let started = Instant::now();
    let threads = threads.max(1);
    let chunks: Vec<Vec<(u64, Vec<KickToken>)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    (first + t..first + count)
                        .step_by(threads as usize)
                        .map(|id| {
                            let advice = if id % 2 == 0 {
                                Advice::Blind
                            } else {
                                Advice::Oracle
                            };
                            let seat = u8::from(id % 4 >= 2);
                            (id, match_tokens(d, advice, Habits::person(id), seat, id))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut rows: Vec<(u64, Vec<KickToken>)> = chunks.into_iter().flatten().collect();
    rows.sort_by_key(|r| r.0);
    let n = rows.len();
    let mut features = vec![0.0f32; n * KICKS * TOKEN_WIDTH];
    let mut targets = vec![-1i8; n * KICKS * TARGETS];
    let mut habits = Vec::with_capacity(n * 9);
    let mut lengths = Vec::with_capacity(n);
    for (i, (id, tokens)) in rows.iter().enumerate() {
        for (k, t) in tokens.iter().take(KICKS).enumerate() {
            let f = (i * KICKS + k) * TOKEN_WIDTH;
            features[f..f + TOKEN_WIDTH].copy_from_slice(&t.features);
            let g = (i * KICKS + k) * TARGETS;
            targets[g..g + TARGETS].copy_from_slice(&t.target);
        }
        habits.extend(habit_row(&Habits::person(*id)));
        lengths.push(tokens.len().min(KICKS) as f32);
    }
    let dir = PathBuf::from(out);
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| {
            std::fs::write(
                dir.join("tokens.npy"),
                npy_f32(&[n, KICKS, TOKEN_WIDTH], &features),
            )
        })
        .and_then(|()| {
            std::fs::write(
                dir.join("targets.npy"),
                npy_i8(&[n, KICKS, TARGETS], &targets),
            )
        })
        .and_then(|()| std::fs::write(dir.join("habits.npy"), npy_f32(&[n, 9], &habits)))
        .and_then(|()| std::fs::write(dir.join("lengths.npy"), npy_f32(&[n], &lengths)));
    if let Err(e) = written {
        eprintln!("cannot write {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let secs = started.elapsed().as_secs_f64();
    println!(
        "{n} matches of persons {first}..{} in {secs:.1} s ({:.0} matches/s) -> {}",
        first + count,
        n as f64 / secs.max(1e-9),
        dir.display()
    );
    ExitCode::SUCCESS
}
