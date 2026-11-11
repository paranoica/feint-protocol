use std::process::ExitCode;

use feint_arena::verify;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: verify-record <record file>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    match verify(&bytes, None) {
        Ok(verified) => {
            println!("{:016x}", verified.digest);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("the record does not verify: {error:?}");
            ExitCode::FAILURE
        }
    }
}
