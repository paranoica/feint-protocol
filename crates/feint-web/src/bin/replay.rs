use std::process::ExitCode;

use airhockey::{AirHockey, Params};
use feint_ai::{decode_record, replay_record};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: replay <record file>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let record = match decode_record(&bytes) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("{path}: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut game = AirHockey::new(Params::default(), record.header.levels[1]);
    match replay_record(&mut game, &record) {
        Ok(digest) => {
            println!("{digest:016x}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{path}: {error:?}");
            ExitCode::FAILURE
        }
    }
}
