use std::io::Read;
use std::process::ExitCode;

use believe::check::check_against;
use believe::{Moves, Params, Style, observation_len};
use feint_turn::{TurnPolicy, turn_layout};

const SEEDS: std::ops::Range<u64> = 30_000..30_400;
const MAX_POLICY_BYTES: u64 = 64 << 20;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: believe-check <policy.safetensors>");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    let read = std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_POLICY_BYTES + 1).read_to_end(&mut bytes));
    if let Err(error) = read {
        eprintln!("cannot read {path}: {error}");
        return ExitCode::from(2);
    }
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        eprintln!("{path}: larger than {MAX_POLICY_BYTES} bytes");
        return ExitCode::from(2);
    }
    let params = Params::default();
    let layout = turn_layout(
        "believe",
        observation_len(&params),
        Moves::new(&params).len(),
    );
    let policy = match TurnPolicy::from_safetensors(&bytes, &layout) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };
    let mut pass = true;
    println!(
        "opponent      won  drawn  lost  share   95% interval      false lays  lies survived  calls hit"
    );
    for name in ["honest", "liar:300", "caller:300", "counter"] {
        let Some(style) = Style::named(name) else {
            eprintln!("unknown style {name}");
            return ExitCode::from(2);
        };
        let t = match check_against(&policy, style, SEEDS) {
            Ok(t) => t,
            Err(error) => {
                eprintln!("{name}: {error}");
                return ExitCode::from(2);
            }
        };
        let (low, high) = t.interval();
        let ratio = |a: u32, b: u32| {
            if b == 0 {
                0.0
            } else {
                f64::from(a) / f64::from(b)
            }
        };
        pass &= low > 0.5;
        println!(
            "{name:<12} {:>4} {:>6} {:>5}  {:.3}  [{low:.3}, {high:.3}]  {:>9.3}  {:>13.3}  {:>9.3}",
            t.wins,
            t.draws,
            t.losses,
            t.share(),
            ratio(t.false_lays, t.lays),
            ratio(t.lies_survived, t.false_lays),
            ratio(t.calls_hit, t.calls),
        );
    }
    println!("criterion 3: {}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
