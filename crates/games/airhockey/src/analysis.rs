use std::borrow::Cow;

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
                if let Some(run) = &mut open {
                    run.until = a.tick;
                }
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

fn span(frames: &[Frame], from: u64, to: u64) -> std::ops::Range<usize> {
    let lo = frames.partition_point(|f| f.tick < from);
    let hi = frames.partition_point(|f| f.tick <= to).max(lo);
    lo..hi
}

fn first_at(frames: &[Frame], tick: u64) -> Option<&Frame> {
    frames.get(span(frames, tick, u64::MAX).start)
}

fn touch_starts(frames: &[Frame], reach: f64) -> Vec<[bool; 2]> {
    let touching = |f: &Frame, i: usize| (f.puck - f.mallets[i]).length() <= reach;
    let mut was = [false; 2];
    frames
        .iter()
        .map(|f| {
            let now = [touching(f, 0), touching(f, 1)];
            let starts = [now[0] && !was[0], now[1] && !was[1]];
            was = now;
            starts
        })
        .collect()
}

#[must_use]
pub fn attempts(params: &Params, frames: &[Frame], actions: &[ActionT]) -> Vec<Attempt> {
    let reach = params.puck_radius + params.mallet_radius + CONTACT_SLACK;
    let mut frames = Cow::Borrowed(frames);
    if !frames.is_sorted_by_key(|f| f.tick) {
        frames.to_mut().sort_by_key(|f| f.tick);
    }
    let mut actions = Cow::Borrowed(actions);
    if !actions.is_sorted_by_key(|a| a.tick) {
        actions.to_mut().sort_by_key(|a| a.tick);
    }
    let frames = frames.as_ref();
    let starts = touch_starts(frames, reach);
    let mut out = Vec::new();
    for player in 0..2u8 {
        let me = usize::from(player);
        let them = 1 - me;
        let first = out.len();
        let found = runs(&actions, player);
        for run in &found {
            let window = span(frames, run.start.saturating_add(1), run.until);
            let contact = window
                .clone()
                .find(|&k| starts[k][me])
                .map(|k| frames[k].tick);
            let mut shown = None;
            let mut widest = 0.0;
            let shown_until = contact.unwrap_or(run.until);
            for f in &frames[span(frames, run.start.saturating_add(1), shown_until)] {
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
        for k in 1..frames.len() {
            if frames[k].score[me] <= frames[k - 1].score[me] {
                continue;
            }
            let goal = frames[k].tick;
            let recent = span(frames, goal.saturating_sub(SCORE_WINDOW_TICKS), goal);
            let Some(last) = recent.rev().find(|&j| starts[j][0] || starts[j][1]) else {
                continue;
            };
            if starts[last][them] {
                continue;
            }
            let touched = frames[last].tick;
            let credited = found.iter().zip(&mut out[first..]).find(|(run, a)| {
                a.contact == Some(touched) && run.start < touched && touched <= run.until
            });
            if let Some((_, a)) = credited {
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
