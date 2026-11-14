use feint_ai::search::{Budget, Game, Model, Prior, Setup, decide};
use std::cell::RefCell;
use std::rc::Rc;

use feint_protocol::{ChaCha8Rng, GameAdapter, ObservationT};
use feint_turn::TurnPolicy;
use rand_chacha::rand_core::RngCore;

use crate::adapter::Believe;
use crate::minds::Person;
use crate::rules::{Counts, MAX_RANKS, Move, Moves, Params, Table, by_rank};
use crate::sample::table_from_view;
use crate::traps::{leaf, learn};

pub const POOL: usize = 32;

pub struct Cards {
    params: Params,
    moves: Moves,
    adapter: Believe,
}

impl Default for Cards {
    fn default() -> Self {
        let params = Params::default();
        Self {
            moves: Moves::new(&params),
            adapter: Believe::default(),
            params,
        }
    }
}

impl Cards {
    #[must_use]
    pub fn params(&self) -> &Params {
        &self.params
    }

    #[must_use]
    pub fn moves(&self) -> &Moves {
        &self.moves
    }

    pub fn observe(&self, table: &Table, player: u8, out: &mut ObservationT) {
        self.adapter.observe(table, player, out);
    }
}

impl Game for Cards {
    type World = Table;
    type Action = Move;

    fn to_act(&self, world: &Table) -> Option<u8> {
        world.outcome.is_none().then_some(world.to_act)
    }

    fn legal(&self, world: &Table, player: u8) -> Vec<Move> {
        world
            .legal(&self.params, &self.moves, player)
            .iter()
            .enumerate()
            .filter(|&(_, &ok)| ok == 1)
            .filter_map(|(i, _)| i32::try_from(i).ok().and_then(|i| self.moves.decode(i)))
            .collect()
    }

    fn apply(&self, world: &mut Table, player: u8, action: Move) {
        let _ = world.play(&self.params, player, &action);
    }

    fn value(&self, world: &Table, me: u8) -> f64 {
        leaf(world, me)
    }

    fn private(&self, world: &Table, me: u8) -> u64 {
        let me = usize::from(me.min(1));
        let hand = by_rank(&self.params, world.hands[me]);
        hand.iter()
            .chain(world.known[me].iter())
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, &c| {
                (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01b3)
            })
    }

    fn public(&self, _world: &Table, _player: u8, action: Move) -> u64 {
        match action {
            Move::Call => 0,
            Move::Lay { claim, counts } => {
                let n: u64 = counts.iter().map(|&c| u64::from(c)).sum();
                1 + u64::from(claim) * 16 + n
            }
        }
    }
}

pub trait LayModel {
    fn lay_odds(
        &self,
        game: &Cards,
        pre: &Table,
        player: u8,
        claim: usize,
        n: u32,
    ) -> Vec<(Counts, f64)>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oracle {
    pub person: Person,
    pub me: u8,
}

impl Model<Cards> for Oracle {
    fn policy(&self, game: &Cards, world: &Table, player: u8) -> Vec<(Move, f64)> {
        self.person.moves(&game.params, world, player)
    }

    fn observe(&mut self, game: &Cards, before: &Table, player: u8, action: Move) {
        learn(
            &mut self.person,
            &game.params,
            before,
            player,
            &action,
            self.me,
        );
    }
}

impl LayModel for Oracle {
    fn lay_odds(
        &self,
        game: &Cards,
        pre: &Table,
        player: u8,
        claim: usize,
        n: u32,
    ) -> Vec<(Counts, f64)> {
        let hand = by_rank(&game.params, pre.hands[usize::from(player)]);
        self.person.lay_odds(&game.params, &hand, claim, n)
    }
}

pub struct PolicyPrior {
    policy: TurnPolicy,
    obs: ObservationT,
    probabilities: Vec<f32>,
}

impl PolicyPrior {
    #[must_use]
    pub fn new(policy: TurnPolicy) -> Self {
        Self {
            policy,
            obs: ObservationT::default(),
            probabilities: Vec::new(),
        }
    }
}

impl Prior<Cards> for PolicyPrior {
    fn prior(&mut self, game: &Cards, world: &Table, me: u8, legal: &[Move]) -> Vec<f64> {
        game.observe(world, me, &mut self.obs);
        let features = self.obs.state.as_deref().unwrap_or_default();
        let mask = self.obs.legal.as_deref().unwrap_or_default();
        self.policy
            .probabilities(features, mask, &mut self.probabilities);
        legal
            .iter()
            .map(|m| {
                game.moves
                    .index(m)
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| self.probabilities.get(i))
                    .map_or(0.0, |&p| f64::from(p))
            })
            .collect()
    }
}

fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn take_random(from: u64, n: u32, rng: &mut ChaCha8Rng) -> u64 {
    let mut chosen = 0u64;
    let mut rest = from;
    for _ in 0..n {
        let size = rest.count_ones();
        if size == 0 {
            break;
        }
        let mut pick = rng.next_u32() % size;
        let mut bits = rest;
        while pick > 0 {
            bits &= bits - 1;
            pick -= 1;
        }
        let bit = bits & bits.wrapping_neg();
        chosen |= bit;
        rest &= !bit;
    }
    chosen
}

fn take_counts(params: &Params, from: u64, counts: &Counts, rng: &mut ChaCha8Rng) -> u64 {
    let c = u32::from(params.copies);
    let mut mask = 0u64;
    for (rank, &n) in counts.iter().enumerate().take(usize::from(params.ranks)) {
        let of_rank = from & (((1u64 << c) - 1) << (rank as u32 * c));
        mask |= take_random(of_rank, u32::from(n), rng);
    }
    mask
}

fn candidate<L: LayModel>(
    game: &Cards,
    obs: &ObservationT,
    model: &L,
    rng: &mut ChaCha8Rng,
) -> (Table, f64) {
    let params = &game.params;
    let me = usize::from(obs.player.min(1));
    let other = 1 - me;
    let mut t = table_from_view(params, obs, rng);
    let theirs_last = t.batches > 0 && usize::from(t.last_by) == other;
    if !theirs_last {
        return (t, 1.0);
    }
    let unweighted = t.clone();
    let last_n = t.last.count_ones();
    let laid_n = t.laid[other].count_ones();
    let possession = t.hands[other] | t.laid[other];
    let earlier = take_random(possession, laid_n.saturating_sub(last_n), rng);
    let before = possession & !earlier;
    let claim = t.claim.map_or(0, usize::from);
    let pre = before_the_lay(&t, me, before, earlier, rng);
    let odds = model.lay_odds(game, &pre, other as u8, claim, last_n);
    let weight: f64 = odds.iter().map(|o| o.1.max(0.0)).sum();
    if weight <= 0.0 {
        return (unweighted, 0.0);
    }
    let u = unit(rng) * weight;
    let mut acc = 0.0;
    let mut counts: Counts = [0; MAX_RANKS];
    for (c, p) in &odds {
        acc += p.max(0.0);
        counts = *c;
        if u < acc {
            break;
        }
    }
    let last = take_counts(params, before, &counts, rng);
    t.last = last;
    t.laid[other] = earlier | last;
    t.hands[other] = before & !last;
    if last.count_ones() != last_n || t.validate(params).is_err() {
        return (unweighted, 0.0);
    }
    (t, weight)
}

#[must_use]
pub fn before_the_lay(
    t: &Table,
    me: usize,
    before: u64,
    earlier: u64,
    rng: &mut ChaCha8Rng,
) -> Table {
    let other = 1 - me;
    let mut pre = t.clone();
    pre.hands[other] = before;
    pre.laid[other] = earlier;
    pre.batches = t.batches.saturating_sub(1);
    pre.to_act = other as u8;
    pre.out = None;
    pre.actions = t.actions.saturating_sub(1);
    if pre.batches == 0 {
        pre.claim = None;
        pre.last = 0;
    } else {
        pre.last = if t.batches <= 3 {
            t.laid[me]
        } else {
            take_random(t.laid[me], 1, rng)
        };
        pre.last_by = me as u8;
    }
    pre
}

