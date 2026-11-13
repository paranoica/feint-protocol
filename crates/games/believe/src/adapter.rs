use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventT, GameAdapter,
    GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

use crate::rules::{Move, Moves, Outcome, Params, Table, by_rank};

pub const GAME_VERSION: u32 = 1;

pub struct Believe {
    params: Params,
    moves: Moves,
    table: Table,
}

#[must_use]
pub fn observation_len(params: &Params) -> usize {
    5 * usize::from(params.ranks) + 6
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

impl Default for Believe {
    fn default() -> Self {
        let params = Params::default();
        Self {
            moves: Moves::new(&params),
            table: Table::empty(&params),
            params,
        }
    }
}

impl Believe {
    pub fn new(params: Params) -> Result<Self, &'static str> {
        let table = Table::empty(&params);
        Self::from_table(params, table)
    }

    pub fn from_table(params: Params, table: Table) -> Result<Self, &'static str> {
        params.validate()?;
        table.validate(&params)?;
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
            ("mine_last", "cards"),
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
        self.table.tick = self.table.tick.saturating_add(1);
    }

    fn observe(&self, state: &Table, player: PlayerId, out: &mut ObservationT) {
        let p = &self.params;
        let ranks = usize::from(p.ranks);
        out.tick = state.tick;
        out.player = player;
        if player > 1 {
            out.to_act = -1;
            for list in [&mut out.state, &mut out.opponent_signals] {
                list.get_or_insert_with(Vec::new).clear();
            }
            out.legal.get_or_insert_with(Vec::new).clear();
            out.score.get_or_insert_with(Vec::new).clear();
            return;
        }
        let me = usize::from(player);
        let other = 1 - me;
        out.to_act = if state.outcome.is_some() {
            -1
        } else {
            state.to_act as i8
        };
        let obs = out.state.get_or_insert_with(Vec::new);
        obs.clear();
        let hand = by_rank(p, state.hands[me]);
        let known = state.known[me];
        let mine_last = if state.batches > 0 && usize::from(state.last_by) == me {
            by_rank(p, state.last)
        } else {
            [0; crate::rules::MAX_RANKS]
        };
        let laid = by_rank(p, state.laid[me]);
        obs.extend(hand[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend(known[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend(laid[..ranks].iter().map(|&c| f32::from(c)));
        obs.extend((0..ranks).map(|r| f32::from(u8::from(state.claim == Some(r as u8)))));
        obs.extend(mine_last[..ranks].iter().map(|&c| f32::from(c)));
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
        crate::sample::table_from_view(&self.params, obs, rng)
    }

    fn simulate(&self, state: &Table, actions: &[ActionT], ticks: u32) -> Table {
        let mut next = state.clone();
        for action in actions {
            let Some(m) = self.moves.decode(action.discrete) else {
                continue;
            };
            let _ = next.play(&self.params, action.player, &m);
        }
        next.tick = next.tick.saturating_add(u64::from(ticks));
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
            u64::from_le_bytes(state.known[0]),
            u64::from_le_bytes(state.known[1]),
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
