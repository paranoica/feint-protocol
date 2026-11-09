use std::collections::VecDeque;

use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Controller, Playbook};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::session::Pointer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: &'static str,
    pub optional: bool,
}

const fn kept(name: &'static str) -> Column {
    Column {
        name,
        optional: false,
    }
}

const fn optional(name: &'static str) -> Column {
    Column {
        name,
        optional: true,
    }
}

pub trait HumanInput {
    fn set(&mut self, values: &[f32]);
    fn push(&mut self, values: &[f32]);
}

pub trait HubGame: GameAdapter + Sized + 'static {
    type Book: Playbook<Self> + 'static;
    type Human: Controller<Self> + HumanInput + Default + 'static;
    const ID: &'static str;
    const VERSION: u32;
    const ARENA: bool;
    const COLUMNS: &'static [Column];

    fn build(level: u8) -> Self;
    fn book() -> Self::Book;
    fn human_body() -> BodyParamsT;
    fn score(&self) -> [u32; 2];
    fn tick(&self) -> u64;
    fn tick_hz(&self) -> u32;
    fn pause(&self) -> f32;
    fn view(&self) -> Vec<f32>;
    fn layout(&self) -> Vec<f32>;
    fn class_point(book: &Self::Book, attacker: u8, class: u8) -> [f32; 2];
    fn frame(&self) -> Vec<Option<f64>>;
}

#[must_use]
pub fn body(values: [f64; 2]) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(values.iter().map(|&v| v as f32).collect());
    b
}

#[derive(Default)]
pub struct Pad {
    target: Option<Vec<f32>>,
    sent: Option<Vec<f32>>,
    queue: VecDeque<Vec<f32>>,
    me: PlayerId,
}

impl HumanInput for Pad {
    fn set(&mut self, values: &[f32]) {
        if values.iter().all(|v| v.is_finite()) {
            self.target = Some(values.to_vec());
        }
    }

    fn push(&mut self, values: &[f32]) {
        if values.iter().all(|v| v.is_finite()) {
            self.queue.push_back(values.to_vec());
        }
    }
}

impl<A: GameAdapter> Controller<A> for Pad {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.sent = None;
        self.queue.clear();
    }

    fn on_tick(&mut self, _game: &A, obs: &ObservationT) -> Option<ActionT> {
        let values = if let Some(once) = self.queue.pop_front() {
            self.sent = None;
            once
        } else {
            let target = self.target.clone()?;
            if self.sent.as_ref() == Some(&target) {
                return None;
            }
            self.sent = Some(target.clone());
            target
        };
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.values = Some(values);
        Some(action)
    }
}

impl HumanInput for Pointer {
    fn set(&mut self, values: &[f32]) {
        if let [x, y, ..] = values {
            self.aim(*x, *y);
        }
    }

    fn push(&mut self, values: &[f32]) {
        self.set(values);
    }
}

impl HubGame for AirHockey {
    type Book = HockeyPlaybook;
    type Human = Pointer;
    const ID: &'static str = "airhockey";
    const VERSION: u32 = GAME_VERSION;
    const ARENA: bool = true;
    const COLUMNS: &'static [Column] = &[
        kept("puck_x"),
        kept("puck_y"),
        kept("puck_vx"),
        kept("puck_vy"),
        kept("m0_x"),
        kept("m0_y"),
        kept("m0_vx"),
        kept("m0_vy"),
        kept("m1_x"),
        kept("m1_y"),
        kept("m1_vx"),
        kept("m1_vy"),
    ];

    fn build(level: u8) -> Self {
        AirHockey::new(Params::default(), level)
    }

    fn book() -> Self::Book {
        HockeyPlaybook::new(Params::default())
    }

    fn human_body() -> BodyParamsT {
        let p = Params::default();
        body([p.max_body_speed, p.max_body_accel])
    }

    fn score(&self) -> [u32; 2] {
        self.live().score()
    }

    fn tick(&self) -> u64 {
        self.live().tick()
    }

    fn tick_hz(&self) -> u32 {
        self.live().params.tick_hz
    }

    fn pause(&self) -> f32 {
        let live = self.live();
        (f64::from(live.pause_ticks()) / f64::from(live.params.tick_hz)) as f32
    }

    fn view(&self) -> Vec<f32> {
        let world = &self.live().world;
        let [a, b] = world.mallets;
        [
            world.puck.pos.x,
            world.puck.pos.y,
            world.puck.vel.x,
            world.puck.vel.y,
            a.pos.x,
            a.pos.y,
            a.vel.x,
            a.vel.y,
            b.pos.x,
            b.pos.y,
            b.vel.x,
            b.vel.y,
        ]
        .map(|v| v as f32)
        .to_vec()
    }

    fn layout(&self) -> Vec<f32> {
        let p = &self.live().params;
        [
            p.half_length,
            p.half_width,
            p.mouth_half_width,
            p.puck_radius,
            p.mallet_radius,
        ]
        .map(|v| v as f32)
        .to_vec()
    }

    fn class_point(book: &Self::Book, attacker: u8, class: u8) -> [f32; 2] {
        let p = book.class_point(attacker, class);
        [p.x as f32, p.y as f32]
    }

    fn frame(&self) -> Vec<Option<f64>> {
        let world = &self.live().world;
        let [a, b] = world.mallets;
        [
            (world.puck.pos, world.puck.vel),
            (a.pos, a.vel),
            (b.pos, b.vel),
        ]
        .iter()
        .flat_map(|(p, v)| [p.x, p.y, v.x, v.y])
        .map(Some)
        .collect()
    }
}

