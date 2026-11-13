use std::ops::Range;

use feint_ai::{Controller, Opponent, RunHeader, run_match};
use feint_protocol::BodyParamsT;
use feint_turn::{TurnPilot, TurnPolicy};

use crate::ledger::{Fact, Ledger};
use crate::rules::{Move, Params};
use crate::{Believe, GAME_VERSION, Scripted, Style};

pub const Z: f64 = 1.96;
pub const MAX_TICKS: u64 = 10_000;

#[must_use]
pub fn wilson(successes: f64, n: f64) -> (f64, f64) {
    if n <= 0.0 {
        return (0.0, 1.0);
    }
    let p = successes / n;
    let z2 = Z * Z;
    let scale = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / scale;
    let half = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / scale;
    ((center - half).max(0.0), (center + half).min(1.0))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub lays: u32,
    pub false_lays: u32,
    pub lies_called: u32,
    pub lies_survived: u32,
    pub calls: u32,
    pub calls_hit: u32,
}

impl Tally {
    #[must_use]
    pub fn matches(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    #[must_use]
    pub fn share(&self) -> f64 {
        let n = f64::from(self.matches());
        if n == 0.0 {
            return 0.0;
        }
        (f64::from(self.wins) + f64::from(self.draws) / 2.0) / n
    }

    #[must_use]
    pub fn interval(&self) -> (f64, f64) {
        let n = f64::from(self.matches());
        wilson(self.share() * n, n)
    }

    fn add(&mut self, other: &Self) {
        self.wins += other.wins;
        self.draws += other.draws;
        self.losses += other.losses;
        self.lays += other.lays;
        self.false_lays += other.false_lays;
        self.lies_called += other.lies_called;
        self.lies_survived += other.lies_survived;
        self.calls += other.calls;
        self.calls_hit += other.calls_hit;
    }
}

#[must_use]
pub fn false_lay(m: &Move) -> bool {
    match m {
        Move::Call => false,
        Move::Lay { claim, counts } => counts
            .iter()
            .enumerate()
            .any(|(rank, &c)| c > 0 && rank != usize::from(*claim)),
    }
}

#[derive(Clone, Debug)]
pub struct Referee {
    me: u8,
    ledger: Ledger,
    last_false: bool,
    pending_lie: bool,
    tally: Tally,
}

impl Referee {
    #[must_use]
    pub fn new(me: u8, params: &Params) -> Self {
        Self {
            me,
            ledger: Ledger::new(params),
            last_false: false,
            pending_lie: false,
            tally: Tally::default(),
        }
    }

    pub fn record(&mut self, player: u8, m: &Move) {
        let mine = player == self.me;
        for fact in self.ledger.record(player, m) {
            match fact {
                Fact::Out { .. } => {}
                Fact::Call { .. } => self.call(mine),
                Fact::Lay { .. } => self.lay(mine, false_lay(m)),
            }
        }
    }

    fn call(&mut self, mine: bool) {
        if mine {
            self.tally.calls += 1;
            self.tally.calls_hit += u32::from(self.last_false);
        } else if self.pending_lie {
            self.tally.lies_called += 1;
        }
        self.pending_lie = false;
    }

    fn lay(&mut self, mine: bool, lie: bool) {
        if mine {
            self.tally.lays += 1;
            self.tally.false_lays += u32::from(lie);
            self.pending_lie = lie;
        } else if self.pending_lie {
            self.tally.lies_survived += 1;
            self.pending_lie = false;
        }
        self.last_false = lie;
    }

    pub fn end_match(&mut self, score: [i64; 2]) {
        if self.pending_lie {
            self.tally.lies_survived += 1;
            self.pending_lie = false;
        }
        let mine = score[usize::from(self.me.min(1))];
        let theirs = score[usize::from(1 - self.me.min(1))];
        match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => self.tally.wins += 1,
            std::cmp::Ordering::Equal => self.tally.draws += 1,
            std::cmp::Ordering::Less => self.tally.losses += 1,
        }
    }

    #[must_use]
    pub fn tally(&self) -> Tally {
        self.tally
    }
}

pub fn check_against(
    policy: &TurnPolicy,
    style: Style,
    seeds: Range<u64>,
) -> Result<Tally, String> {
    let params = Params::default();
    let mut total = Tally::default();
    let start = seeds.start;
    for seed in seeds {
        let me = ((seed - start) % 2) as u8;
        let mut game = Believe::new(params.clone()).map_err(str::to_string)?;
        let moves = game.moves().clone();
        let mut pilot = TurnPilot::new(policy.clone());
        let mut scripted = Scripted::new(style);
        let header = RunHeader {
            game_id: "believe".into(),
            game_version: GAME_VERSION,
            seed,
            bodies: vec![BodyParamsT::default(); 2],
            levels: [0, 0],
            opponent: Opponent::HandMade,
        };
        let controllers: [&mut dyn Controller<Believe>; 2] = if me == 0 {
            [&mut pilot, &mut scripted]
        } else {
            [&mut scripted, &mut pilot]
        };
        let report =
            run_match(&mut game, controllers, header, MAX_TICKS).map_err(|e| e.to_string())?;
        if !report.finished {
            return Err(format!("seed {seed}: the match did not finish"));
        }
        let mut referee = Referee::new(me, &params);
        for action in &report.actions {
            let m = moves
                .decode(action.discrete)
                .ok_or_else(|| format!("seed {seed}: action {} is not a move", action.discrete))?;
            referee.record(action.player, &m);
        }
        let score = [
            report.score.first().copied().unwrap_or(0),
            report.score.get(1).copied().unwrap_or(0),
        ];
        referee.end_match(score);
        total.add(&referee.tally());
    }
    Ok(total)
}
