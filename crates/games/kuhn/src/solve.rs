use feint_ai::cfr::{Table, info_key};
use feint_turn::TurnPolicy;

use crate::{BET, PASS};

pub const REACH_FLOOR: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    Opening,
    AfterPass,
    FacingBet,
    FacingAfterPass,
}

const NODES: [Node; 4] = [
    Node::Opening,
    Node::AfterPass,
    Node::FacingBet,
    Node::FacingAfterPass,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Infoset {
    card: u8,
    node: Node,
}

impl Infoset {
    #[must_use]
    pub fn opening(card: u8) -> Self {
        Self {
            card,
            node: Node::Opening,
        }
    }

    #[must_use]
    pub fn after_pass(card: u8) -> Self {
        Self {
            card,
            node: Node::AfterPass,
        }
    }

    #[must_use]
    pub fn facing_bet(card: u8) -> Self {
        Self {
            card,
            node: Node::FacingBet,
        }
    }

    #[must_use]
    pub fn facing_after_pass(card: u8) -> Self {
        Self {
            card,
            node: Node::FacingAfterPass,
        }
    }

    #[must_use]
    pub fn all() -> Vec<Self> {
        NODES
            .iter()
            .flat_map(|&node| (0..3).map(move |card| Self { card, node }))
            .collect()
    }

    #[must_use]
    pub fn player(&self) -> u8 {
        match self.node {
            Node::Opening | Node::FacingAfterPass => 0,
            Node::AfterPass | Node::FacingBet => 1,
        }
    }

    #[must_use]
    pub fn card(&self) -> u8 {
        self.card
    }

    #[must_use]
    pub fn history(&self) -> &'static [i32] {
        match self.node {
            Node::Opening => &[],
            Node::AfterPass => &[PASS],
            Node::FacingBet => &[BET],
            Node::FacingAfterPass => &[PASS, BET],
        }
    }

    fn index(&self) -> usize {
        let node = NODES.iter().position(|&n| n == self.node).unwrap_or(0);
        node * 3 + usize::from(self.card.min(2))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Strategy {
    bet: [f64; 12],
}

impl Strategy {
    #[must_use]
    pub fn constant(p: f64) -> Self {
        Self { bet: [p; 12] }
    }

    #[must_use]
    pub fn get(&self, set: Infoset) -> f64 {
        self.bet[set.index()]
    }

    pub fn set(&mut self, set: Infoset, p: f64) {
        self.bet[set.index()] = p;
    }

    #[must_use]
    pub fn is_probability(&self) -> bool {
        self.bet.iter().all(|p| (0.0..=1.0).contains(p))
    }
}

fn showdown(c0: u8, c1: u8, stake: f64) -> f64 {
    if c0 > c1 { stake } else { -stake }
}

fn deals() -> impl Iterator<Item = (u8, u8)> {
    (0..3u8).flat_map(|c0| (0..3u8).filter(move |&c1| c1 != c0).map(move |c1| (c0, c1)))
}

fn deal_value(s: &Strategy, c0: u8, c1: u8) -> f64 {
    let open = s.get(Infoset::opening(c0));
    let call = s.get(Infoset::facing_bet(c1));
    let bet_after_pass = s.get(Infoset::after_pass(c1));
    let call_after_pass = s.get(Infoset::facing_after_pass(c0));
    let after_bet = call * showdown(c0, c1, 2.0) + (1.0 - call);
    let after_raise = call_after_pass * showdown(c0, c1, 2.0) - (1.0 - call_after_pass);
    let after_pass = bet_after_pass * after_raise + (1.0 - bet_after_pass) * showdown(c0, c1, 1.0);
    open * after_bet + (1.0 - open) * after_pass
}

#[must_use]
pub fn value(s: &Strategy) -> f64 {
    deals().map(|(c0, c1)| deal_value(s, c0, c1)).sum::<f64>() / 6.0
}

#[must_use]
pub fn best_response(s: &Strategy, player: u8) -> f64 {
    let sets: Vec<Infoset> = Infoset::all()
        .into_iter()
        .filter(|i| i.player() == player)
        .collect();
    let mut best = f64::NEG_INFINITY;
    for bits in 0..(1u32 << sets.len()) {
        let mut pure = s.clone();
        for (k, set) in sets.iter().enumerate() {
            pure.set(*set, f64::from((bits >> k) & 1));
        }
        let u = value(&pure);
        best = best.max(if player == 0 { u } else { -u });
    }
    best
}

#[must_use]
pub fn exploitability(s: &Strategy) -> f64 {
    (best_response(s, 0) + best_response(s, 1)) / 2.0
}

#[must_use]
pub fn reach(s: &Strategy, set: Infoset) -> f64 {
    let card = set.card;
    deals()
        .map(|(c0, c1)| {
            let open = s.get(Infoset::opening(c0));
            match set.node {
                Node::Opening if c0 == card => 1.0,
                Node::AfterPass if c1 == card => 1.0 - open,
                Node::FacingBet if c1 == card => open,
                Node::FacingAfterPass if c0 == card => {
                    (1.0 - open) * s.get(Infoset::after_pass(c1))
                }
                _ => 0.0,
            }
        })
        .sum::<f64>()
        / 6.0
}

#[must_use]
pub fn family(alpha: f64) -> Strategy {
    let mut s = Strategy::constant(0.0);
    s.set(Infoset::opening(0), alpha);
    s.set(Infoset::opening(2), 3.0 * alpha);
    s.set(Infoset::facing_after_pass(1), alpha + 1.0 / 3.0);
    s.set(Infoset::facing_after_pass(2), 1.0);
    s.set(Infoset::after_pass(0), 1.0 / 3.0);
    s.set(Infoset::after_pass(2), 1.0);
    s.set(Infoset::facing_bet(1), 1.0 / 3.0);
    s.set(Infoset::facing_bet(2), 1.0);
    s
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FamilyCheck {
    pub alpha: f64,
    pub jack_bluff: f64,
    pub deviation: f64,
}

#[must_use]
pub fn family_check(s: &Strategy) -> FamilyCheck {
    let alpha = s.get(Infoset::opening(0)).clamp(0.0, 1.0 / 3.0);
    let target = family(alpha);
    let deviation = Infoset::all()
        .into_iter()
        .filter(|&set| reach(s, set) >= REACH_FLOOR)
        .map(|set| (s.get(set) - target.get(set)).abs())
        .fold(0.0, f64::max);
    FamilyCheck {
        alpha: s.get(Infoset::opening(0)),
        jack_bluff: s.get(Infoset::after_pass(0)),
        deviation,
    }
}

pub const MAX_EXPLOITABILITY: f64 = 0.01;
pub const TOLERANCE: f64 = 0.03;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Criterion {
    pub exploitability: f64,
    pub check: FamilyCheck,
    pub pass: bool,
}

#[must_use]
pub fn criterion_2(s: &Strategy) -> Criterion {
    let exploitability = exploitability(s);
    let check = family_check(s);
    let pass = s.is_probability()
        && exploitability <= MAX_EXPLOITABILITY
        && (check.jack_bluff - 1.0 / 3.0).abs() <= TOLERANCE
        && check.deviation <= TOLERANCE;
    Criterion {
        exploitability,
        check,
        pass,
    }
}

#[must_use]
pub fn observation(set: Infoset) -> [f32; 4] {
    let mut obs = [f32::from(set.card), -1.0, -1.0, -1.0];
    for (slot, &a) in obs[1..].iter_mut().zip(set.history()) {
        *slot = a as f32;
    }
    obs
}

pub fn strategy_from_policy(policy: &mut TurnPolicy) -> Strategy {
    let mut s = Strategy::constant(0.0);
    let mut probs = Vec::new();
    for set in Infoset::all() {
        policy.probabilities(&observation(set), &[1, 1], &mut probs);
        s.set(
            set,
            f64::from(probs.get(BET as usize).copied().unwrap_or(0.0)),
        );
    }
    s
}

#[must_use]
pub fn table_key(set: Infoset) -> Vec<u8> {
    info_key(set.player(), &observation(set), &[1, 1])
}

#[must_use]
pub fn strategy_from_table(table: &Table) -> (Strategy, Vec<Infoset>) {
    let mut s = Strategy::constant(0.5);
    let mut missing = Vec::new();
    for set in Infoset::all() {
        match table.get(&table_key(set)) {
            Some(p) => s.set(set, p.get(BET as usize).copied().unwrap_or(0.0)),
            None => missing.push(set),
        }
    }
    (s, missing)
}
