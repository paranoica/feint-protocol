use std::collections::VecDeque;

use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Controller, Playbook};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::session::Pointer;

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
}
