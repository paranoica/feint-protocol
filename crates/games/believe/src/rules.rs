use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

pub const MAX_RANKS: usize = 8;
pub const MAX_CARDS: u32 = 64;
pub const MAX_MOVES: u64 = 4096;

pub type Counts = [u8; MAX_RANKS];

pub use crate::moves::{Move, Moves};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub ranks: u8,
    pub copies: u8,
    pub hand: u8,
    pub max_batch: u8,
    pub cap_actions: u32,
    pub tick_hz: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ranks: 5,
            copies: 4,
            hand: 8,
            max_batch: 3,
            cap_actions: 300,
            tick_hz: 10,
        }
    }
}

impl Params {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(2..=MAX_RANKS as u8).contains(&self.ranks) {
            return Err("2 to 8 ranks");
        }
        if self.copies == 0 || u32::from(self.ranks) * u32::from(self.copies) > MAX_CARDS {
            return Err("1 copy or more, 64 cards at most");
        }
        if self.hand == 0 || 2 * u32::from(self.hand) > self.deck() {
            return Err("both hands must fit in the deck");
        }
        if self.max_batch == 0 || self.max_batch > self.copies {
            return Err("a batch holds 1 card up to the copies of a rank");
        }
        if self.move_count() > MAX_MOVES {
            return Err("too many distinct batches for the action space");
        }
        if self.cap_actions < 2 {
            return Err("the cap must allow a lay and an answer");
        }
        if self.tick_hz == 0 {
            return Err("ticks need a rate");
        }
        Ok(())
    }

    #[must_use]
    pub fn deck(&self) -> u32 {
        u32::from(self.ranks) * u32::from(self.copies)
    }

    #[must_use]
    pub fn move_count(&self) -> u64 {
        let ranks = u64::from(self.ranks);
        let mut sets = 0u64;
        let mut choose = 1u64;
        for size in 1..=u64::from(self.max_batch) {
            choose = choose * (ranks + size - 1) / size;
            sets += choose;
        }
        1 + ranks * sets
    }

    #[must_use]
    pub fn deck_mask(&self) -> u64 {
        u64::MAX >> (MAX_CARDS - self.deck().clamp(1, MAX_CARDS))
    }

    #[must_use]
    pub fn rank_of(&self, id: u32) -> usize {
        (id / u32::from(self.copies)) as usize
    }
}

#[must_use]
pub fn by_rank(params: &Params, mask: u64) -> Counts {
    let mut counts = [0u8; MAX_RANKS];
    let mut rest = mask;
    while rest != 0 {
        let id = rest.trailing_zeros();
        counts[params.rank_of(id)] += 1;
        rest &= rest - 1;
    }
    counts
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Winner(u8),
    Draw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub hands: [u64; 2],
    pub laid: [u64; 2],
    pub aside: u64,
    pub known: [Counts; 2],
    pub last: u64,
    pub last_by: u8,
    pub claim: Option<u8>,
    pub to_act: u8,
    pub batches: u16,
    pub actions: u32,
    pub out: Option<u8>,
    pub outcome: Option<Outcome>,
    pub tick: u64,
}

impl Table {
    #[must_use]
    pub fn empty(_params: &Params) -> Self {
        Self {
            hands: [0; 2],
            laid: [0; 2],
            aside: 0,
            known: [[0; MAX_RANKS]; 2],
            last: 0,
            last_by: 0,
            claim: None,
            to_act: 0,
            batches: 0,
            actions: 0,
            out: None,
            outcome: None,
            tick: 0,
        }
    }

    #[must_use]
    pub fn deal(params: &Params, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let deck = params.deck();
        let mut ids: Vec<u32> = (0..deck).collect();
        for i in (1..ids.len()).rev() {
            let j = (rng.next_u32() % (i as u32 + 1)) as usize;
            ids.swap(i, j);
        }
        let hand = usize::from(params.hand);
        let mask = |slice: &[u32]| slice.iter().fold(0u64, |m, &id| m | (1 << id));
        let mut table = Self::empty(params);
        table.hands = [mask(&ids[..hand]), mask(&ids[hand..2 * hand])];
        table.aside = mask(&ids[2 * hand..]);
        table.to_act = (rng.next_u32() % 2) as u8;
        table
    }

    #[must_use]
    pub fn score(&self) -> [i64; 2] {
        match self.outcome {
            Some(Outcome::Winner(0)) => [1, 0],
            Some(Outcome::Winner(_)) => [0, 1],
            _ => [0, 0],
        }
    }

