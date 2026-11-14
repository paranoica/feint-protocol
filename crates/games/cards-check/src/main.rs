use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("search") {
        println!("{}", cards_check::search_digest());
        for level in [6u8, 7, 8] {
            let started = Instant::now();
            let n = cards_check::search_decisions(level);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            eprintln!(
                "level {level}: {n} decisions, {:.1} ms each",
                ms / n.max(1) as f64
            );
        }
        return;
    }
    println!(
        "{} {}",
        cards_check::kuhn_digest(),
        cards_check::believe_digest()
    );
}
