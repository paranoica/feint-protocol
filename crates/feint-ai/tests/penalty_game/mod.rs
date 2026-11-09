#![allow(dead_code)]

use feint_ai::{Choice, Controller, Cue, Playbook, Read, Stance};
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventT, GameAdapter, GameSpecT,
    MatchResultT, ObservationT, PlayerId, TimeMode,
};

pub const STRIKER: PlayerId = 0;
pub const KEEPER: PlayerId = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Kick {
    pub shown: u8,
    pub real: u8,
    pub cover: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PenaltyState {
    pub tick: u64,
    pub round: u32,
    pub rounds: u32,
    pub shown: Option<u8>,
    pub real: Option<u8>,
    pub last: Option<Kick>,
    pub score: [u32; 2],
}

impl PenaltyState {
    pub fn to_act(&self) -> PlayerId {
        if self.shown.is_none() {
            STRIKER
        } else {
            KEEPER
        }
    }

    fn kick(&mut self, shown: u8, real: u8) {
        self.shown = Some(shown);
        self.real = Some(real);
        self.tick = self.tick.saturating_add(1);
    }

    fn cover(&mut self, cover: u8, predicted_real: Option<u8>) {
        let shown = self.shown.unwrap_or(0);
        let real = self.real.or(predicted_real).unwrap_or(shown);
        if real != cover {
            self.score[usize::from(STRIKER)] += 1;
        } else {
            self.score[usize::from(KEEPER)] += 1;
        }
        self.last = Some(Kick { shown, real, cover });
        self.shown = None;
        self.real = None;
        self.round += 1;
        self.tick = self.tick.saturating_add(1);
    }

    pub fn over(&self) -> bool {
        self.round >= self.rounds
    }
}

pub struct Penalty {
    pub state: PenaltyState,
}

impl Penalty {
    pub fn new(rounds: u32) -> Self {
        Self {
            state: PenaltyState {
                tick: 0,
                round: 0,
                rounds,
                shown: None,
                real: None,
                last: None,
                score: [0, 0],
            },
        }
    }
}

pub fn penalty_spec() -> GameSpecT {
    let mut spec = GameSpecT::default();
    spec.game_id = Some("penalty".into());
    spec.time_mode = TimeMode::TurnBased;
    let mut space = ActionSpaceT::default();
    space.discrete = 4;
    spec.action_space = Some(Box::new(space));
    spec
}

fn decode_kick(discrete: i32) -> Option<(u8, u8)> {
    let d = u8::try_from(discrete).ok().filter(|&d| d < 4)?;
    Some((d / 2, d % 2))
}

fn encode_kick(shown: u8, real: u8) -> i32 {
    i32::from(shown * 2 + real)
}

fn as_f32(v: Option<u8>) -> f32 {
    v.map_or(-1.0, f32::from)
}

fn from_f32(v: f32) -> Option<u8> {
    (v >= 0.0).then_some(v as u8)
}

impl GameAdapter for Penalty {
    type State = PenaltyState;

    fn spec(&self) -> GameSpecT {
        penalty_spec()
    }

    fn reset(&mut self, _seed: u64, _bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        self.state = Penalty::new(self.state.rounds).state;
        Ok(())
    }

    fn live(&self) -> &PenaltyState {
        &self.state
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if self.state.over() {
            return Err(AdapterError::MatchOver);
        }
        if action.player != self.state.to_act() {
            return Err(AdapterError::BadAction("not this player's turn"));
        }
        if action.player == STRIKER {
            let (shown, real) =
                decode_kick(action.discrete).ok_or(AdapterError::BadAction("kick"))?;
            self.state.kick(shown, real);
        } else {
            let cover = u8::try_from(action.discrete)
                .ok()
                .filter(|&c| c < 2)
                .ok_or(AdapterError::BadAction("cover"))?;
            self.state.cover(cover, None);
        }
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {}

    fn observe(&self, state: &PenaltyState, player: PlayerId, out: &mut ObservationT) {
        out.tick = state.tick;
        out.player = player;
        out.to_act = if state.over() {
            -1
        } else {
            state.to_act() as i8
        };
        let real = if player == STRIKER { state.real } else { None };
        let last = state.last.as_ref();
        out.state = Some(vec![
            state.round as f32,
            state.rounds as f32,
            as_f32(state.shown),
            as_f32(real),
            as_f32(last.map(|k| k.shown)),
            as_f32(last.map(|k| k.real)),
            as_f32(last.map(|k| k.cover)),
            state.score[0] as f32,
            state.score[1] as f32,
        ]);
        let mine = !state.over() && state.to_act() == player;
        let n = if player == STRIKER { 4 } else { 2 };
        out.legal = Some(vec![u8::from(mine); n]);
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> PenaltyState {
        let v: Vec<f32> = obs.state.clone().unwrap_or_default();
        let at = |i: usize| v.get(i).copied().unwrap_or(-1.0);
        let last = match (from_f32(at(4)), from_f32(at(5)), from_f32(at(6))) {
            (Some(shown), Some(real), Some(cover)) => Some(Kick { shown, real, cover }),
            _ => None,
        };
        PenaltyState {
            tick: obs.tick,
            round: at(0).max(0.0) as u32,
            rounds: at(1).max(0.0) as u32,
            shown: from_f32(at(2)),
            real: from_f32(at(3)),
            last,
            score: [at(7).max(0.0) as u32, at(8).max(0.0) as u32],
        }
    }

    fn simulate(&self, state: &PenaltyState, actions: &[ActionT], ticks: u32) -> PenaltyState {
        let mut next = state.clone();
        let predicted = actions
            .iter()
            .find(|a| a.player == STRIKER)
            .and_then(|a| decode_kick(a.discrete))
            .map(|(_, real)| real);
        for _ in 0..ticks {
            for action in actions {
                if next.over() || action.player != next.to_act() {
                    continue;
                }
                if action.player == STRIKER {
                    if let Some((shown, real)) = decode_kick(action.discrete) {
                        next.kick(shown, real);
                    }
                } else if let Ok(cover) = u8::try_from(action.discrete) {
                    next.cover(cover.min(1), predicted);
                }
            }
        }
        next
    }

    fn signals(&self, _state: &PenaltyState, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        if let Some((shown, _)) = decode_kick(action.discrete) {
            out.push(f32::from(shown));
        }
    }

    fn digest(&self, state: &PenaltyState) -> u64 {
        u64::from(state.round) * 1_000_003
            + u64::from(state.score[0]) * 1_009
            + u64::from(state.score[1])
    }

    fn result(&self) -> Option<MatchResultT> {
        self.state.over().then(|| {
            let mut r = MatchResultT::default();
            r.score = Some(self.state.score.iter().map(|&s| i64::from(s)).collect());
            r
        })
    }
}

pub struct PenaltyPlaybook;

pub const COVER_IDS: u16 = 10;

fn choice(id: u16, shown: Option<u8>, real: Option<u8>, deceptive: bool) -> Choice {
    Choice {
        id,
        intent: u8::from(deceptive),
        deceptive,
        horizon_ticks: 1,
        param: [f32::from(shown.unwrap_or(0)), f32::from(real.unwrap_or(0))],
        shown,
        real,
    }
}

impl Playbook<Penalty> for PenaltyPlaybook {
    fn options(
        &self,
        _s: &PenaltyState,
        me: PlayerId,
        _rep: u8,
        _cue: &Cue,
        out: &mut Vec<Choice>,
    ) {
        out.clear();
        if me == STRIKER {
            for side in 0..2u8 {
                out.push(choice(u16::from(side), Some(side), Some(side), false));
            }
            for side in 0..2u8 {
                out.push(choice(
                    2 + u16::from(side),
                    Some(side),
                    Some(1 - side),
                    true,
                ));
            }
        } else {
            for side in 0..2u8 {
                out.push(choice(COVER_IDS + u16::from(side), None, Some(side), false));
            }
        }
    }

    fn act(&self, _s: &PenaltyState, me: PlayerId, c: &Choice, _e: u64, _aim: [f64; 2]) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.discrete = if me == STRIKER {
            encode_kick(c.shown.unwrap_or(0), c.real.unwrap_or(0))
        } else {
            i32::from(c.real.unwrap_or(0))
        };
        a
    }

    fn opponent_move(&self, _s: &PenaltyState, them: PlayerId, stance: Stance) -> ActionT {
        let mut a = ActionT::default();
        a.player = them;
        a.discrete = match (them, stance) {
            (STRIKER, Stance::Shoot(c)) => encode_kick(c.min(1), c.min(1)),
            (STRIKER, _) => encode_kick(0, 0),
            (_, Stance::Cover(c)) => i32::from(c.min(1)),
            _ => 0,
        };
        a
    }

    fn evaluate(&self, s: &PenaltyState, me: PlayerId) -> f64 {
        let me = usize::from(me.min(1));
        f64::from(s.score[me]) - f64::from(s.score[1 - me])
    }

    fn margin(&self) -> f64 {
        0.0
    }

    fn read(&self, before: &PenaltyState, after: &PenaltyState, them: PlayerId) -> Read {
        if after.round <= before.round {
            return Read::default();
        }
        let Some(kick) = after.last.as_ref() else {
            return Read::default();
        };
        if them == KEEPER {
            Read {
                covered: Some(kick.cover),
                shot: None,
            }
        } else {
            Read {
                covered: None,
                shot: Some(kick.real),
            }
        }
    }

    fn importance(&self, s: &PenaltyState, _me: PlayerId) -> f64 {
        if s.round + 1 >= s.rounds { 1.0 } else { 0.0 }
    }

    fn possession(&self, s: &PenaltyState, me: PlayerId) -> Option<u64> {
        (!s.over() && s.to_act() == me).then_some(u64::from(s.round))
    }
}

pub enum KeeperRule {
    Biter,
    Anticipator,
    Adaptive,
}

pub struct Keeper {
    pub rule: KeeperRule,
    history: [[u32; 2]; 2],
    seen_round: u32,
}

impl Keeper {
    pub fn new(rule: KeeperRule) -> Self {
        Self {
            rule,
            history: [[0; 2]; 2],
            seen_round: 0,
        }
    }
}

impl Controller<Penalty> for Keeper {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {
        self.history = [[0; 2]; 2];
        self.seen_round = 0;
    }

    fn on_tick(&mut self, game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        let mut rng = <ChaCha8Rng as rand_chacha::rand_core::SeedableRng>::seed_from_u64(0);
        let s = game.state_from_observation(obs, &mut rng);
        if s.round > self.seen_round {
            if let Some(k) = &s.last {
                self.history[usize::from(k.shown)][usize::from(k.real)] += 1;
            }
            self.seen_round = s.round;
        }
        if obs.to_act != KEEPER as i8 {
            return None;
        }
        let shown = s.shown?;
        let cover = match self.rule {
            KeeperRule::Biter => shown,
            KeeperRule::Anticipator => 1 - shown,
            KeeperRule::Adaptive => {
                let row = self.history[usize::from(shown)];
                if row[usize::from(1 - shown)] > row[usize::from(shown)] {
                    1 - shown
                } else {
                    shown
                }
            }
        };
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = KEEPER;
        a.discrete = i32::from(cover);
        Some(a)
    }
}

pub struct HabitStriker(pub u8);

impl Controller<Penalty> for HabitStriker {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {}

    fn on_tick(&mut self, _game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        if obs.to_act != STRIKER as i8 {
            return None;
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = STRIKER;
        a.discrete = encode_kick(self.0, self.0);
        Some(a)
    }
}