pub fn worlds<L: LayModel>(
    game: &Cards,
    obs: &ObservationT,
    model: &L,
    rng: &mut ChaCha8Rng,
    n: usize,
) -> Vec<(Table, f64)> {
    let pool: Vec<(Table, f64)> = (0..n.max(1))
        .map(|_| candidate(game, obs, model, rng))
        .collect();
    if pool.iter().any(|w| w.1 > 0.0) {
        pool
    } else {
        pool.into_iter().map(|(t, _)| (t, 1.0)).collect()
    }
}

fn draw(pool: &[(Table, f64)], rng: &mut ChaCha8Rng) -> Table {
    let total: f64 = pool.iter().map(|w| w.1).sum();
    let u = unit(rng) * total;
    let mut acc = 0.0;
    for (t, w) in pool {
        acc += w;
        if u < acc {
            return t.clone();
        }
    }
    pool.iter()
        .rev()
        .find(|w| w.1 > 0.0)
        .map_or_else(|| pool[0].0.clone(), |w| w.0.clone())
}

pub fn world<L: LayModel>(
    game: &Cards,
    obs: &ObservationT,
    model: &L,
    rng: &mut ChaCha8Rng,
) -> Table {
    let pool = worlds(game, obs, model, rng, POOL);
    draw(&pool, rng)
}

pub fn choose<M: Model<Cards> + LayModel>(
    game: &Cards,
    obs: &ObservationT,
    model: &M,
    prior: &mut PolicyPrior,
    budget: Budget,
    rng: &mut ChaCha8Rng,
) -> Option<Move> {
    let me = obs.player.min(1);
    let mut setup = Setup {
        game,
        model,
        prior,
        me,
        budget,
    };
    let size = POOL.max(usize::try_from(budget.iterations / 4).unwrap_or(POOL));
    let pool = worlds(game, obs, model, rng, size);
    let mut sample = |r: &mut ChaCha8Rng| draw(&pool, r);
    decide(&mut setup, &mut sample, rng)
}

#[derive(Clone)]
pub struct PolicyModel {
    policy: Rc<RefCell<TurnPolicy>>,
}

impl PolicyModel {
    #[must_use]
    pub fn new(policy: TurnPolicy) -> Self {
        Self {
            policy: Rc::new(RefCell::new(policy)),
        }
    }
}

impl Model<Cards> for PolicyModel {
    fn policy(&self, game: &Cards, world: &Table, player: u8) -> Vec<(Move, f64)> {
        let mut obs = ObservationT::default();
        game.observe(world, player, &mut obs);
        let features = obs.state.as_deref().unwrap_or_default();
        let mask = obs.legal.as_deref().unwrap_or_default();
        let mut probabilities = Vec::new();
        self.policy
            .borrow_mut()
            .probabilities(features, mask, &mut probabilities);
        probabilities
            .iter()
            .enumerate()
            .filter(|&(i, &p)| p > 0.0 && mask.get(i) == Some(&1))
            .filter_map(|(i, &p)| {
                let m = game.moves.decode(i32::try_from(i).ok()?)?;
                Some((m, f64::from(p)))
            })
            .collect()
    }

    fn observe(&mut self, _game: &Cards, _before: &Table, _player: u8, _action: Move) {}
}

impl LayModel for PolicyModel {
    fn lay_odds(
        &self,
        game: &Cards,
        pre: &Table,
        player: u8,
        claim: usize,
        n: u32,
    ) -> Vec<(Counts, f64)> {
        self.policy(game, pre, player)
            .into_iter()
            .filter_map(|(m, p)| match m {
                Move::Lay { claim: c, counts }
                    if usize::from(c) == claim
                        && counts.iter().map(|&k| u32::from(k)).sum::<u32>() == n =>
                {
                    Some((counts, p))
                }
                _ => None,
            })
            .collect()
    }
}
