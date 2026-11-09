use crate::mallet::Side;
use crate::params::{Body, Params};
use crate::puck::Puck;
use crate::vec2::Vec2;
use crate::world::{SIDES, World, away_from_goal, regions};

/// One player's mallet target for one tick, in the world frame, exactly as recorded
/// (FNT-D6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub tick: u64,
    pub player: u8,
    pub target: [f32; 2],
}

/// Why an input was refused; a refused input changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    WrongTick { expected: u64, got: u64 },
    BadPlayer(u8),
    NotFinite,
    MatchOver,
}

/// What happened on a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchEvent {
    Goal {
        tick: u64,
        scorer: Side,
    },
    /// The puck stayed in one half too long and goes to the other player.
    RoundEnd {
        tick: u64,
        to: Side,
    },
    Over {
        tick: u64,
    },
}

/// A match: the world plus score, serve and stall rules (FNT-D5).
#[derive(Clone, Debug)]
pub struct Match {
    pub params: Params,
    pub bodies: [Body; 2],
    pub world: World,
    seed: u64,
    tick: u64,
    score: [u32; 2],
    /// The half the puck has been in since `stall_ticks` started counting.
    stall_half: Option<Side>,
    stall_ticks: u32,
    over: bool,
}

impl Match {
    /// The first serve goes to the side picked by the seed's lowest bit.
    ///
    /// # Panics
    /// When `params` fail [`Params::validate`] or a body is beyond their declared limits:
    /// both are configuration, not input (untrusted bodies go through [`Params::body`]).
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
            over: false,
        };
        game.serve(if seed & 1 == 0 { Side::Low } else { Side::High });
        game
    }

    /// A match rebuilt from what a player observed; the stall half follows the puck.
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
            over,
        }
    }

    /// Ticks the puck has spent in its current half.
    #[must_use]
    pub fn stall_ticks(&self) -> u32 {
        self.stall_ticks
    }

    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The tick the next `step` will run.
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

    /// Set a player's mallet target for the coming tick; it holds until replaced.
    ///
    /// # Errors
    /// Another tick, an unknown player, a non-finite target or a finished match.
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

    /// Run one tick, appending what happened. Does nothing once the match is over.
    pub fn step(&mut self, events: &mut Vec<MatchEvent>) {
        if self.over {
            return;
        }
        let tick = self.tick;
        self.tick = self.tick.saturating_add(1);
        if let Some(scorer) = self.world.tick(&self.params, &self.bodies) {
            let goals = &mut self.score[scorer.index()];
            *goals += 1;
            events.push(MatchEvent::Goal { tick, scorer });
            if *goals >= self.params.goals_to_win {
                self.over = true;
                events.push(MatchEvent::Over { tick });
            }
            self.serve(scorer.other());
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
        }
    }

    /// FNV-1a over the exact bits of everything that shapes the rest of the match.
    #[must_use]
    pub fn digest(&self) -> u64 {
        let w = &self.world;
        let mut words = [0u64; 22];
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

    /// Put the puck at rest on `to`'s serve spot; a mallet standing there is pushed off.
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
