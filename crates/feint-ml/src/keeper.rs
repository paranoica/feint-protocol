use std::collections::VecDeque;
use std::ops::AddAssign;

use airhockey::{AirHockey, HockeyPlaybook, Params, Vec2};
use feint_ai::{Controller, Playbook, Stance};
use feint_protocol::{
    ActionT, ChaCha8Rng, EventKind, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::show::{SHOW_HOLD_TICKS, seen, shown_side};

pub const KEEPER_DELAY_TICKS: usize = 24;
pub const BITE_TICKS: u64 = 120;
pub const BITE_GOAL_TICKS: u64 = BITE_TICKS + 48;
const COMING: f64 = 0.3;
const SLOW: f64 = 1.0;
const MAX_BOUNCES: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeeperCounts {
    pub reads: u64,
    pub bites: u64,
    pub bite_goals: u64,
    pub goals: u64,
}

impl AddAssign for KeeperCounts {
    fn add_assign(&mut self, other: Self) {
        self.reads += other.reads;
        self.bites += other.bites;
        self.bite_goals += other.bite_goals;
        self.goals += other.goals;
    }
}

pub struct Read {
    permille: u16,
    rng: ChaCha8Rng,
    run: Option<(u8, usize)>,
    drawn: bool,
}

impl Read {
    #[must_use]
    pub fn new(permille: u16, rng: ChaCha8Rng) -> Self {
        Self {
            permille,
            rng,
            run: None,
            drawn: false,
        }
    }

    pub fn track(&mut self, side: Option<u8>) {
        match (side, self.run) {
            (Some(s), Some((r, n))) if s == r => self.run = Some((r, n + 1)),
            (Some(s), _) => {
                self.run = Some((s, 1));
                self.drawn = false;
            }
            (None, _) => self.forget(),
        }
    }

    pub fn see(&mut self, side: Option<u8>) -> Option<(u8, bool)> {
        self.track(side);
        let (side, held) = self.run?;
        if held < SHOW_HOLD_TICKS || self.drawn {
            return None;
        }
        self.drawn = true;
        Some((side, self.rng.next_u64() % 1000 < u64::from(self.permille)))
    }

    pub fn forget(&mut self) {
        self.run = None;
        self.drawn = false;
    }
}

pub struct Keeper {
    params: Params,
    playbook: HockeyPlaybook,
    permille: u16,
    seen: VecDeque<ObservationT>,
    view: ChaCha8Rng,
    read: Read,
    me: PlayerId,
    bite: Option<(u8, u64)>,
    bitten_at: Option<u64>,
    bite_ticks: Vec<u64>,
    counts: KeeperCounts,
}

impl Keeper {
    #[must_use]
    pub fn new(params: Params, bite_permille: u16) -> Self {
        Self {
            playbook: HockeyPlaybook::new(params.clone()),
            params,
            permille: bite_permille,
            seen: VecDeque::new(),
            view: ChaCha8Rng::seed_from_u64(0),
            read: Read::new(bite_permille, ChaCha8Rng::seed_from_u64(0)),
            me: 0,
            bite: None,
            bitten_at: None,
            bite_ticks: Vec::new(),
            counts: KeeperCounts::default(),
        }
    }

    #[must_use]
    pub fn counts(&self) -> KeeperCounts {
        self.counts
    }

    #[must_use]
    pub fn bite_ticks(&self) -> &[u64] {
        &self.bite_ticks
    }

    fn crossing(&self, from: Vec2, vel: Vec2) -> f64 {
        let p = &self.params;
        let limit = p.half_width - p.puck_radius;
        let mut left = (p.half_length - from.x) / vel.x;
        let (mut y, mut vy) = (from.y, vel.y);
        for _ in 0..MAX_BOUNCES {
            let wall = if vy > 0.0 { limit } else { -limit };
            if vy == 0.0 || (wall - y) / vy >= left {
                return y + vy * left;
            }
            left -= (wall - y) / vy;
            y = wall;
            vy = -vy * p.rail_restitution;
        }
        y
    }

    fn act(&self, tick: u64, values: [f64; 2]) -> ActionT {
        let mut action = ActionT::default();
        action.tick = tick;
        action.player = self.me;
        action.values = Some(vec![values[0] as f32, values[1] as f32]);
        action
    }
}

impl Controller<AirHockey> for Keeper {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.view = ChaCha8Rng::seed_from_u64(seed);
        self.view.set_stream(2 * u64::from(me) + 1);
        let mut bites = ChaCha8Rng::seed_from_u64(seed);
        bites.set_stream(2 * u64::from(me) + 2);
        self.read = Read::new(self.permille, bites);
        self.bite = None;
        self.bitten_at = None;
        self.bite_ticks.clear();
        self.counts = KeeperCounts::default();
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let now = obs.tick;
        self.seen.push_back(obs.clone());
        while self.seen.len() > KEEPER_DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        if self.seen.len() <= KEEPER_DELAY_TICKS {
            return None;
        }
        let state = game.state_from_observation(self.seen.front()?, &mut self.view);
        let attacker = 1 - self.me;
        let puck = state.world.puck.pos;
        let vel = state.world.puck.vel;
        if self.bite.is_some_and(|(_, until)| now >= until) {
            self.bite = None;
        }
        let shown = shown_side(
            &self.params,
            puck,
            state.world.mallets[usize::from(attacker)].pos,
            attacker,
        );
        if self.bite.is_some() {
            self.read.track(shown);
        } else if let Some((side, bites)) = self.read.see(shown) {
            self.counts.reads += 1;
            if bites {
                self.counts.bites += 1;
                self.bite = Some((side, now + BITE_TICKS));
                self.bitten_at = Some(now);
                self.bite_ticks.push(now);
            }
        }
        let p = &self.params;
        let edge = -(p.half_length - p.mallet_radius);
        let reach = p.mouth_half_width - p.mallet_radius;
        let own_puck = seen(self.me, puck);
        let own_vel = seen(self.me, vel);
        let y = if let Some((side, _)) = self.bite {
            match side {
                2 => -reach,
                0 => reach,
                _ => 0.0,
            }
        } else if own_puck.x < 0.0 && vel.length() < SLOW {
            let mut action = self
                .playbook
                .opponent_move(&state, self.me, Stance::Shoot(1));
            action.tick = now;
            action.player = self.me;
            return Some(action);
        } else if own_vel.x < -COMING {
            (-self.crossing(-own_puck, -own_vel)).clamp(-reach, reach)
        } else {
            0.0
        };
        Some(self.act(now, [edge, y]))
    }

    fn on_event(&mut self, event: &EventT) {
        if event.kind != EventKind::Score {
            return;
        }
        if event.player == i8::try_from(1 - self.me).unwrap_or(-1) {
            self.counts.goals += 1;
            if self
                .bitten_at
                .is_some_and(|at| event.tick <= at + BITE_GOAL_TICKS)
            {
                self.counts.bite_goals += 1;
            }
        }
        self.bite = None;
        self.read.forget();
    }
}
