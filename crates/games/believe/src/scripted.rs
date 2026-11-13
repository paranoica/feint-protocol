use feint_ai::Controller;
use feint_protocol::{ActionT, GameSpecT, ObservationT, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::adapter::Believe;
use crate::rules::{MAX_RANKS, Move};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Honest,
    Liar(u16),
    Caller(u16),
    Counter,
}

pub struct Scripted {
    style: Style,
    me: PlayerId,
    rng: ChaCha8Rng,
}

struct View<'a> {
    hand: &'a [f32],
    laid: &'a [f32],
    claim: Option<usize>,
    last_batch: u8,
    opponent_hand: f32,
    batches: f32,
}

impl<'a> View<'a> {
    fn read(state: &'a [f32], ranks: usize) -> Option<Self> {
        let group = |g: usize| state.get(g * ranks..(g + 1) * ranks);
        let scalar = |i: usize| state.get(5 * ranks + i).copied();
        Some(Self {
            hand: group(0)?,
            laid: group(2)?,
            claim: group(3)?.iter().position(|&f| f > 0.5),
            last_batch: scalar(1)? as u8,
            opponent_hand: scalar(2)?,
            batches: scalar(4)?,
        })
    }
}

impl Scripted {
    #[must_use]
    pub fn new(style: Style) -> Self {
        Self {
            style,
            me: 0,
            rng: ChaCha8Rng::seed_from_u64(0),
        }
    }

    fn chance(&mut self, permille: u16) -> bool {
        self.rng.next_u32() % 1000 < u32::from(permille)
    }

    fn lay(claim: usize, rank: usize, n: u8) -> Move {
        let mut counts = [0u8; MAX_RANKS];
        counts[rank] = n;
        Move::Lay {
            claim: claim as u8,
            counts,
        }
    }

    fn most_held(hand: &[f32], besides: Option<usize>) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (rank, &n) in hand.iter().enumerate() {
            if n < 1.0 || Some(rank) == besides {
                continue;
            }
            if best.is_none_or(|b| n > hand[b]) {
                best = Some(rank);
            }
        }
        best
    }

    fn choose(&mut self, view: &View, max_batch: u8, copies: u8) -> Option<Move> {
        let Some(claim) = view.claim.filter(|_| view.batches > 0.0) else {
            let rank = Self::most_held(view.hand, None)?;
            return Some(Self::lay(
                rank,
                rank,
                (view.hand[rank] as u8).min(max_batch),
            ));
        };
        if view.opponent_hand < 1.0 {
            return Some(Move::Call);
        }
        let held = view.hand[claim] as u8;
        let true_lay = (held > 0).then(|| Self::lay(claim, claim, held.min(max_batch)));
        let false_lay =
            Self::most_held(view.hand, Some(claim)).map(|rank| Self::lay(claim, rank, 1));
        match self.style {
            Style::Honest => true_lay.or(Some(Move::Call)),
            Style::Caller(permille) => {
                if self.chance(permille) {
                    Some(Move::Call)
                } else {
                    true_lay.or(Some(Move::Call))
                }
            }
            Style::Counter => {
                let accounted = held.saturating_add(view.laid[claim] as u8);
                if view.last_batch > copies.saturating_sub(accounted) {
                    Some(Move::Call)
                } else {
                    true_lay.or(Some(Move::Call))
                }
            }
            Style::Liar(permille) => match true_lay {
                Some(honest) if !self.chance(permille) => Some(honest),
                honest => false_lay.or(honest).or(Some(Move::Call)),
            },
        }
    }
}

impl Controller<Believe> for Scripted {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.rng = ChaCha8Rng::seed_from_u64(
            seed ^ 0x9e37_79b9_7f4a_7c15_u64.wrapping_mul(u64::from(me) + 1),
        );
    }

    fn on_tick(&mut self, adapter: &Believe, obs: &ObservationT) -> Option<ActionT> {
        if obs.to_act != self.me as i8 {
            return None;
        }
        let params = adapter.params();
        let state = obs.state.as_deref()?;
        let view = View::read(state, usize::from(params.ranks))?;
        let m = self.choose(&view, params.max_batch, params.copies)?;
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.discrete = adapter.moves().index(&m)?;
        Some(action)
    }
}

impl Style {
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let rate = |prefix: &str| {
            name.strip_prefix(prefix)
                .and_then(|n| n.parse::<u16>().ok())
                .filter(|&n| n <= 1000)
        };
        match name {
            "honest" => Some(Self::Honest),
            "counter" => Some(Self::Counter),
            _ => rate("liar:")
                .map(Self::Liar)
                .or_else(|| rate("caller:").map(Self::Caller)),
        }
    }
}

impl feint_turn::TurnGame for Believe {
    fn fresh() -> Self {
        Self::default()
    }

    fn reward(score: &[i64], me: usize) -> f32 {
        let mine = score.get(me).copied().unwrap_or(0);
        let theirs = score.get(1 - me.min(1)).copied().unwrap_or(0);
        (mine - theirs).signum() as f32
    }

    fn scripted(name: &str) -> Option<Box<dyn Controller<Self>>> {
        Style::named(name).map(|style| Box::new(Scripted::new(style)) as Box<dyn Controller<Self>>)
    }
}
