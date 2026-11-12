use std::ops::Range;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, Opponent};
use feint_protocol::{ActionT, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::detect::{Shot, shots};
use crate::play::{Played, human_params, play_against_human};

pub const MATCH_TICKS: u64 = 12;
pub const LIVE_TICKS: u64 = 24;
pub const HELD_TICKS: u64 = 48;
pub const REACH_X: f64 = 0.9;
const REACH_SPLIT: f64 = 0.035;
const THREADS: usize = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calibration {
    pub matches: u64,
    pub reached: u32,
    pub agreed: u32,
    pub unshot: u32,
    pub labelled: u32,
    pub botched: u32,
    pub detected: u32,
    pub found: u32,
    pub caught: u32,
}

fn ratio(a: u32, b: u32) -> Option<f64> {
    (b > 0).then(|| f64::from(a) / f64::from(b))
}

impl Calibration {
    fn per_match(&self, n: u32) -> f64 {
        if self.matches == 0 {
            0.0
        } else {
            f64::from(n) / self.matches as f64
        }
    }

    #[must_use]
    pub fn labelled_rate(&self) -> f64 {
        self.per_match(self.labelled)
    }

    #[must_use]
    pub fn detected_rate(&self) -> f64 {
        self.per_match(self.detected)
    }

    #[must_use]
    pub fn agreement(&self) -> Option<f64> {
        ratio(self.agreed, self.reached)
    }

    #[must_use]
    pub fn precision(&self) -> Option<f64> {
        ratio(self.found, self.detected)
    }

    #[must_use]
    pub fn recall(&self) -> Option<f64> {
        ratio(self.caught, self.labelled)
    }

    fn add(&mut self, o: &Self) {
        self.matches += o.matches;
        self.reached += o.reached;
        self.agreed += o.agreed;
        self.unshot += o.unshot;
        self.labelled += o.labelled;
        self.botched += o.botched;
        self.detected += o.detected;
        self.found += o.found;
        self.caught += o.caught;
    }
}

#[must_use]
pub fn pair(labelled: &[u64], detected: &[u64]) -> (u32, u32) {
    let near = |a: u64, b: u64| a.abs_diff(b) <= MATCH_TICKS;
    let count = |from: &[u64], to: &[u64]| {
        u32::try_from(
            from.iter()
                .filter(|&&a| to.iter().any(|&b| near(a, b)))
                .count(),
        )
        .unwrap_or(u32::MAX)
    };
    (count(detected, labelled), count(labelled, detected))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub tick: u64,
    pub class: u8,
}

pub struct Probe {
    pub bot: Bot<AirHockey, HockeyPlaybook>,
    pub shows: Vec<Shown>,
}

impl Controller<AirHockey> for Probe {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.shows.clear();
        self.bot.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let action = self.bot.on_tick(game, obs);
        if let Some(c) = self.bot.current()
            && c.deceptive
            && let Some(class) = c.shown
        {
            self.shows.push(Shown {
                tick: game.live().tick(),
                class,
            });
        }
        action
    }

    fn on_event(&mut self, event: &EventT) {
        self.bot.on_event(event);
    }

    fn end_match(&mut self, game: &AirHockey, obs: &ObservationT) {
        self.bot.end_match(game, obs);
    }
}

#[must_use]
pub fn held_shows(shows: &[Shown]) -> Vec<(u64, u64, u8)> {
    let mut runs: Vec<(u64, u64, u8)> = Vec::new();
    for w in shows {
        match runs.last_mut() {
            Some((_, end, class)) if *class == w.class && w.tick == *end + 1 => *end = w.tick,
            _ => runs.push((w.tick, w.tick, w.class)),
        }
    }
    runs
}

fn seen(player: u8, v: airhockey::Vec2) -> airhockey::Vec2 {
    if player == 0 { v } else { -v }
}

