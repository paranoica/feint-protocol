use std::process::ExitCode;

use feint_compare::trained::play_trained;

fn main() -> ExitCode {
    let args: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let [level, seed, seat] = args[..] else {
        eprintln!("usage: rerun <level> <seed> <seat>");
        return ExitCode::FAILURE;
    };
    let (Ok(level), Ok(seat)) = (u8::try_from(level), u8::try_from(seat)) else {
        eprintln!("level and seat are small numbers");
        return ExitCode::FAILURE;
    };
    if !(1..=5).contains(&level) || seat > 1 {
        eprintln!("level 1 to 5, seat 0 or 1");
        return ExitCode::FAILURE;
    }
    println!("{:016x}", play_trained(level, seed, seat).report.digest);
    ExitCode::SUCCESS
}
