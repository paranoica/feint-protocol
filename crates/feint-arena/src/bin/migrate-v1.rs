use std::path::PathBuf;
use std::process::ExitCode;

use feint_arena::migrate::{BACKUP, FROM, migrate};
use feint_protocol::PROTOCOL_VERSION;

fn main() -> ExitCode {
    let mut write = true;
    let mut root = PathBuf::from("data");
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => write = false,
            a if a.starts_with('-') => {
                eprintln!("usage: migrate-v1 [--check] [data dir]");
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
                "check only, nothing written"
            };
            println!("{FROM} -> {PROTOCOL_VERSION} in {}: {mode}", root.display());
            println!(
                "records: {} upgraded, {} already {PROTOCOL_VERSION}; {} verified by replay against the stored digest",
                r.records_upgraded, r.records_current, r.verified
            );
            for (id, game, why) in &r.replayed {
                println!(
                    "record {id}: {game}, today's bot plays it otherwise ({why}); its recorded inputs replay to the stored digest and score"
                );
            }
            for (id, game, version) in &r.byte_proven {
                println!(
                    "record {id}: {game} game version {version} cannot replay today; proven byte for byte except the version string"
                );
            }
            println!(
                "profiles: {} upgraded, {} already {PROTOCOL_VERSION}",
                r.profiles_upgraded, r.profiles_current
            );
            for path in &r.stray {
                println!("not in the database, left alone: {}", path.display());
            }
            if write {
                let backup = root.join(BACKUP);
                let made = if r.backup_made {
                    "made"
                } else {
                    "kept as it was"
                };
                println!("backup {}: {made}", backup.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("migrate-v1 stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