#[must_use]
pub fn reached_side(
    params: &Params,
    frames: &[airhockey::analysis::Frame],
    contact: u64,
    player: u8,
) -> Option<u8> {
    let reach = params.puck_radius + params.mallet_radius + crate::detect::CONTACT_SLACK;
    let jump = params.puck_speed_cap * params.tick_dt() * 2.0;
    let start = frames.partition_point(|f| f.tick < contact);
    let first = frames.get(start)?;
    let mut touching = [0, 1].map(|i| (first.puck - first.mallets[i]).length() <= reach);
    for k in start + 1..frames.len() {
        let f = &frames[k];
        let prev = &frames[k - 1];
        if f.score != first.score || (f.puck - prev.puck).length() > jump {
            return None;
        }
        let now = [0, 1].map(|i| (f.puck - f.mallets[i]).length() <= reach);
        if (now[0] && !touching[0]) || (now[1] && !touching[1]) {
            return None;
        }
        touching = now;
        let p = seen(player, f.puck);
        if p.x >= REACH_X * params.half_length {
            return Some(if p.y < -REACH_SPLIT {
                0
            } else if p.y > REACH_SPLIT {
                2
            } else {
                1
            });
        }
    }
    None
}

#[must_use]
pub fn judge<C>(params: &Params, played: &Played<C>, shows: &[Shown]) -> Calibration {
    let bot = played.seat;
    let mine: Vec<Shot> = shots(params, &played.frames)
        .into_iter()
        .filter(|s| s.player == bot)
        .collect();
    let mut out = Calibration {
        matches: 1,
        ..Calibration::default()
    };
    let held = held_shows(shows);
    let mut fakes = Vec::new();
    for s in &mine {
        if let Some(side) = reached_side(params, &played.frames, s.contact, bot) {
            out.reached += 1;
            out.agreed += u32::from(side == s.realised);
        }
    }
    let live_at = |contact: u64, from: u64| {
        let live = contact.saturating_sub(LIVE_TICKS).max(from);
        held.iter()
            .rev()
            .find(|&&(start, end, _)| {
                start < contact && end >= live && end.min(contact - 1) + 1 - start >= HELD_TICKS
            })
            .map(|&(_, _, class)| class)
    };
    out.unshot = u32::try_from(
        held.iter()
            .filter(|&&(start, end, _)| end + 1 - start >= HELD_TICKS)
            .filter(|&&(start, end, _)| {
                !mine
                    .iter()
                    .any(|s| s.contact > start && s.contact <= end + LIVE_TICKS)
            })
            .count(),
    )
    .unwrap_or(u32::MAX);
    for s in &mine {
        let shown = live_at(s.contact, s.from);
        match shown {
            Some(class) if class != s.realised => fakes.push(s.contact),
            Some(_) => out.botched += 1,
            None => {}
        }
    }
    let detected: Vec<u64> = mine
        .iter()
        .filter(|s| s.fake())
        .map(|s| s.contact)
        .collect();
    let (found, caught) = pair(&fakes, &detected);
    out.labelled = u32::try_from(fakes.len()).unwrap_or(u32::MAX);
    out.detected = u32::try_from(detected.len()).unwrap_or(u32::MAX);
    out.found = found;
    out.caught = caught;
    out
}

#[must_use]
pub fn calibrate(d: Difficulty, seeds: Range<u64>) -> Calibration {
    let next = AtomicU64::new(seeds.start);
    let total = Mutex::new(Calibration::default());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed >= seeds.end {
                        break;
                    }
                    let seat = (seed % 2) as u8;
                    let played = play_against_human(
                        |spec, params| {
                            let bot = Bot::new(d, HockeyPlaybook::new(params.clone()), spec)
                                .unwrap_or_else(|e| panic!("a preset builds a bot: {e:?}"));
                            let probe = Probe {
                                bot,
                                shows: Vec::new(),
                            };
                            (probe, d.body(spec), Opponent::HandMade)
                        },
                        seed,
                        seat,
                    );
                    let one = judge(&human_params(), &played, &played.bot.shows);
                    total
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .add(&one);
                }
            });
        }
    });
    total
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
