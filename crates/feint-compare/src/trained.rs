use std::fmt::Write;
use std::num::NonZeroU64;
use std::ops::Range;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use airhockey::HockeyPlaybook;
use airhockey::analysis::Frame;
use feint_ai::{Bot, Difficulty, Opponent};
use feint_ml::mlp::Mlp;
use feint_ml::registry::for_level;
use feint_ml::show::{class, seen};
use feint_ml::trained::{TrainedPilot, trained_pilot, trained_pilot_from};

use crate::detect::{Shot, shots};
use crate::play::{Played, human_params, play_against_human};

const THREADS: usize = 7;
const LINKS: usize = 3;
const Z: f64 = 1.959_963_984_540_054;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub seed: u64,
    pub seat: u8,
    pub contact: u64,
    pub scored: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrainedReport {
    pub level: u8,
    pub checkpoint: u64,
    pub matches: u64,
    pub noise: f64,
    pub shots: u32,
    pub fakes: u32,
    pub fake_scored: u32,
    pub honest: u32,
    pub honest_scored: u32,
    pub leaned: u32,
    pub links: Vec<Link>,
}

pub enum Source {
    Shipped,
    File(Box<Mlp>, u64),
}

#[must_use]
pub fn leaned_to_shown(frames: &[Frame], shot: &Shot) -> bool {
    let Some(shown) = shot.shown else {
        return false;
    };
    let at = frames.partition_point(|f| f.tick < shot.contact);
    let Some(frame) = frames.get(at) else {
        return false;
    };
    let defender = frame.mallets[usize::from(1 - shot.player)];
    class(seen(shot.player, defender).y) == shown
}

#[must_use]
pub fn wilson(k: u32, n: u32) -> Option<(f64, f64)> {
    if n == 0 {
        return None;
    }
    let p = f64::from(k) / f64::from(n);
    let n = f64::from(n);
    let centre = (p + Z * Z / (2.0 * n)) / (1.0 + Z * Z / n);
    let half = Z * (p * (1.0 - p) / n + Z * Z / (4.0 * n * n)).sqrt() / (1.0 + Z * Z / n);
    Some(((centre - half).max(0.0), (centre + half).min(1.0)))
}

#[must_use]
pub fn play_trained(level: u8, seed: u64, seat: u8) -> Played<TrainedPilot> {
    play_trained_from(level, &Source::Shipped, seed, seat)
}

#[must_use]
pub fn play_trained_from(level: u8, source: &Source, seed: u64, seat: u8) -> Played<TrainedPilot> {
    let checkpoint = match source {
        Source::Shipped => {
            for_level(level)
                .unwrap_or_else(|| panic!("level {level} has a checkpoint"))
                .id
        }
        Source::File(_, id) => *id,
    };
    play_against_human(
        |spec, _| {
            let pilot = match source {
                Source::Shipped => trained_pilot(level, checkpoint, spec),
                Source::File(mlp, _) => trained_pilot_from(level, mlp.as_ref().clone(), spec),
            }
            .unwrap_or_else(|e| panic!("the checkpoint builds: {e:?}"));
            let body = Difficulty::preset(level)
                .unwrap_or_else(|| panic!("level {level} exists"))
                .body(spec);
            let id = NonZeroU64::new(checkpoint).unwrap_or(NonZeroU64::MIN);
            (pilot, body, Opponent::Trained(id))
        },
        seed,
        seat,
    )
}

#[must_use]
pub fn trained_report(level: u8, seeds: Range<u64>) -> TrainedReport {
    trained_report_from(level, &Source::Shipped, seeds)
}

#[must_use]
pub fn trained_report_from(level: u8, source: &Source, seeds: Range<u64>) -> TrainedReport {
    let params = human_params();
    let next = AtomicU64::new(seeds.start);
    let found = Mutex::new((
        TrainedReport {
            level,
            checkpoint: match source {
                Source::Shipped => for_level(level).map_or(0, |c| c.id),
                Source::File(_, id) => *id,
            },
            matches: 0,
            noise: 0.0,
            shots: 0,
            fakes: 0,
            fake_scored: 0,
            honest: 0,
            honest_scored: 0,
            leaned: 0,
            links: Vec::new(),
        },
        Vec::new(),
    ));
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed >= seeds.end {
                        break;
                    }
                    let seat = (seed % 2) as u8;
                    let played = play_trained_from(level, source, seed, seat);
                    let mine: Vec<_> = shots(&params, &played.frames)
                        .into_iter()
                        .filter(|s| s.player == seat)
                        .collect();
                    let mut guard = found
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let (r, links) = &mut *guard;
                    r.matches += 1;
                    for shot in &mine {
                        r.shots += 1;
                        if shot.fake() {
                            r.fakes += 1;
                            r.leaned += u32::from(leaned_to_shown(&played.frames, shot));
                            r.fake_scored += u32::from(shot.scored);
                            links.push(Link {
                                seed,
                                seat,
                                contact: shot.contact,
                                scored: shot.scored,
                            });
                        } else {
                            r.honest += 1;
                            r.honest_scored += u32::from(shot.scored);
                        }
                    }
                }
            });
        }
    });
    let (mut r, links) = found
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    r.links = pick(links);
    r
}

#[must_use]
pub fn hand_made_leans(seeds: Range<u64>) -> (u32, u32) {
    let params = human_params();
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5 exists"));
    let next = AtomicU64::new(seeds.start);
    let found = Mutex::new((0u32, 0u32));
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
                        |spec, p| {
                            let bot = Bot::new(five, HockeyPlaybook::new(p.clone()), spec)
                                .unwrap_or_else(|e| panic!("level 5 builds: {e:?}"));
                            (bot, five.body(spec), Opponent::HandMade)
                        },
                        seed,
                        seat,
                    );
                    let fakes: Vec<Shot> = shots(&params, &played.frames)
                        .into_iter()
                        .filter(|s| s.player == seat && s.fake())
                        .collect();
                    let leaned = fakes
                        .iter()
                        .filter(|s| leaned_to_shown(&played.frames, s))
                        .count();
                    let mut guard = found
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.0 += fakes.len() as u32;
                    guard.1 += leaned as u32;
                }
            });
        }
    });
    found
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[must_use]
pub fn pick(mut links: Vec<Link>) -> Vec<Link> {
    links.sort_by_key(|l| (!l.scored, l.seed, l.contact));
    links.truncate(LINKS);
    links
}

fn side(out: &mut String, name: &str, count: u32, scored: u32) {
    let (low, high) = wilson(scored, count)
        .map_or(("null".to_string(), "null".to_string()), |(l, h)| {
            (format!("{l:.4}"), format!("{h:.4}"))
        });
    let _ = write!(
        out,
        "\"{name}\":{{\"count\":{count},\"scored\":{scored},\"low\":{low},\"high\":{high}}}"
    );
}

impl TrainedReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = format!(
            "{{\"level\":{},\"checkpoint\":\"{:016x}\",\"matches\":{},\"noise\":{:.6},\"shots\":{},",
            self.level, self.checkpoint, self.matches, self.noise, self.shots
        );
        side(&mut out, "fake", self.fakes, self.fake_scored);
        out.push(',');
        side(&mut out, "honest", self.honest, self.honest_scored);
        out.push_str(",\"links\":[");
        for (i, l) in self.links.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"seed\":{},\"seat\":{},\"from\":{},\"scored\":{}}}",
                l.seed, l.seat, l.contact, l.scored
            );
        }
        out.push_str("]}");
        out
    }
}