    pub fn validate(&self, params: &Params) -> Result<(), &'static str> {
        let deck = params.deck_mask();
        let sets = [
            self.hands[0],
            self.hands[1],
            self.laid[0],
            self.laid[1],
            self.aside,
        ];
        let mut all = 0u64;
        for set in sets {
            if set & !deck != 0 {
                return Err("a card outside the deck");
            }
            if set & all != 0 {
                return Err("a card in two places");
            }
            all |= set;
        }
        if self.to_act > 1 || self.last_by > 1 || self.out.is_some_and(|p| p > 1) {
            return Err("no such player");
        }
        if self.last & !self.laid[usize::from(self.last_by)] != 0 {
            return Err("the last batch is not in its layer's pile");
        }
        let laid = self.laid[0] | self.laid[1];
        if (self.batches > 0) != (laid != 0) {
            return Err("cards on the table exactly while a round runs");
        }
        if self.batches > 0 && (self.claim.is_none() || self.last == 0) {
            return Err("a running round needs a claim and a last batch");
        }
        if self.actions > params.cap_actions {
            return Err("past the cap");
        }
        if self.claim.is_some_and(|c| c >= params.ranks)
            || matches!(self.outcome, Some(Outcome::Winner(w)) if w > 1)
        {
            return Err("no such rank or winner");
        }
        for viewer in 0..2 {
            let theirs = 1 - viewer;
            let held = by_rank(params, self.hands[theirs] | self.laid[theirs]);
            if self.known[viewer].iter().zip(held).any(|(&k, h)| k > h) {
                return Err("more known cards than the opponent holds");
            }
        }
        Ok(())
    }

    fn take(params: &Params, hand: u64, counts: &Counts) -> Option<u64> {
        let mut mask = 0u64;
        for (rank, &want) in counts.iter().enumerate().take(usize::from(params.ranks)) {
            let mut left = want;
            for copy in 0..params.copies {
                if left == 0 {
                    break;
                }
                let bit = 1u64 << (rank as u32 * u32::from(params.copies) + u32::from(copy));
                if hand & bit != 0 {
                    mask |= bit;
                    left -= 1;
                }
            }
            if left > 0 {
                return None;
            }
        }
        Some(mask)
    }

    fn check(&self, params: &Params, player: u8, m: &Move) -> Result<Option<u64>, &'static str> {
        if self.outcome.is_some() {
            return Err("the match is over");
        }
        if player > 1 {
            return Err("no such player");
        }
        if player != self.to_act {
            return Err("not this player's turn");
        }
        match m {
            Move::Call => {
                if self.batches == 0 {
                    return Err("nothing to call");
                }
                Ok(None)
            }
            Move::Lay { claim, counts } => {
                let ranks = usize::from(params.ranks);
                if *claim >= params.ranks || counts[ranks..].iter().any(|&c| c > 0) {
                    return Err("no such rank");
                }
                let total: u32 = counts.iter().map(|&c| u32::from(c)).sum();
                if total == 0 || total > u32::from(params.max_batch) {
                    return Err("batch size out of range");
                }
                if self.batches > 0 && self.claim != Some(*claim) {
                    return Err("the claim is fixed in a round");
                }
                Self::take(params, self.hands[usize::from(player)], counts)
                    .map(Some)
                    .ok_or("cards not in hand")
            }
        }
    }

    pub fn play(&mut self, params: &Params, player: u8, m: &Move) -> Result<(), &'static str> {
        let cards = self.check(params, player, m)?;
        let me = usize::from(player);
        let other = 1 - player;
        match (m, cards) {
            (Move::Lay { .. }, _) if self.out == Some(other) => {
                self.outcome = Some(Outcome::Winner(other));
            }
            (Move::Lay { claim, .. }, Some(mask)) => {
                self.hands[me] &= !mask;
                self.laid[me] |= mask;
                self.last = mask;
                self.last_by = player;
                self.claim = Some(*claim);
                self.batches = self.batches.saturating_add(1);
                if self.hands[me] == 0 {
                    self.out = Some(player);
                }
                self.to_act = other;
            }
            _ => self.resolve(params, player),
        }
        self.actions = self.actions.saturating_add(1);
        if self.outcome.is_none() && self.actions >= params.cap_actions {
            let held = [self.hands[0].count_ones(), self.hands[1].count_ones()];
            self.outcome = Some(match held[0].cmp(&held[1]) {
                std::cmp::Ordering::Less => Outcome::Winner(0),
                std::cmp::Ordering::Greater => Outcome::Winner(1),
                std::cmp::Ordering::Equal => Outcome::Draw,
            });
        }
        Ok(())
    }

    fn resolve(&mut self, params: &Params, doubter: u8) {
        let claim = self.claim.map_or(0, usize::from);
        let counts = by_rank(params, self.last);
        let honest = counts
            .iter()
            .enumerate()
            .all(|(rank, &c)| c == 0 || rank == claim);
        let layer = self.last_by;
        if honest && self.out == Some(layer) {
            self.outcome = Some(Outcome::Winner(layer));
            return;
        }
        let (taker, right) = if honest {
            (doubter, layer)
        } else {
            (layer, doubter)
        };
        let pile_by = [by_rank(params, self.laid[0]), by_rank(params, self.laid[1])];
        for viewer in 0..2u8 {
            let v = usize::from(viewer);
            let theirs = 1 - v;
            for (rank, known) in self.known[v].iter_mut().enumerate() {
                *known = if taker == viewer {
                    known.saturating_sub(pile_by[theirs][rank])
                } else if layer == viewer {
                    *known + pile_by[v][rank]
                } else {
                    (*known).max(counts[rank]) + pile_by[v][rank]
                };
            }
        }
        let pile = self.laid[0] | self.laid[1];
        let t = usize::from(taker);
        self.hands[t] |= pile;
        self.laid = [0, 0];
        self.last = 0;
        self.claim = None;
        self.batches = 0;
        self.out = None;
        self.to_act = right;
    }

    #[must_use]
    pub fn legal(&self, params: &Params, moves: &Moves, player: u8) -> Vec<u8> {
        let mut out = vec![0u8; moves.len()];
        if self.outcome.is_some() || player != self.to_act {
            return out;
        }
        for (index, slot) in out.iter_mut().enumerate() {
            if let Some(m) = moves.decode(index as i32) {
                *slot = u8::from(self.check(params, player, &m).is_ok());
            }
        }
        out
    }
}