impl HubGame for penalty::Penalty {
    type Book = penalty::PenaltyPlaybook;
    type Human = Pad;
    const ID: &'static str = "penalty";
    const VERSION: u32 = penalty::GAME_VERSION;
    const ARENA: bool = true;
    const COLUMNS: &'static [Column] = &[
        kept("shooter"),
        kept("phase"),
        kept("t"),
        kept("kicks"),
        kept("dist"),
        kept("running"),
        kept("lean"),
        kept("lean_target"),
        optional("aim"),
        kept("ball_y"),
        kept("keeper_y"),
        kept("keeper_target"),
        optional("dive_target"),
        kept("dive_left"),
        kept("dived"),
        kept("score0"),
        kept("score1"),
    ];

    fn build(level: u8) -> Self {
        penalty::Penalty::new(penalty::Params::default(), level)
    }

    fn book() -> Self::Book {
        penalty::PenaltyPlaybook::new(penalty::Params::default())
    }

    fn human_body() -> BodyParamsT {
        let p = penalty::Params::default();
        body([p.max_dive_speed, p.max_lean_rate])
    }

    fn score(&self) -> [u32; 2] {
        self.live().score
    }

    fn tick(&self) -> u64 {
        self.live().tick
    }

    fn tick_hz(&self) -> u32 {
        self.live().params.tick_hz
    }

    fn pause(&self) -> f32 {
        let s = self.live();
        if s.phase == penalty::Phase::Settle {
            let left = s.params.settle_ticks.saturating_sub(s.t);
            (f64::from(left) / f64::from(s.params.tick_hz)) as f32
        } else {
            0.0
        }
    }

    fn view(&self) -> Vec<f32> {
        let s = self.live();
        let flight = if s.phase == penalty::Phase::Flight {
            f64::from(s.t) / f64::from(s.params.flight_ticks)
        } else if s.phase == penalty::Phase::Settle {
            1.0
        } else {
            0.0
        };
        let dive = s.dive.map_or(s.keeper_y, |d| d.target);
        let diving = s.dive.is_some_and(|d| d.left > 0);
        [
            f64::from(s.shooter()),
            s.phase.code(),
            s.dist,
            s.lean,
            s.ball_y,
            flight,
            s.keeper_y,
            dive,
            f64::from(u8::from(diving)),
            f64::from(s.kicks),
            s.last_goal.map_or(-1.0, |g| f64::from(u8::from(g))),
            if matches!(s.phase, penalty::Phase::Flight | penalty::Phase::Settle) {
                s.aim.unwrap_or(0.0)
            } else {
                0.0
            },
        ]
        .map(|v| v as f32)
        .to_vec()
    }

    fn layout(&self) -> Vec<f32> {
        let p = &self.live().params;
        [p.half_goal, p.keeper_reach, p.run_up, p.wide, p.kick_range]
            .map(|v| v as f32)
            .to_vec()
    }

    fn class_point(book: &Self::Book, _attacker: u8, class: u8) -> [f32; 2] {
        let [x, y] = book.class_point(class);
        [x as f32, y as f32]
    }

    fn frame(&self) -> Vec<Option<f64>> {
        let s = self.live();
        let flag = |b: bool| Some(f64::from(u8::from(b)));
        vec![
            Some(f64::from(s.shooter())),
            Some(s.phase.code()),
            Some(f64::from(s.t)),
            Some(f64::from(s.kicks)),
            Some(s.dist),
            flag(s.running),
            Some(s.lean),
            Some(s.lean_target),
            s.aim,
            Some(s.ball_y),
            Some(s.keeper_y),
            Some(s.keeper_target),
            s.dive.map(|d| d.target),
            Some(s.dive.map_or(0.0, |d| f64::from(d.left))),
            flag(s.dived),
            Some(f64::from(s.score[0])),
            Some(f64::from(s.score[1])),
        ]
    }
}

