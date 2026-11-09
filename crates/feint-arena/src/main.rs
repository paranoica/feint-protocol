use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use feint_arena::{Arena, Store, router};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| default.to_owned())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let dist = PathBuf::from(arg(&args, "--dist", "web/dist"));
    let data = PathBuf::from(arg(&args, "--data", "data"));
    let port = arg(&args, "--port", "8787");
    let store = match Store::open(&data) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open {}: {error:?}", data.display());
            return ExitCode::FAILURE;
        }
    };
    let mut seed = [0u8; 32];
    let entropy = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut seed));
    if let Err(error) = entropy {
        eprintln!("cannot read /dev/urandom: {error}");
        return ExitCode::FAILURE;
    }
    let arena = Arena::new(store, dist, Arc::new(now), seed);
    let listener = match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("cannot listen on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Ok(addr) = listener.local_addr() {
        println!("feint-arena on http://{addr}");
    }
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match axum::serve(listener, router(arena))
        .with_graceful_shutdown(shutdown)
        .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("server stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
