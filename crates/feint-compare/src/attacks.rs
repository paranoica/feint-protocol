use feint_ai::{Bot, Controller, Difficulty, Playbook, RunHeader, Runner};
use feint_protocol::{BodyParamsT, GameAdapter};

pub const OUTCOME_TICKS: u64 = 480;
const STRIKE_INTENT: u8 = 1;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Side {
    pub attempts: u32,
    pub scored: u32,
    pub conceded: u32,
    pub ticks: u64,
    pub bit: u32,
    pub read: u32,
    pub scored_bit: u32,
    pub scored_unbit: u32,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Study {
    pub honest: Side,
    pub fake: Side,
    pub unread: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    start: u64,
    end: Option<u64>,
    struck: bool,
    faked: bool,
    faking: bool,
    shown: Option<u8>,
    answer: Option<u8>,
    scored: bool,
    conceded: bool,
}

#[derive(Debug, Default)]
pub struct Ledger {
    entries: Vec<Entry>,
}

impl Ledger {
    fn open(&mut self) -> Option<&mut Entry> {
        self.entries.last_mut().filter(|e| e.end.is_none())
    }

    pub fn start(&mut self, tick: u64) {
        self.end(tick);
        self.entries.push(Entry {
            start: tick,
            ..Entry::default()
        });
    }

    pub fn end(&mut self, tick: u64) {
        if let Some(e) = self.open() {
            e.end = Some(tick);
        }
    }

    pub fn chose(&mut self, tick: u64, intent: u8, deceptive: bool, shown: Option<u8>) -> bool {
        let Some(e) = self.open().filter(|e| e.start < tick) else {
            return false;
        };
        e.struck |= intent == STRIKE_INTENT;
        let started = deceptive && !e.faking;
        e.faking = deceptive;
        if deceptive {
            e.faked = true;
            if shown.is_some() {
                e.shown = shown;
            }
        }
        started
    }

    pub fn answered(&mut self, covered: u8) {
        if let Some(e) = self.open().filter(|e| e.faked && e.answer.is_none()) {
            e.answer = Some(covered);
        }
    }

    pub fn goal(&mut self, tick: u64, mine: bool) {
        let latest = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.start < tick)
            .filter(|e| e.end.is_none_or(|end| tick <= end + OUTCOME_TICKS));
        if let Some(e) = latest {
            if mine {
                e.scored = true;
            } else {
                e.conceded = true;
            }
        }
    }

    #[must_use]
    pub fn finish(self) -> Study {
        let mut out = Study::default();
        let last = self.entries.iter().filter_map(|e| e.end).max().unwrap_or(0);
        for e in self.entries.iter().filter(|e| e.struck) {
            let side = if e.faked {
                &mut out.fake
            } else {
                &mut out.honest
            };
            side.attempts += 1;
            side.scored += u32::from(e.scored);
            side.conceded += u32::from(e.conceded);
            side.ticks += e.end.unwrap_or(last).saturating_sub(e.start);
            if let (true, Some(shown)) = (e.faked, e.shown) {
                match e.answer {
                    Some(answer) => {
                        side.read += 1;
                        if answer == shown {
                            side.bit += 1;
                            side.scored_bit += u32::from(e.scored);
                        } else {
                            side.scored_unbit += u32::from(e.scored);
                        }
                    }
                    None => out.unread += 1,
                }
            }
        }
        out
    }
}

struct Setup<A: GameAdapter> {
    header: RunHeader,
    seat: u8,
    cap: u64,
    score_of: fn(&A::State) -> [i64; 2],
}

fn study<A, P>(
    mut game: A,
    mut bot: Bot<A, P>,
    reader: &P,
    human: &mut dyn Controller<A>,
    setup: Setup<A>,
    out: &mut Ledger,
) where
    A: GameAdapter,
    P: Playbook<A>,
{
    let Setup {
        header,
        seat,
        cap,
        score_of,
    } = setup;
    let me = usize::from(seat);
    let them = 1 - seat;
    let mut runner = if seat == 0 {
        Runner::start(&mut game, [&mut bot, human], header)
    } else {
        Runner::start(&mut game, [human, &mut bot], header)
    }
    .unwrap_or_else(|e| panic!("start: {e:?}"));
    let mut possession = None;
    let mut reference = game.live().clone();
    let mut score = score_of(game.live());
    while runner.ticks() < cap {
        let stepped = if seat == 0 {
            runner.step(&mut game, [&mut bot as &mut dyn Controller<A>, human])
        } else {
            runner.step(&mut game, [human, &mut bot as &mut dyn Controller<A>])
        };
        let tick = runner.ticks();
        let state = game.live().clone();
        if let Some(covered) = reader.read(&reference, &state, them).covered {
            out.answered(covered);
        }
        let now_possession = reader.possession(&state, seat);
        if now_possession != possession {
            if now_possession.is_some() {
                out.start(tick);
            } else {
                out.end(tick);
            }
            possession = now_possession;
        }
        if let Some(c) = bot.current()
            && out.chose(tick, c.intent, c.deceptive, c.shown)
        {
            reference = state.clone();
        }
        let now = score_of(&state);
        for _ in score[me]..now[me] {
            out.goal(tick, true);
        }
        for _ in score[1 - me]..now[1 - me] {
            out.goal(tick, false);
        }
        score = now;
        if !stepped {
            break;
        }
    }
    out.end(runner.ticks());
}

