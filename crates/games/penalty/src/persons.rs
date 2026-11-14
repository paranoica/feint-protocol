use std::ops::Range;
use std::sync::{Arc, Mutex};

use feint_ai::{Advisor, Bot, Difficulty, Opponent, Predictor, RunHeader, Runner, run_match};
use feint_protocol::{GameAdapter, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::context::{KickToken, Tokenizer};
pub use crate::human::{Habits, Kick};
use crate::human::{Human, human_body};
use crate::{Params, Penalty, PenaltyPlaybook, Shootout};

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

impl Advisor<Shootout> for Blind {}

impl Advisor<Shootout> for Oracle {}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Played {
    pub me: PlayerId,
    pub kicks: Vec<KickResult>,
    pub score: [u32; 2],
}

#[must_use]
pub fn measured_params() -> Params {
    Params {
        play_out: true,
        max_kicks: 2 * Params::default().kicks_each,
        ..Params::default()
    }
}

fn preset(level: u8) -> Difficulty {
    Difficulty::preset(level).unwrap_or_else(|| panic!("no level {level}"))
}

fn header(level: u8, game: &Penalty, me: PlayerId, seed: u64) -> RunHeader {
    let spec = game.spec();
    let d = preset(level);
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
        levels: if me == 0 { [level, 0] } else { [0, level] },
        opponent: Opponent::HandMade,
    }
}

pub type MakeAdvisor<'a> = &'a dyn Fn(Habits) -> Option<Box<dyn Advisor<Shootout>>>;

fn advisor_for(advice: Advice, habits: Habits) -> Option<Box<dyn Advisor<Shootout>>> {
    match advice {
        Advice::Count => None,
        Advice::Blind => Some(Box::new(Blind)),
        Advice::Oracle => Some(Box::new(Oracle::new(habits))),
    }
}

#[must_use]
pub fn one_match(level: u8, advice: Advice, person: u64) -> Played {
    one_match_with(level, person, &|h| advisor_for(advice, h))
}