impl HubGame for duel::QuickDraw {
    type Book = duel::DuelPlaybook;
    type Human = Pad;
    const ID: &'static str = "duel";
    const VERSION: u32 = duel::GAME_VERSION;
    const ARENA: bool = true;
    const COLUMNS: &'static [Column] = &[
        kept("phase"),
        kept("t"),
        kept("round"),
        kept("x0"),
        kept("hand0"),
        kept("committed0"),
        kept("loaded0"),
        optional("aim0"),
        kept("recovery0"),
        kept("x1"),
        kept("hand1"),
        kept("committed1"),
        kept("loaded1"),
        optional("aim1"),
        kept("recovery1"),
        optional("bullet0_x"),
        optional("bullet1_x"),
        kept("score0"),
        kept("score1"),
    ];

    fn build(level: u8) -> Self {
        duel::QuickDraw::new(duel::Params::default(), level)
    }

    fn book() -> Self::Book {
        duel::DuelPlaybook::new(duel::Params::default())
    }

    fn human_body() -> BodyParamsT {
        let p = duel::Params::default();
        body([p.max_draw_rate, p.max_step_rate])
    }

    fn score(&self) -> [u32; 2] {
        self.live().score
    }

    fn tick(&self) -> u64 {
        self.live().tick
    }

    fn tick_hz(&self) -> u32 {
        self.live().params.tick_hz
    }

    fn pause(&self) -> f32 {
        let s = self.live();
        let left = match s.phase {
            duel::Phase::Ready => s.params.ready_ticks.saturating_sub(s.t),
            duel::Phase::Settle => s.params.settle_ticks.saturating_sub(s.t),
            _ => 0,
        };
        (f64::from(left) / f64::from(s.params.tick_hz)) as f32
    }

    fn view(&self) -> Vec<f32> {
        let s = self.live();
        let hz = f64::from(s.params.tick_hz);
        let flag = |b: bool| f64::from(u8::from(b));
        let mut v = vec![s.phase.code(), f64::from(s.t) / hz];
        for g in &s.gunners {
            v.extend([
                g.x,
                g.hand,
                flag(g.committed),
                flag(g.loaded),
                f64::from(g.recovery) / hz,
            ]);
        }
        for b in &s.bullets {
            v.extend(b.map_or([0.0, 0.0, 0.0], |b| {
                [1.0, b.x, f64::from(b.t) / f64::from(s.params.flight_ticks)]
            }));
        }
        let last = match s.last {
            Some(duel::RoundEvent::Hit { shooter }) => f64::from(shooter),
            Some(duel::RoundEvent::Void) => 2.0,
            _ => -1.0,
        };
        v.extend([last, f64::from(s.round)]);
        v.into_iter().map(|x| x as f32).collect()
    }

    fn layout(&self) -> Vec<f32> {
        let p = &self.live().params;
        [
            p.half_width,
            p.hit_radius,
            p.roll_dist,
            p.commit_at,
            f64::from(p.flight_ticks) / f64::from(p.tick_hz),
        ]
        .map(|v| v as f32)
        .to_vec()
    }

    fn class_point(book: &Self::Book, _attacker: u8, class: u8) -> [f32; 2] {
        let [x, y] = book.class_point(class);
        [x as f32, y as f32]
    }

    fn frame(&self) -> Vec<Option<f64>> {
        let s = self.live();
        let flag = |b: bool| Some(f64::from(u8::from(b)));
        let mut v = vec![
            Some(s.phase.code()),
            Some(f64::from(s.t)),
            Some(f64::from(s.round)),
        ];
        for g in &s.gunners {
            v.extend([
                Some(g.x),
                Some(g.hand),
                flag(g.committed),
                flag(g.loaded),
                g.aim,
                Some(f64::from(g.recovery)),
            ]);
        }
        v.extend(s.bullets.iter().map(|b| b.map(|b| b.x)));
        v.extend([Some(f64::from(s.score[0])), Some(f64::from(s.score[1]))]);
        v
    }
}
