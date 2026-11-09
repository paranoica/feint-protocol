use crate::mallet::Side;
use crate::params::{Body, Params};
use crate::puck::Puck;
use crate::vec2::Vec2;
use crate::world::{SIDES, World, away_from_goal, regions};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub tick: u64,
    pub player: u8,
    pub target: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    WrongTick { expected: u64, got: u64 },
    BadPlayer(u8),
    NotFinite,
    MatchOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchEvent {
    Goal { tick: u64, scorer: Side },
    RoundEnd { tick: u64, to: Side },
    Over { tick: u64 },
}

#[derive(Clone, Debug)]
pub struct Match {
    pub params: Params,
    pub bodies: [Body; 2],
    pub world: World,
    seed: u64,
    tick: u64,
    score: [u32; 2],
    stall_half: Option<Side>,
    stall_ticks: u32,
    pause_ticks: u32,
    over: bool,
}

impl Match {
    #[must_use]
    pub fn new(params: Params, bodies: [Body; 2], seed: u64) -> Self {
        if let Err(why) = params.validate() {
            panic!("invalid Params: {why}");
        }
        for body in &bodies {
            assert!(
                body.max_speed() <= params.max_body_speed
                    && body.max_accel() <= params.max_body_accel,
                "body beyond the declared limits: {body:?}"
            );
        }
        let world = World::new(&params);
        let mut game = Self {
            params,
            bodies,
            world,
            seed,
            tick: 0,
            score: [0, 0],
            stall_half: None,
            stall_ticks: 0,
            pause_ticks: 0,
            over: false,
        };
        game.serve(if seed & 1 == 0 { Side::Low } else { Side::High });
        game
    }

    #[must_use]
    pub fn from_observed(
        params: Params,
        bodies: [Body; 2],
        world: World,
        tick: u64,
        score: [u32; 2],
        stall_ticks: u32,
    ) -> Self {
        let x = world.puck.pos.x;
        let stall_half = (x != 0.0).then_some(if x < 0.0 { Side::Low } else { Side::High });
        let over = score.iter().any(|&goals| goals >= params.goals_to_win);
        Self {
            params,
            bodies,
            world,
            seed: 0,
            tick,
            score,
            stall_half,
            stall_ticks,
            pause_ticks: 0,
            over,
        }
    }

    #[must_use]
    pub fn with_pause(mut self, ticks: u32) -> Self {
        self.pause_ticks = ticks.min(self.params.serve_pause_ticks());
        self
    }

    #[must_use]
    pub fn pause_ticks(&self) -> u32 {
        self.pause_ticks
    }

    #[must_use]
    pub fn stall_ticks(&self) -> u32 {
        self.stall_ticks
    }

    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    #[must_use]
    pub fn score(&self) -> [u32; 2] {
        self.score
    }

    #[must_use]
    pub fn is_over(&self) -> bool {
        self.over
    }

    pub fn apply(&mut self, input: Input) -> Result<(), InputError> {
        if self.over {
            return Err(InputError::MatchOver);
        }
        if input.tick != self.tick {
            return Err(InputError::WrongTick {
                expected: self.tick,
                got: input.tick,
            });
        }
        let [x, y] = input.target;
        let mallet = self
            .world
            .mallets
            .get_mut(usize::from(input.player))
            .ok_or(InputError::BadPlayer(input.player))?;
        if !(x.is_finite() && y.is_finite()) {
            return Err(InputError::NotFinite);
        }
        mallet.target = Vec2::new(f64::from(x), f64::from(y));
        Ok(())
    }

    pub fn step(&mut self, events: &mut Vec<MatchEvent>) {
        if self.over {
            return;
        }
        let tick = self.tick;
        self.tick = self.tick.saturating_add(1);
        if self.pause_ticks > 0 {
            self.pause_ticks -= 1;
            self.world.hold(&self.params, &self.bodies);
            return;
        }
        if let Some(scorer) = self.world.tick(&self.params, &self.bodies) {
            let goals = &mut self.score[scorer.index()];
            *goals += 1;
            events.push(MatchEvent::Goal { tick, scorer });
            if *goals >= self.params.goals_to_win {
                self.over = true;
                events.push(MatchEvent::Over { tick });
            }
            self.serve(scorer.other());
            if !self.over {
                self.pause_ticks = self.params.serve_pause_ticks();
            }
            return;
        }
        let x = self.world.puck.pos.x;
        let half = (x != 0.0).then_some(if x < 0.0 { Side::Low } else { Side::High });
        if half.is_some() && half == self.stall_half {
            self.stall_ticks += 1;
        } else {
            self.stall_half = half;
            self.stall_ticks = u32::from(half.is_some());
        }
        let limit = (self.params.stall_seconds * f64::from(self.params.tick_hz)).round() as u32;
        if let Some(stalled) = half
            && self.stall_ticks >= limit
        {
            events.push(MatchEvent::RoundEnd {
                tick,
                to: stalled.other(),
            });
            self.serve(stalled.other());
            self.pause_ticks = self.params.serve_pause_ticks();
        }
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        let w = &self.world;
        let mut words = [0u64; 23];
        let mut i = 0;
        let mut put = |word: u64| {
            words[i] = word;
            i += 1;
        };
        put(self.tick);
        put(u64::from(self.score[0]));
        put(u64::from(self.score[1]));
        put(match self.stall_half {
            None => 0,
            Some(Side::Low) => 1,
            Some(Side::High) => 2,
        });
        put(u64::from(self.stall_ticks));
        put(u64::from(self.pause_ticks));
        put(u64::from(self.over));
        for v in [w.puck.pos, w.puck.vel] {
            put(v.x.to_bits());
            put(v.y.to_bits());
        }
        for mallet in &w.mallets {
            for v in [mallet.pos, mallet.vel, mallet.target] {
                put(v.x.to_bits());
                put(v.y.to_bits());
            }
        }
        fnv1a(&words)
    }

    fn serve(&mut self, to: Side) {
        let x = self.params.half_length * 0.5;
        let spot = match to {
            Side::Low => Vec2::new(-x, 0.0),
            Side::High => Vec2::new(x, 0.0),
        };
        self.world.puck = Puck::at_rest(spot);
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let regions = regions(&self.params);
        for ((mallet, region), side) in self.world.mallets.iter_mut().zip(&regions).zip(SIDES) {
            mallet.push_off(spot, reach, -away_from_goal(side), region);
        }
        self.stall_half = Some(to);
        self.stall_ticks = 0;
    }
}

fn fnv1a(words: &[u64]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for word in words {
        for byte in word.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}