#[must_use]
pub fn one_match_with(level: u8, person: u64, make: MakeAdvisor<'_>) -> Played {
    let params = measured_params();
    let habits = Habits::person(person);
    let me: PlayerId = if person.is_multiple_of(2) { 0 } else { 1 };
    let mut game = Penalty::new(params.clone(), level);
    let spec = game.spec();
    let mut bot = Bot::new(preset(level), PenaltyPlaybook::new(params), &spec)
        .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
    if let Some(advisor) = make(habits) {
        bot.advise(advisor);
    }
    let mut human = Human::with_habits(habits);
    let head = header(level, &game, me, person);
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
    Played {
        me,
        kicks,
        score: game.live().score,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchHalves {
    pub person: u64,
    pub shooter: [[u32; 2]; 2],
    pub keeper: [[u32; 2]; 2],
}

impl MatchHalves {
    fn share(&self, half: usize) -> f64 {
        let won = self.shooter[half][0] + self.keeper[half][0];
        let kicks = self.shooter[half][1] + self.keeper[half][1];
        f64::from(won) / f64::from(kicks.max(1))
    }

    #[must_use]
    pub fn difference(&self) -> f64 {
        self.share(1) - self.share(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Halves {
    pub matches: Vec<MatchHalves>,
}

fn mean_se(values: impl Iterator<Item = f64> + Clone) -> (f64, f64) {
    let n = values.clone().count() as f64;
    if n < 2.0 {
        return (values.sum::<f64>() / n.max(1.0), f64::NAN);
    }
    let mean = values.clone().sum::<f64>() / n;
    let var = values.map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    (mean, (var / n).sqrt())
}

impl Halves {
    fn pooled(&self, half: usize, role: impl Fn(&MatchHalves) -> [[u32; 2]; 2]) -> f64 {
        let (won, n) = self.matches.iter().fold((0, 0), |(w, n), m| {
            let r = role(m)[half];
            (w + r[0], n + r[1])
        });
        f64::from(won) / f64::from(n.max(1))
    }

    #[must_use]
    pub fn share(&self, half: usize) -> f64 {
        let (won, n) = self.matches.iter().fold((0, 0), |(w, n), m| {
            (
                w + m.shooter[half][0] + m.keeper[half][0],
                n + m.shooter[half][1] + m.keeper[half][1],
            )
        });
        f64::from(won) / f64::from(n.max(1))
    }

    #[must_use]
    pub fn shooter(&self, half: usize) -> f64 {
        self.pooled(half, |m| m.shooter)
    }

    #[must_use]
    pub fn keeper(&self, half: usize) -> f64 {
        self.pooled(half, |m| m.keeper)
    }

    #[must_use]
    pub fn difference(&self) -> (f64, f64) {
        mean_se(self.matches.iter().map(MatchHalves::difference))
    }

    #[must_use]
    pub fn against(&self, control: &Self) -> (f64, f64) {
        let paired = self.matches.iter().filter_map(|m| {
            control
                .matches
                .iter()
                .find(|c| c.person == m.person)
                .map(|c| m.difference() - c.difference())
        });
        mean_se(paired)
    }
}

#[must_use]
pub fn halves(level: u8, advice: Advice, persons: Range<u64>) -> Halves {
    halves_with(level, persons, &|h| advisor_for(advice, h))
}

#[must_use]
pub fn halves_with(level: u8, persons: Range<u64>, make: MakeAdvisor<'_>) -> Halves {
    let regulation = 2 * Params::default().kicks_each as usize;
    let mut h = Halves::default();
    for person in persons {
        let played = one_match_with(level, person, make);
        let mut m = MatchHalves {
            person,
            ..MatchHalves::default()
        };
        for (k, kick) in played.kicks.iter().take(regulation).enumerate() {
            let half = usize::from(k >= regulation / 2);
            let row = if kick.bot_shoots {
                &mut m.shooter[half]
            } else {
                &mut m.keeper[half]
            };
            row[0] += u32::from(kick.bot_won);
            row[1] += 1;
        }
        h.matches.push(m);
    }
    h
}

#[must_use]
pub fn digest_at(level: u8, seed: u64) -> u64 {
    let mut game = Penalty::new(Params::default(), level);
    let spec = game.spec();
    let mut bot = Bot::new(
        preset(level),
        PenaltyPlaybook::new(Params::default()),
        &spec,
    )
    .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
    let mut human = Human::new();
    let head = header(level, &game, 0, seed);
    run_match(&mut game, [&mut bot, &mut human], head, 240 * 900)
        .unwrap_or_else(|e| panic!("a match runs: {e:?}"))
        .digest
}

struct Recorder {
    inner: Box<dyn Advisor<Shootout>>,
    tokens: Arc<Mutex<Tokenizer>>,
    states: Option<Arc<Mutex<Vec<Shootout>>>>,
}

impl Predictor for Recorder {
    fn likely_shot(&self) -> Option<u8> {
        self.inner.likely_shot()
    }

    fn likely_cover(&self, shown: u8) -> Option<u8> {
        self.inner.likely_cover(shown)
    }

    fn likely_real(&self, shown: u8) -> Option<u8> {
        self.inner.likely_real(shown)
    }

    fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        self.inner.their_odds(shown)
    }

    fn knowing_beats_reading(&self) -> bool {
        self.inner.knowing_beats_reading()
    }

    fn shot_odds(&self) -> Option<Vec<(u8, f64)>> {
        self.inner.shot_odds()
    }
}

impl Advisor<Shootout> for Recorder {
    fn new_match(&mut self) {
        self.inner.new_match();
    }

    fn see(&mut self, state: &Shootout, me: PlayerId) {
        self.inner.see(state, me);
        if let Ok(mut t) = self.tokens.lock() {
            t.see(state, me);
        }
        if let Some(Ok(mut kept)) = self.states.as_ref().map(|s| s.lock()) {
            kept.push(state.clone());
        }
    }
}

#[must_use]
pub fn match_tokens(
    level: u8,
    advice: Advice,
    habits: Habits,
    seat: PlayerId,
    seed: u64,
) -> Vec<KickToken> {
    recorded(level, advice, habits, seat, seed, false).0
}

#[must_use]
pub fn match_states(level: u8, habits: Habits, seat: PlayerId, seed: u64) -> Vec<Shootout> {
    recorded(level, Advice::Blind, habits, seat, seed, true).1
}

fn recorded(
    level: u8,
    advice: Advice,
    habits: Habits,
    seat: PlayerId,
    seed: u64,
    keep_states: bool,
) -> (Vec<KickToken>, Vec<Shootout>) {
    let params = measured_params();
    let mut game = Penalty::new(params.clone(), level);
    let spec = game.spec();
    let mut bot = Bot::new(preset(level), PenaltyPlaybook::new(params), &spec)
        .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
    let inner: Box<dyn Advisor<Shootout>> = match advice {
        Advice::Oracle => Box::new(Oracle::new(habits)),
        Advice::Blind => Box::new(Blind),
        Advice::Count => {
            panic!("recording replaces the count model's answers: ask for Blind or Oracle")
        }
    };
    let tokens = Arc::new(Mutex::new(Tokenizer::default()));
    let states = keep_states.then(|| Arc::new(Mutex::new(Vec::new())));
    bot.advise(Box::new(Recorder {
        inner,
        tokens: Arc::clone(&tokens),
        states: states.clone(),
    }));
    let mut human = Human::with_habits(habits);
    let head = header(level, &game, seat, seed);
    let played = if seat == 0 {
        run_match(&mut game, [&mut bot, &mut human], head, 240 * 900)
    } else {
        run_match(&mut game, [&mut human, &mut bot], head, 240 * 900)
    };
    played.unwrap_or_else(|e| panic!("a measured match runs: {e:?}"));
    let tokens = tokens
        .lock()
        .map(|t| t.tokens().to_vec())
        .unwrap_or_default();
    let states = states
        .and_then(|s| s.lock().ok().map(|v| v.clone()))
        .unwrap_or_default();
    (tokens, states)
}
