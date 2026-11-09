use feint_protocol::{ActionT, PlayerId};

use crate::params::Params;
use crate::vec2::Vec2;

const ATTACK_INTENT: u8 = 1;
const RUN_GAP_TICKS: u64 = 30;
const SHOW_MIN: f64 = 0.06;
const SHIFT_MIN: f64 = 0.03;
const CONTACT_SLACK: f64 = 0.01;
const SCORE_WINDOW_TICKS: u64 = 240;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub tick: u64,
    pub puck: Vec2,
    pub mallets: [Vec2; 2],
    pub score: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub player: PlayerId,
    pub start: u64,
    pub end: u64,
    pub deceptive: bool,
    pub contact: Option<u64>,
    pub shown: Option<u8>,
    pub shifted: bool,
    pub scored: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub fakes: u32,
    pub shifted: u32,
    pub fake_goals: u32,
    pub honest: u32,
    pub honest_goals: u32,
}

#[derive(Clone, Copy)]
struct Run {
    start: u64,
    end: u64,
    until: u64,
    deceptive: bool,
}

fn seen(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

fn runs(actions: &[ActionT], player: PlayerId) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut open: Option<Run> = None;
    for a in actions.iter().filter(|a| a.player == player) {
        if let Some(run) = &mut open {
            run.until = run.until.min(a.tick);
        }
        let attack = a.intent == ATTACK_INTENT;
        match &mut open {
            Some(run)
                if attack
                    && a.deceptive == run.deceptive
                    && a.tick.saturating_sub(run.end) <= RUN_GAP_TICKS =>
            {
                run.end = a.tick;
                run.until = a.tick.saturating_add(RUN_GAP_TICKS);
            }
            _ => {
                out.extend(open.take());
                open = attack.then_some(Run {
                    start: a.tick,
                    end: a.tick,
                    until: a.tick.saturating_add(RUN_GAP_TICKS),
                    deceptive: a.deceptive,
                });
            }
        }
    }
    out.extend(open);
    out
}

fn between(frames: &[Frame], from: u64, to: u64) -> impl Iterator<Item = &Frame> {
    frames.iter().filter(move |f| f.tick >= from && f.tick < to)
}

fn first_at(frames: &[Frame], tick: u64) -> Option<&Frame> {
    between(frames, tick, u64::MAX).next()
}

#[must_use]
pub fn attempts(params: &Params, frames: &[Frame], actions: &[ActionT]) -> Vec<Attempt> {
    let reach = params.puck_radius + params.mallet_radius + CONTACT_SLACK;
    let mut out = Vec::new();
    for player in 0..2u8 {
        let me = usize::from(player);
        let them = 1 - me;
        let first = out.len();
        for run in runs(actions, player) {
            let contact = between(frames, run.start + 1, run.until + 1)
                .find(|f| (f.puck - f.mallets[me]).length() <= reach)
                .map(|f| f.tick);
            let mut shown = None;
            let mut widest = 0.0;
            for f in between(frames, run.start + 1, contact.unwrap_or(run.until) + 1) {
                let rel = seen(player, f.mallets[me] - f.puck);
                if rel.x < 0.0 && rel.y.abs() > widest {
                    widest = rel.y.abs();
                    shown = (widest >= SHOW_MIN).then_some(if rel.y > 0.0 { 0 } else { 2 });
                }
            }
            let lateral =
                |tick: u64| first_at(frames, tick).map(|f| seen(player, f.mallets[them]).y);
            let shifted = match (shown, lateral(run.start), contact.and_then(lateral)) {
                (Some(0), Some(a), Some(b)) => b - a < -SHIFT_MIN,
                (Some(_), Some(a), Some(b)) => b - a > SHIFT_MIN,
                _ => false,
            };
            out.push(Attempt {
                player,
                start: run.start,
                end: run.end,
                deceptive: run.deceptive,
                contact,
                shown,
                shifted,
                scored: false,
            });
        }
        let mine = &mut out[first..];
        for pair in frames.windows(2) {
            if pair[1].score[me] <= pair[0].score[me] {
                continue;
            }
            let goal = pair[1].tick;
            let last = mine
                .iter_mut()
                .filter(|a| {
                    a.contact
                        .is_some_and(|c| c <= goal && goal - c <= SCORE_WINDOW_TICKS)
                })
                .max_by_key(|a| a.contact);
            if let Some(a) = last {
                a.scored = true;
            }
        }
    }
    out
}

#[must_use]
pub fn tally(attempts: &[Attempt], player: PlayerId) -> Tally {
    let mut t = Tally::default();
    for a in attempts
        .iter()
        .filter(|a| a.player == player && a.contact.is_some())
    {
        if a.deceptive {
            if a.shown.is_some() {
                t.fakes += 1;
                t.shifted += u32::from(a.shifted);
                t.fake_goals += u32::from(a.scored);
            }
        } else {
            t.honest += 1;
            t.honest_goals += u32::from(a.scored);
        }
    }
    t
}
