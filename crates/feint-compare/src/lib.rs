pub mod attacks;

use std::fmt::Write;

use feint_ai::{Difficulty, LongGame, Standing};

pub const GAMES: [&str; 3] = ["airhockey", "penalty", "duel"];

#[derive(Clone, Debug, PartialEq)]
pub struct GameReport {
    pub id: &'static str,
    pub ladder: Vec<Standing>,
    pub honest: Standing,
    pub attacks: attacks::Study,
}

impl GameReport {
    #[must_use]
    pub fn payoff(&self) -> f64 {
        self.ladder.last().map_or(0.0, Standing::share) - self.honest.share()
    }
}

#[must_use]
pub fn honest(d: Difficulty) -> Difficulty {
    Difficulty {
        deception: 0.0,
        long_game: LongGame::None,
        ..d
    }
}

fn run(id: &str, d: Difficulty, matches: u64) -> Standing {
    match id {
        "airhockey" => airhockey::human::against_human(d, matches),
        "penalty" => penalty::human::against_human(d, matches),
        "duel" => duel::human::against_human(d, matches),
        _ => panic!("unknown game {id}"),
    }
}

#[must_use]
pub fn measure(id: &'static str, matches: u64) -> GameReport {
    let preset = |l: u8| Difficulty::preset(l).unwrap_or_else(|| panic!("level {l} exists"));
    GameReport {
        id,
        ladder: (1..=5).map(|l| run(id, preset(l), matches)).collect(),
        honest: run(id, honest(preset(5)), matches),
        attacks: attacks::attacks(id, matches),
    }
}

fn row(out: &mut String, level: u8, s: &Standing) {
    let _ = write!(
        out,
        "{{\"level\":{level},\"matches\":{},\"won\":{},\"mine\":{},\"theirs\":{},\"share\":{:.4},\"fakes\":{:.4}}}",
        s.matches,
        s.won,
        s.mine,
        s.theirs,
        s.share(),
        s.fakes_per_match()
    );
}

#[must_use]
pub fn to_json(matches: u64, games: &[GameReport]) -> String {
    let mut out = format!("{{\"matches\":{matches},\"games\":[");
    for (i, g) in games.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"id\":\"{}\",\"levels\":[", g.id);
        for (k, s) in g.ladder.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            row(&mut out, u8::try_from(k + 1).unwrap_or(u8::MAX), s);
        }
        out.push_str("],\"honest\":");
        row(&mut out, 5, &g.honest);
        let (h, f) = (&g.attacks.honest, &g.attacks.fake);
        let _ = write!(
            out,
            ",\"payoff\":{:.4},\"attacks\":{{\"honest\":{{\"count\":{},\"scored\":{}}},\"fake\":{{\"count\":{},\"scored\":{},\"answered\":{},\"bitten\":{}}}}}}}",
            g.payoff(),
            h.attempts,
            h.scored,
            f.attempts,
            f.scored,
            f.read,
            f.bit
        );
    }
    out.push_str("]}");
    out
}
