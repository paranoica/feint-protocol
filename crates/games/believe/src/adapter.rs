use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventT, GameAdapter,
    GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::RngCore;

use crate::rules::{Move, Moves, Outcome, Params, Table, by_rank};

pub const GAME_VERSION: u32 = 1;

pub struct Believe {
    params: Params,
    moves: Moves,
    table: Table,
}

#[must_use]
pub fn observation_len(params: &Params) -> usize {
    4 * usize::from(params.ranks) + 6
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

fn refusal(why: &'static str) -> AdapterError {
    if why == "the match is over" {
        AdapterError::MatchOver
    } else {
        AdapterError::BadAction(why)
    }
}

impl Believe {
    pub fn new(params: Params) -> Result<Self, &'static str> {
        let table = Table::empty(&params);
        Self::from_table(params, table)
    }

    pub fn from_table(params: Params, table: Table) -> Result<Self, &'static str> {
        params.validate()?;
        Ok(Self {
            moves: Moves::new(&params),
            params,
            table,
        })
    }

    #[must_use]
    pub fn params(&self) -> &Params {
        &self.params
    }

    #[must_use]
    pub fn moves(&self) -> &Moves {
        &self.moves
    }

    fn leader(table: &Table) -> u8 {
        if table.batches.is_multiple_of(2) {
            table.to_act
        } else {
            1 - table.to_act
        }
    }

    fn take_lowest(&self, from: &mut u64, rank: usize, n: u8) -> u64 {
        let mut mask = 0u64;
        let mut left = n;
        for copy in 0..u32::from(self.params.copies) {
            if left == 0 {
                break;
            }
            let bit = 1u64 << (rank as u32 * u32::from(self.params.copies) + copy);
            if *from & bit != 0 {
                *from &= !bit;
                mask |= bit;
                left -= 1;
            }
        }
        mask
    }

    fn take_counts(&self, from: &mut u64, counts: &[f32]) -> u64 {
        let mut mask = 0u64;
        for (rank, &n) in counts.iter().enumerate() {
            mask |= self.take_lowest(from, rank, n as u8);
        }
        mask
    }

    fn take_random(from: &mut u64, n: u32, rng: &mut ChaCha8Rng) -> u64 {
        let mut mask = 0u64;
        for _ in 0..n {
            let size = from.count_ones();
            if size == 0 {
                break;
            }
            let mut pick = rng.next_u32() % size;
            let mut rest = *from;
            while pick > 0 {
                rest &= rest - 1;
                pick -= 1;
            }
            let bit = 1u64 << rest.trailing_zeros();
            *from &= !bit;
            mask |= bit;
        }
        mask
    }
}

impl GameAdapter for Believe {
    type State = Table;

