use std::path::PathBuf;
use std::process::ExitCode;

use feint_arena::migrate::{BACKUP, FROM, migrate};
use feint_protocol::PROTOCOL_VERSION;

fn main() -> ExitCode {
    let mut write = false;
    let mut root = PathBuf::from("data");
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--write" => write = true,
            a if a.starts_with('-') => {
                eprintln!("usage: migrate-v1 [--write] [data dir]");
                return ExitCode::FAILURE;
            }
            a => root = PathBuf::from(a),
        }
    }
    match migrate(&root, write) {
        Ok(r) => {
            let mode = if write {
                "migrated"
            } else {
                "check only, nothing written (pass --write to migrate)"
            };
            println!("{FROM} -> {PROTOCOL_VERSION} in {}: {mode}", root.display());
            println!(
                "records: {} to upgrade or upgraded, {} already {PROTOCOL_VERSION}",
                r.records_upgraded, r.records_current
            );
            println!(
                "{} replayed with the bot to the stored digest and bytes",
                r.verified
            );
            for (id, game) in &r.replayed {
                println!(
                    "record {id}: {game}, today's bot plays it otherwise; the recorded inputs replay to the stored digest, score and per-tick Parquet"
                );
            }
            for (id, game, version) in &r.byte_proven {
                println!(
                    "record {id}: {game} game version {version} is older than today's; proven byte for byte except the version string"
                );
            }
            println!(
                "profiles: {} to upgrade or upgraded, {} already {PROTOCOL_VERSION}",
                r.profiles_upgraded, r.profiles_current
            );
            for path in &r.stray {
                println!("not in the database, left alone: {}", path.display());
            }
            if write {
                let made = if r.backup_made {
                    "made"
                } else {
                    "kept as it was"
                };
                println!("backup {}: {made}", root.join(BACKUP).display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("migrate-v1 stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
