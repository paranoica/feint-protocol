use crate::rules::{Counts, MAX_RANKS, Params};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Call,
    Lay { claim: u8, counts: Counts },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moves {
    ranks: u8,
    sets: Vec<Counts>,
}

fn multisets(ranks: usize, size: u8, from: usize, current: &mut Counts, out: &mut Vec<Counts>) {
    if size == 0 {
        out.push(*current);
        return;
    }
    for rank in from..ranks {
        current[rank] += 1;
        multisets(ranks, size - 1, rank, current, out);
        current[rank] -= 1;
    }
}

impl Moves {
    #[must_use]
    pub fn new(params: &Params) -> Self {
        let mut sets = Vec::new();
        for size in 1..=params.max_batch {
            multisets(
                usize::from(params.ranks),
                size,
                0,
                &mut [0; MAX_RANKS],
                &mut sets,
            );
        }
        sets.retain(|set| set.iter().all(|&c| c <= params.copies));
        Self {
            ranks: params.ranks,
            sets,
        }
    }

    #[must_use]
    pub fn per_claim(&self) -> usize {
        self.sets.len()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        1 + usize::from(self.ranks) * self.sets.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    #[must_use]
    pub fn decode(&self, index: i32) -> Option<Move> {
        let index = usize::try_from(index).ok()?;
        if index == 0 {
            return Some(Move::Call);
        }
        if index >= self.len() {
            return None;
        }
        let lay = index - 1;
        Some(Move::Lay {
            claim: (lay / self.sets.len()) as u8,
            counts: self.sets[lay % self.sets.len()],
        })
    }

    #[must_use]
    pub fn index(&self, m: &Move) -> Option<i32> {
        match m {
            Move::Call => Some(0),
            Move::Lay { claim, counts } => {
                if *claim >= self.ranks {
                    return None;
                }
                let set = self.sets.iter().position(|s| s == counts)?;
                i32::try_from(1 + usize::from(*claim) * self.sets.len() + set).ok()
            }
        }
    }
}