    fn spec(&self) -> GameSpecT {
        let p = &self.params;
        let ranks = f32::from(p.ranks);
        let deck = p.deck() as f32;
        let copies = f32::from(p.copies);
        let mut spec = GameSpecT::default();
        spec.game_id = Some("believe".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::TurnBased;
        spec.tick_hz = p.tick_hz;
        spec.decision_hz = p.tick_hz;
        let mut space = ActionSpaceT::default();
        space.discrete = self.moves.len() as u32;
        spec.action_space = Some(Box::new(space));
        let mut observation = Vec::new();
        for (group, unit) in [
            ("hand", "cards"),
            ("known", "cards"),
            ("laid", "cards"),
            ("claim", "flag"),
        ] {
            for rank in 0..p.ranks {
                let max = if unit == "flag" { 1.0 } else { copies };
                observation.push(dim(&format!("{group}_{rank}"), 0.0, max, unit));
            }
        }
        observation.extend([
            dim("pile", 0.0, deck, "cards"),
            dim("last_batch", 0.0, f32::from(p.max_batch), "cards"),
            dim("opponent_hand", 0.0, deck, "cards"),
            dim("leader", 0.0, 1.0, "flag"),
            dim("batches", 0.0, p.cap_actions as f32, "count"),
            dim("actions_left", 0.0, p.cap_actions as f32, "count"),
        ]);
        spec.observation = Some(observation);
        spec.signals = Some(vec![
            dim("call", 0.0, 1.0, "flag"),
            dim("claim", -1.0, ranks - 1.0, "rank"),
            dim("count", 0.0, f32::from(p.max_batch), "cards"),
        ]);
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        if bodies.len() != 2 {
            return Err(AdapterError::BadBody("one entry per player"));
        }
        self.table = Table::deal(&self.params, seed);
        Ok(())
    }

    fn live(&self) -> &Table {
        &self.table
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.tick != self.table.tick {
            return Err(AdapterError::WrongTick {
                expected: self.table.tick,
                got: action.tick,
            });
        }
        if action.player > 1 {
            return Err(AdapterError::BadPlayer(action.player));
        }
        if self.table.outcome.is_some() {
            return Err(AdapterError::MatchOver);
        }
        let m = self
            .moves
            .decode(action.discrete)
            .ok_or(AdapterError::BadAction("no such action"))?;
        self.table
            .play(&self.params, action.player, &m)
            .map_err(refusal)
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {
        self.table.tick += 1;
    }

    fn observe(&self, state: &Table, player: PlayerId, out: &mut ObservationT) {
        let p = &self.params;
        let ranks = usize::from(p.ranks);
        let me = usize::from(player);
        let other = 1 - me;
        out.tick = state.tick;
        out.player = player;
        out.to_act = if state.outcome.is_some() {
            -1
        } else {
            state.to_act as i8
        };
        let obs = out.state.get_or_insert_with(Vec::new);
        obs.clear();
        let hand = by_rank(p, state.hands[me]);
        let known = state.known(p, player);
        let laid = by_rank(p, state.laid[me]);
        obs.extend(hand[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend(known[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend(laid[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend((0..ranks).map(|r| f32::from(u8::from(state.claim == Some(r as u8)))));
        obs.push((state.laid[0] | state.laid[1]).count_ones() as f32);
        obs.push(state.last.count_ones() as f32);
        obs.push(state.hands[other].count_ones() as f32);
        obs.push(f32::from(u8::from(Self::leader(state) == player)));
        obs.push(f32::from(state.batches));
        obs.push(p.cap_actions.saturating_sub(state.actions) as f32);
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        if state.batches > 0 && usize::from(state.last_by) == other {
            signals.extend([
                0.0,
                state.claim.map_or(-1.0, f32::from),
                state.last.count_ones() as f32,
            ]);
        }
        let legal = out.legal.get_or_insert_with(Vec::new);
        *legal = state.legal(p, &self.moves, player);
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(state.score());
    }

    fn state_from_observation(&self, obs: &ObservationT, rng: &mut ChaCha8Rng) -> Table {
        let p = &self.params;
        let ranks = usize::from(p.ranks);
        let seen = obs.state.as_deref().unwrap_or_default();
        let field = |group: usize| seen.get(group * ranks..(group + 1) * ranks).unwrap_or(&[]);
        let scalar = |i: usize| seen.get(4 * ranks + i).copied().unwrap_or(0.0);
        let me = usize::from(obs.player.min(1));
        let other = 1 - me;
        let mut free = p.deck_mask();
        let mut table = Table::empty(p);
        table.hands[me] = self.take_counts(&mut free, field(0));
        let known = self.take_counts(&mut free, field(1));
        table.laid[me] = self.take_counts(&mut free, field(2));
        table.claim = field(3).iter().position(|&f| f > 0.5).map(|r| r as u8);
        let pile = scalar(0) as u32;
        let last_count = scalar(1) as u32;
        let their_hand = scalar(2) as u32;
        let leads = scalar(3) > 0.5;
        table.batches = scalar(4) as u16;
        table.actions = p.cap_actions.saturating_sub(scalar(5) as u32);
        let their_laid = pile.saturating_sub(table.laid[me].count_ones());
        let possession = their_hand.saturating_add(their_laid);
        let mut theirs = known
            | Self::take_random(
                &mut free,
                possession.saturating_sub(known.count_ones()),
                rng,
            );
        table.laid[other] = Self::take_random(&mut theirs, their_laid, rng);
        table.hands[other] = theirs;
        table.aside = free;
        table.to_act = match obs.to_act {
            0 => 0,
            1 => 1,
            _ => me as u8,
        };
        let me8 = me as u8;
        let leader = if leads { me8 } else { 1 - me8 };
        if table.batches > 0 {
            table.last_by = if table.batches % 2 == 1 {
                leader
            } else {
                1 - leader
            };
            let mut from = table.laid[usize::from(table.last_by)];
            table.last = Self::take_random(&mut from, last_count, rng);
        }
        if table.batches > 0 && table.hands[usize::from(table.last_by)] == 0 {
            table.out = Some(table.last_by);
        }
        table.seen[me] = table.hands[me] | table.laid[me] | known;
        table.seen[other] = table.hands[other] | table.laid[other];
        let score = obs.score.as_deref().unwrap_or_default();
        if obs.to_act < 0 {
            table.outcome = Some(match score {
                [1, 0] => Outcome::Winner(0),
                [0, 1] => Outcome::Winner(1),
                _ => Outcome::Draw,
            });
        }
        table.tick = obs.tick;
        table
    }

    fn simulate(&self, state: &Table, actions: &[ActionT], ticks: u32) -> Table {
        let mut next = state.clone();
        for action in actions {
            let Some(m) = self.moves.decode(action.discrete) else {
                break;
            };
            if next.play(&self.params, action.player, &m).is_err() {
                break;
            }
        }
        next.tick += u64::from(ticks);
        next
    }

    fn signals(&self, _state: &Table, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        match self.moves.decode(action.discrete) {
            Some(Move::Lay { claim, counts }) => {
                let total: u32 = counts.iter().map(|&c| u32::from(c)).sum();
                out.extend([0.0, f32::from(claim), total as f32]);
            }
            _ => out.extend([1.0, -1.0, 0.0]),
        }
    }

    fn digest(&self, state: &Table) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        let outcome = match state.outcome {
            None => 0u64,
            Some(Outcome::Winner(w)) => 1 + u64::from(w),
            Some(Outcome::Draw) => 3,
        };
        let words = [
            state.hands[0],
            state.hands[1],
            state.laid[0],
            state.laid[1],
            state.aside,
            state.seen[0],
            state.seen[1],
            state.last,
            u64::from(state.last_by),
            state.claim.map_or(u64::MAX, u64::from),
            u64::from(state.to_act),
            u64::from(state.batches),
            u64::from(state.actions),
            state.out.map_or(u64::MAX, u64::from),
            outcome,
            state.tick,
        ];
        for byte in words.iter().flat_map(|w| w.to_le_bytes()) {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    fn result(&self) -> Option<MatchResultT> {
        self.table.outcome.map(|_| {
            let mut result = MatchResultT::default();
            result.score = Some(self.table.score().to_vec());
            result.duration_ticks = self.table.tick;
            result
        })
    }
}
