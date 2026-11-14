use std::ops::Range;

use feint_ai::{Bot, Difficulty, Opponent, Predictor, RunHeader, Runner, run_match};
use feint_protocol::{GameAdapter, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

pub use crate::human::{Habits, Kick};
use crate::human::{Human, human_body};
use crate::{Params, Penalty, PenaltyPlaybook};

pub const TEST_PERSONS: u64 = 1 << 40;

impl Habits {
    #[must_use]
    pub fn person(id: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(id ^ 0x5045_5253_4f4e);
        let mut unit = || (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32;
        let favourite = ((unit() * 3.0) as usize).min(2);
        let strength = 0.4 + 0.5 * unit();
        let mut corner = [(1.0 - strength) / 2.0; 3];
        corner[favourite] = strength;
        let honest = 0.9 * unit();
        let fake = (1.0 - honest) * unit();
        let straighten = 0.5 * unit();
        let guess = unit();
        let follows_lean = unit() < 0.5;
        let noise = 0.5 + unit();
        Self {
            corner,
            honest,
            fake,
            straighten,
            guess,
            follows_lean,
            noise,
        }
    }
}

pub struct Blind;

impl Predictor for Blind {
    fn likely_shot(&self) -> Option<u8> {
        None
    }

    fn likely_cover(&self, _shown: u8) -> Option<u8> {
        None
    }

    fn likely_real(&self, _shown: u8) -> Option<u8> {
        None
    }

    fn their_odds(&self, _shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        None
    }

    fn knowing_beats_reading(&self) -> bool {
        false
    }
}

pub struct Oracle {
    habits: Habits,
}

impl Oracle {
    #[must_use]
    pub fn new(habits: Habits) -> Self {
        Self { habits }
    }
}

impl Predictor for Oracle {
    fn likely_shot(&self) -> Option<u8> {
        let c = self.habits.corner;
        let mut best = 0;
        for (i, &w) in c.iter().enumerate() {
            if w > c[best] {
                best = i;
            }
        }
        u8::try_from(best).ok()
    }

    fn likely_cover(&self, shown: u8) -> Option<u8> {
        if self.habits.guess < 0.5 || shown == 1 || shown > 2 {
            return None;
        }
        Some(if self.habits.follows_lean {
            shown
        } else {
            2 - shown
        })
    }

    fn likely_real(&self, _shown: u8) -> Option<u8> {
        None
    }

    fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        if shown == 1 || shown > 2 {
            return None;
        }
        let w = self.habits.corner.map(f64::from);
        let same = w[usize::from(shown)] * f64::from(self.habits.honest);
        let other = w[usize::from(2 - shown)] * f64::from(self.habits.fake);
        let total = same + other;
        if total <= 0.0 {
            return None;
        }
        let mut odds = Vec::new();
        for (class, weight) in [(shown, same), (2 - shown, other)] {
            if weight > 0.0 {
                odds.push((Some(class), weight / total));
            }
        }
        Some(odds)
    }

    fn knowing_beats_reading(&self) -> bool {
        false
    }

    fn shot_odds(&self) -> Option<Vec<(u8, f64)>> {
        let c = self.habits.corner.map(f64::from);
        let total: f64 = c.iter().sum();
        (total > 0.0).then(|| (0..3u8).zip(c).map(|(k, w)| (k, w / total)).collect())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    Count,
    Blind,
    Oracle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KickResult {
    pub bot_shoots: bool,
    pub bot_won: bool,
}

#[must_use]
pub fn measured_params() -> Params {
    Params {
        play_out: true,
        max_kicks: 2 * Params::default().kicks_each,
        ..Params::default()
    }
}

fn header(d: Difficulty, game: &Penalty, me: PlayerId, seed: u64) -> RunHeader {
    let spec = game.spec();
    let bodies = if me == 0 {
        vec![d.body(&spec), human_body(game)]
    } else {
        vec![human_body(game), d.body(&spec)]
    };
    RunHeader {
        game_id: "penalty".into(),
        game_version: crate::GAME_VERSION,
        seed,
        bodies,
        levels: if me == 0 { [5, 0] } else { [0, 5] },
        opponent: Opponent::HandMade,
    }
}

#[must_use]
pub fn one_match(d: Difficulty, advice: Advice, person: u64) -> Vec<KickResult> {
    let params = measured_params();
    let habits = Habits::person(person);
    let me: PlayerId = if person.is_multiple_of(2) { 0 } else { 1 };
    let mut game = Penalty::new(params.clone(), 5);
    let spec = game.spec();
    let mut bot = Bot::new(d, PenaltyPlaybook::new(params), &spec)
        .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
    match advice {
        Advice::Count => {}
        Advice::Blind => bot.advise(Box::new(Blind)),
        Advice::Oracle => bot.advise(Box::new(Oracle::new(habits))),
    }
    let mut human = Human::with_habits(habits);
    let head = header(d, &game, me, person);
    let mut kicks = Vec::new();
    let [first, second]: [&mut dyn feint_ai::Controller<Penalty>; 2] = if me == 0 {
        [&mut bot, &mut human]
    } else {
        [&mut human, &mut bot]
    };
    let mut runner = Runner::start(&mut game, [&mut *first, &mut *second], head)
        .unwrap_or_else(|e| panic!("a measured match starts: {e:?}"));
    let mut ticks = 0;
    let mut start = game.live().clone();
    while ticks < 240 * 900 {
        if !runner.step(&mut game, [&mut *first, &mut *second]) {
            break;
        }
        ticks += 1;
        let after = game.live();
        if after.kicks != start.kicks {
            let shooter = start.shooter();
            let scored = after.score[usize::from(shooter)] > start.score[usize::from(shooter)];
            let bot_shoots = shooter == me;
            kicks.push(KickResult {
                bot_shoots,
                bot_won: scored == bot_shoots,
            });
            start = after.clone();
        }
    }
    kicks
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Halves {
    pub matches: u32,
    pub shooter: [[u32; 2]; 2],
    pub keeper: [[u32; 2]; 2],
}

impl Halves {
    #[must_use]
    pub fn share(&self, half: usize) -> f64 {
        let won = self.shooter[half][0] + self.keeper[half][0];
        let kicks = self.shooter[half][1] + self.keeper[half][1];
        f64::from(won) / f64::from(kicks.max(1))
    }

    #[must_use]
    pub fn difference(&self) -> (f64, f64) {
        let n = |h: usize| f64::from((self.shooter[h][1] + self.keeper[h][1]).max(1));
        let (a, b) = (self.share(0), self.share(1));
        let se = (a * (1.0 - a) / n(0) + b * (1.0 - b) / n(1)).sqrt();
        (b - a, se)
    }
}

#[must_use]
pub fn halves(d: Difficulty, advice: Advice, persons: Range<u64>) -> Halves {
    let regulation = 2 * Params::default().kicks_each as usize;
    let mut h = Halves::default();
    for person in persons {
        let kicks = one_match(d, advice, person);
        for (k, kick) in kicks.iter().take(regulation).enumerate() {
            let half = usize::from(k >= regulation / 2);
            let row = if kick.bot_shoots {
                &mut h.shooter[half]
            } else {
                &mut h.keeper[half]
            };
            row[0] += u32::from(kick.bot_won);
            row[1] += 1;
        }
        h.matches += 1;
    }
    h
}

#[must_use]
pub fn digest_against(d: Difficulty, mut human: Human, seed: u64) -> u64 {
    let mut game = Penalty::new(Params::default(), 3);
    let spec = game.spec();
    let mut bot = Bot::new(d, PenaltyPlaybook::new(Params::default()), &spec)
        .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
    let head = header(d, &game, 0, seed);
    run_match(&mut game, [&mut bot, &mut human], head, 240 * 900)
        .unwrap_or_else(|e| panic!("a match runs: {e:?}"))
        .digest
}
