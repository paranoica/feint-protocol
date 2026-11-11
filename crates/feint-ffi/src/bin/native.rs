use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_ffi::body;
use feint_protocol::GameAdapter;

fn main() {
    let seed = 77;
    let mut game = AirHockey::new(Params::default(), 5);
    let (Some(a_body), Some(b_body)) = (body(1, &game), body(5, &game)) else {
        std::process::exit(1);
    };
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: vec![a_body, b_body],
        levels: [1, 5],
        opponent: feint_ai::Opponent::HandMade,
    };
    let spec = game.spec();
    let bot = |level| {
        Difficulty::preset(level)
            .and_then(|d| Bot::new(d, HockeyPlaybook::new(Params::default()), &spec).ok())
    };
    let (Some(mut a), Some(mut b)) = (bot(1), bot(5)) else {
        std::process::exit(1);
    };
    match run_match(&mut game, [&mut a, &mut b], header, 240 * 60 * 10) {
        Ok(r) => println!(
            "score {}:{} after {} ticks digest {:016x}",
            r.score[0], r.score[1], r.ticks, r.digest
        ),
        Err(_) => std::process::exit(1),
    }
}