fn header(
    id: &str,
    version: u32,
    seed: u64,
    bot: BodyParamsT,
    human: BodyParamsT,
    seat: u8,
) -> RunHeader {
    RunHeader {
        game_id: id.into(),
        game_version: version,
        seed,
        bodies: if seat == 0 {
            vec![bot, human]
        } else {
            vec![human, bot]
        },
        levels: [0, 0],
        opponent: feint_ai::Opponent::HandMade,
    }
}

#[must_use]
pub fn attacks(id: &str, matches: u64) -> Study {
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5"));

    let mut s = Ledger::default();
    match id {
        "airhockey" => {
            let params = airhockey::Params {
                goals_to_win: airhockey::human::HUMAN_GOALS,
                ..airhockey::Params::default()
            };
            for seed in 0..matches {
                let seat = (seed % 2) as u8;
                let game = airhockey::AirHockey::new(params.clone(), 1);
                let spec = game.spec();
                let bot = Bot::new(five, airhockey::HockeyPlaybook::new(params.clone()), &spec)
                    .unwrap_or_else(|e| panic!("{e:?}"));
                let reader = airhockey::HockeyPlaybook::new(params.clone());
                let mut human =
                    airhockey::human::Human::new(params.clone(), airhockey::human::HUMAN_NOISE);
                let h = header(
                    "airhockey",
                    airhockey::GAME_VERSION,
                    seed,
                    five.body(&spec),
                    five.body(&spec),
                    seat,
                );
                study(
                    game,
                    bot,
                    &reader,
                    &mut human,
                    Setup {
                        header: h,
                        seat,
                        cap: 240 * 600,
                        score_of: |s: &airhockey::Match| s.score().map(i64::from),
                    },
                    &mut s,
                );
            }
        }
        "penalty" => {
            for seed in 0..matches {
                let seat = (seed % 2) as u8;
                let game = penalty::Penalty::new(penalty::Params::default(), 5);
                let spec = game.spec();
                let bot = Bot::new(
                    five,
                    penalty::PenaltyPlaybook::new(penalty::Params::default()),
                    &spec,
                )
                .unwrap_or_else(|e| panic!("{e:?}"));
                let reader = penalty::PenaltyPlaybook::new(penalty::Params::default());
                let mut human = penalty::human::Human::new();
                let hb = penalty::human::human_body(&game);
                let h = header(
                    "penalty",
                    penalty::GAME_VERSION,
                    seed,
                    five.body(&spec),
                    hb,
                    seat,
                );
                study(
                    game,
                    bot,
                    &reader,
                    &mut human,
                    Setup {
                        header: h,
                        seat,
                        cap: 240 * 900,
                        score_of: |s: &penalty::Shootout| s.score.map(i64::from),
                    },
                    &mut s,
                );
            }
        }
        "duel" => {
            for seed in 0..matches {
                let seat = (seed % 2) as u8;
                let game = duel::QuickDraw::new(duel::Params::default(), 5);
                let spec = game.spec();
                let bot = Bot::new(
                    five,
                    duel::DuelPlaybook::new(duel::Params::default()),
                    &spec,
                )
                .unwrap_or_else(|e| panic!("{e:?}"));
                let reader = duel::DuelPlaybook::new(duel::Params::default());
                let mut human = duel::human::Human::new();
                let hb = duel::human::human_body(&game);
                let h = header("duel", duel::GAME_VERSION, seed, five.body(&spec), hb, seat);
                study(
                    game,
                    bot,
                    &reader,
                    &mut human,
                    Setup {
                        header: h,
                        seat,
                        cap: 240 * 900,
                        score_of: |s: &duel::Duel| s.score.map(i64::from),
                    },
                    &mut s,
                );
            }
        }
        _ => panic!("unknown game {id}"),
    }
    s.finish()
}
