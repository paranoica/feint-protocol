use std::num::NonZeroU64;

use believe::ledger::{Fact, Ledger};
use believe::{Believe, GAME_VERSION, Move, Params};
use feint_ai::{Controller, Opponent, RunHeader, RunReport, Runner, encode_record};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use feint_turn::TurnPilot;

use crate::session::SessionError;

pub const CARD_GAME_ID: &str = "believe";
pub const CARD_LEVEL: u8 = 5;
pub const CARD_CAP_TICKS: u64 = 36_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardEvent {
    Lay {
        seat: u8,
        claim: u8,
        count: u8,
    },
    Call {
        seat: u8,
        honest: bool,
        taker: Option<u8>,
        revealed: Vec<u8>,
    },
    Out {
        seat: u8,
    },
}

#[must_use]
pub const fn event_stride(ranks: usize) -> usize {
    4 + ranks
}

impl CardEvent {
    #[must_use]
    pub fn seat(&self) -> u8 {
        match self {
            Self::Lay { seat, .. } | Self::Call { seat, .. } | Self::Out { seat } => *seat,
        }
    }

    #[must_use]
    pub fn flat(&self, ranks: usize) -> Vec<i32> {
        let mut out = vec![0; event_stride(ranks)];
        match self {
            Self::Lay { seat, claim, count } => {
                out[1] = i32::from(*seat);
                out[2] = i32::from(*claim);
                out[3] = i32::from(*count);
            }
            Self::Call {
                seat,
                honest,
                taker,
                revealed,
            } => {
                out[0] = 1;
                out[1] = i32::from(*seat);
                out[2] = i32::from(*honest);
                out[3] = taker.map_or(-1, i32::from);
                for (slot, &c) in out[4..].iter_mut().zip(revealed) {
                    *slot = i32::from(c);
                }
            }
            Self::Out { seat } => {
                out[0] = 2;
                out[1] = i32::from(*seat);
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardView {
    pub hand: Vec<u8>,
    pub known: Vec<u8>,
    pub mine_in_pile: Vec<u8>,
    pub claim: Option<u8>,
    pub my_last: Vec<u8>,
    pub pile: u32,
    pub last_count: u32,
    pub their_cards: u32,
    pub leader: bool,
    pub batches: u32,
    pub actions_left: u32,
    pub my_turn: bool,
    pub outcome: Option<i8>,
    pub events: Vec<CardEvent>,
}

fn whole(v: f32) -> u32 {
    if v.is_finite() && v > 0.0 {
        v as u32
    } else {
        0
    }
}

impl CardView {
    #[must_use]
    pub fn from_observation(obs: &ObservationT, params: &Params) -> Self {
        let ranks = usize::from(params.ranks);
        let state = obs.state.as_deref().unwrap_or_default();
        let get = |i: usize| state.get(i).copied().unwrap_or(0.0);
        let block = |b: usize| -> Vec<u8> {
            (0..ranks)
                .map(|r| u8::try_from(whole(get(b * ranks + r))).unwrap_or(u8::MAX))
                .collect()
        };
        let tail = 5 * ranks;
        let claim = (0..ranks).find(|&r| get(3 * ranks + r) > 0.5);
        let finished = obs.to_act < 0;
        let score = obs.score.as_deref().unwrap_or_default();
        let outcome = finished.then(|| {
            let mine = score.first().copied().unwrap_or(0);
            let theirs = score.get(1).copied().unwrap_or(0);
            match mine.cmp(&theirs) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => -1,
            }
        });
        Self {
            hand: block(0),
            known: block(1),
            mine_in_pile: block(2),
            claim: claim.and_then(|r| u8::try_from(r).ok()),
            my_last: block(4),
            pile: whole(get(tail)),
            last_count: whole(get(tail + 1)),
            their_cards: whole(get(tail + 2)),
            leader: get(tail + 3) > 0.5,
            batches: whole(get(tail + 4)),
            actions_left: whole(get(tail + 5)),
            my_turn: obs.to_act == obs.player as i8 && !finished,
            outcome,
            events: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Queued {
    me: PlayerId,
    pending: Option<i32>,
}

impl Controller<Believe> for Queued {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.pending = None;
    }

    fn on_tick(&mut self, _game: &Believe, obs: &ObservationT) -> Option<ActionT> {
        if PlayerId::try_from(obs.to_act).ok() != Some(self.me) {
            return None;
        }
        let discrete = self.pending.take()?;
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.discrete = discrete;
        Some(action)
    }
}

pub struct CardMatch {
    game: Believe,
    params: Params,
    runner: Option<Runner>,
    report: Option<RunReport>,
    human: Queued,
    bot: TurnPilot,
    events: Vec<CardEvent>,
    read: usize,
    ledger: Ledger,
}

#[must_use]
pub fn card_header(seed: u64, checkpoint: NonZeroU64) -> RunHeader {
    RunHeader {
        game_id: CARD_GAME_ID.into(),
        game_version: GAME_VERSION,
        seed,
        bodies: vec![BodyParamsT::default(); 2],
        levels: [0, CARD_LEVEL],
        opponent: Opponent::Trained(checkpoint),
    }
}

impl CardMatch {
    pub fn new(seed: u64, checkpoint: u64) -> Result<Self, SessionError> {
        let id = NonZeroU64::new(checkpoint).ok_or(SessionError::Checkpoint)?;
        let policy = believe::trained::policy(checkpoint).map_err(|_| SessionError::Checkpoint)?;
        let params = Params::default();
        let mut game = Believe::new(params.clone()).map_err(|_| SessionError::Unsupported)?;
        let mut human = Queued::default();
        let mut bot = TurnPilot::new(policy);
        let runner = Runner::start(&mut game, [&mut human, &mut bot], card_header(seed, id))
            .map_err(|_| SessionError::Unsupported)?;
        Ok(Self {
            game,
            ledger: Ledger::new(&params),
            params,
            runner: Some(runner),
            report: None,
            human,
            bot,
            events: Vec::new(),
            read: 0,
        })
    }

    fn observation(&self) -> ObservationT {
        let mut obs = ObservationT::default();
        self.game.observe(self.game.live(), 0, &mut obs);
        obs
    }

    #[must_use]
    pub fn view(&self) -> CardView {
        let mut view = CardView::from_observation(&self.observation(), &self.params);
        view.events.clone_from(&self.events);
        view
    }

    #[must_use]
    pub fn legal(&self) -> Vec<i32> {
        if self.runner.is_none() {
            return Vec::new();
        }
        let obs = self.observation();
        obs.legal
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter(|&(_, &l)| l == 1)
            .filter_map(|(i, _)| i32::try_from(i).ok())
            .collect()
    }

    #[must_use]
    pub fn ranks(&self) -> u8 {
        self.params.ranks
    }

    #[must_use]
    pub fn describe(&self, index: i32) -> Vec<i32> {
        match self.game.moves().decode(index) {
            None => Vec::new(),
            Some(Move::Call) => vec![-1],
            Some(Move::Lay { claim, counts }) => std::iter::once(i32::from(claim))
                .chain(
                    counts[..usize::from(self.params.ranks)]
                        .iter()
                        .map(|&c| i32::from(c)),
                )
                .collect(),
        }
    }

    pub fn play(&mut self, index: i32) -> bool {
        if self.human.pending.is_some() || !self.legal().contains(&index) {
            return false;
        }
        self.human.pending = Some(index);
        true
    }

    pub fn advance(&mut self, ticks: u32) {
        for _ in 0..ticks {
            let Some(runner) = self.runner.as_mut() else {
                break;
            };
            let more = runner.step(&mut self.game, [&mut self.human, &mut self.bot]);
            let fresh: Vec<ActionT> = runner.actions()[self.read..].to_vec();
            self.read = runner.actions().len();
            let capped = runner.ticks() >= CARD_CAP_TICKS;
            for action in &fresh {
                self.note(action);
            }
            if !more || capped || self.game.result().is_some() {
                if let Some(runner) = self.runner.take() {
                    self.report =
                        Some(runner.finish(&mut self.game, [&mut self.human, &mut self.bot]));
                }
                break;
            }
        }
    }

    fn note(&mut self, action: &ActionT) {
        let Some(m) = self.game.moves().decode(action.discrete) else {
            return;
        };
        let ranks = usize::from(self.params.ranks);
        for fact in self.ledger.record(action.player, &m) {
            self.events.push(match fact {
                Fact::Lay {
                    seat,
                    claim,
                    counts,
                } => CardEvent::Lay {
                    seat,
                    claim,
                    count: counts[..ranks].iter().sum(),
                },
                Fact::Call {
                    seat,
                    honest,
                    taker,
                    revealed,
                    ..
                } => CardEvent::Call {
                    seat,
                    honest,
                    taker,
                    revealed: revealed[..ranks].to_vec(),
                },
                Fact::Out { seat } => CardEvent::Out { seat },
            });
        }
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.report.is_some()
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        self.game.digest(self.game.live())
    }

    #[must_use]
    pub fn record(&self) -> Vec<u8> {
        let report = match (&self.report, &self.runner) {
            (Some(report), _) => report.clone(),
            (None, Some(runner)) => runner.report(&self.game),
            (None, None) => return Vec::new(),
        };
        encode_record(&report.record())
    }
}
