use crate::rules::{Counts, Move, Params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fact {
    Lay {
        seat: u8,
        claim: u8,
        counts: Counts,
    },
    Call {
        seat: u8,
        layer: u8,
        claim: u8,
        honest: bool,
        taker: Option<u8>,
        revealed: Counts,
    },
    Out {
        seat: u8,
    },
}

#[derive(Clone, Debug)]
pub struct Ledger {
    cards: [u32; 2],
    pile: u32,
    last: Option<(u8, u8, Counts)>,
}

impl Ledger {
    #[must_use]
    pub fn new(params: &Params) -> Self {
        let hand = u32::from(params.hand);
        Self {
            cards: [hand, hand],
            pile: 0,
            last: None,
        }
    }

    #[must_use]
    pub fn cards(&self) -> [u32; 2] {
        self.cards
    }

    #[must_use]
    pub fn pile(&self) -> u32 {
        self.pile
    }

    pub fn record(&mut self, player: u8, m: &Move) -> Vec<Fact> {
        let me = usize::from(player.min(1));
        let other = 1 - me;
        match m {
            Move::Lay { claim, counts } => {
                if self.cards[other] == 0 {
                    return vec![Fact::Out { seat: other as u8 }];
                }
                let n: u32 = counts.iter().map(|&c| u32::from(c)).sum();
                self.cards[me] = self.cards[me].saturating_sub(n);
                self.pile += n;
                self.last = Some((player, *claim, *counts));
                vec![Fact::Lay {
                    seat: player,
                    claim: *claim,
                    counts: *counts,
                }]
            }
            Move::Call => {
                let Some((layer, claim, revealed)) = self.last.take() else {
                    return Vec::new();
                };
                let honest = revealed
                    .iter()
                    .enumerate()
                    .all(|(rank, &c)| c == 0 || rank == usize::from(claim));
                let call = |taker| Fact::Call {
                    seat: player,
                    layer,
                    claim,
                    honest,
                    taker,
                    revealed,
                };
                if honest && self.cards[usize::from(layer.min(1))] == 0 {
                    return vec![call(None), Fact::Out { seat: layer }];
                }
                let taker = if honest { player } else { layer };
                self.cards[usize::from(taker.min(1))] += self.pile;
                self.pile = 0;
                vec![call(Some(taker))]
            }
        }
    }
}
