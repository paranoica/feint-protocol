use feint_ai::Controller;
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventT, GameAdapter,
    GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};
use feint_turn::TurnGame;
use rand_chacha::rand_core::{RngCore, SeedableRng};

pub const PASS: i32 = 0;
pub const BET: i32 = 1;
pub const GAME_VERSION: u32 = 1;
pub const TICK_HZ: u32 = 10;
pub const OBSERVATION_LEN: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deal {
    pub cards: [u8; 2],
    pub history: Vec<i32>,
    pub tick: u64,
}

impl Deal {
    #[must_use]
    pub fn to_act(&self) -> Option<PlayerId> {
        match self.history.as_slice() {
            [] | [PASS, BET] => Some(0),
            [_] => Some(1),
            _ => None,
        }
    }

    #[must_use]
    pub fn payoff(&self) -> Option<[i64; 2]> {
        let showdown = |stake: i64| {
            if self.cards[0] > self.cards[1] {
                [stake, -stake]
            } else {
                [-stake, stake]
            }
        };
        match self.history.as_slice() {
            [PASS, PASS] => Some(showdown(1)),
            [BET, PASS] => Some([1, -1]),
            [PASS, BET, PASS] => Some([-1, 1]),
            [BET, BET] | [PASS, BET, BET] => Some(showdown(2)),
            _ => None,
        }
    }

    fn play(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.tick != self.tick {
            return Err(AdapterError::WrongTick {
                expected: self.tick,
                got: action.tick,
            });
        }
        if action.player > 1 {
            return Err(AdapterError::BadPlayer(action.player));
        }
        let Some(to_act) = self.to_act() else {
            return Err(AdapterError::MatchOver);
        };
        if action.player != to_act {
            return Err(AdapterError::BadAction("not this player's turn"));
        }
        if !(PASS..=BET).contains(&action.discrete) {
            return Err(AdapterError::BadAction("pass or bet only"));
        }
        self.history.push(action.discrete);
        Ok(())
    }
}

pub struct Kuhn {
    deal: Deal,
}

impl Default for Kuhn {
    fn default() -> Self {
        Self::new()
    }
}

impl Kuhn {
    #[must_use]
    pub fn new() -> Self {
        Self::from_deal(Deal {
            cards: [0, 1],
            history: vec![],
            tick: 0,
        })
    }

    #[must_use]
    pub fn from_deal(deal: Deal) -> Self {
        Self { deal }
    }
}

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    let mut d = DimT::default();
    d.name = Some(name.into());
    d.min = min;
    d.max = max;
    d.unit = Some(unit.into());
    d
}

impl GameAdapter for Kuhn {
    type State = Deal;

    fn spec(&self) -> GameSpecT {
        let mut spec = GameSpecT::default();
        spec.game_id = Some("kuhn-poker".into());
        spec.game_version = GAME_VERSION;
        spec.num_players = 2;
        spec.time_mode = TimeMode::TurnBased;
        spec.tick_hz = TICK_HZ;
        spec.decision_hz = TICK_HZ;
        let mut space = ActionSpaceT::default();
        space.discrete = 2;
        spec.action_space = Some(Box::new(space));
        spec.observation = Some(vec![
            dim("card", 0.0, 2.0, "rank"),
            dim("first", -1.0, 1.0, "action"),
            dim("second", -1.0, 1.0, "action"),
            dim("third", -1.0, 1.0, "action"),
        ]);
        spec.signals = Some(vec![dim("action", 0.0, 1.0, "bet")]);
        spec
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        if bodies.len() != 2 {
            return Err(AdapterError::BadBody("one entry per player"));
        }
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let first = (rng.next_u32() % 3) as u8;
        let second = (first + 1 + (rng.next_u32() % 2) as u8) % 3;
        self.deal = Deal {
            cards: [first, second],
            history: vec![],
            tick: 0,
        };
        Ok(())
    }

    fn live(&self) -> &Deal {
        &self.deal
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        self.deal.play(action)
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {
        self.deal.tick = self.deal.tick.saturating_add(1);
    }

    fn observe(&self, state: &Deal, player: PlayerId, out: &mut ObservationT) {
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
        out.to_act = state.to_act().map_or(-1, |p| p as i8);
        let obs = out.state.get_or_insert_with(Vec::new);
        obs.clear();
        obs.push(f32::from(state.cards[me]));
        obs.extend((0..3).map(|i| state.history.get(i).map_or(-1.0, |&a| a as f32)));
        let signals = out.opponent_signals.get_or_insert_with(Vec::new);
        signals.clear();
        signals.extend(
            state
                .history
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 2 != me)
                .map(|(_, &a)| a as f32),
        );
        let my_turn = u8::from(state.to_act() == Some(player));
        let legal = out.legal.get_or_insert_with(Vec::new);
        legal.clear();
        legal.extend([my_turn, my_turn]);
        let score = out.score.get_or_insert_with(Vec::new);
        score.clear();
        score.extend(state.payoff().unwrap_or([0, 0]));
    }

    fn state_from_observation(&self, obs: &ObservationT, rng: &mut ChaCha8Rng) -> Deal {
        let seen = obs.state.as_deref().unwrap_or_default();
        let own = seen.first().map_or(0, |&c| (c as u8).min(2));
        let other = (own + 1 + (rng.next_u32() % 2) as u8) % 3;
        let mut cards = [own, other];
        if obs.player == 1 {
            cards.swap(0, 1);
        }
        Deal {
            cards,
            history: seen
                .iter()
                .skip(1)
                .filter(|&&a| a >= 0.0)
                .map(|&a| a as i32)
                .collect(),
            tick: obs.tick,
        }
    }

    fn simulate(&self, state: &Deal, actions: &[ActionT], ticks: u32) -> Deal {
        let mut next = state.clone();
        for action in actions {
            let mut stamped = action.clone();
            stamped.tick = next.tick;
            let _ = next.play(&stamped);
        }
        next.tick = next.tick.saturating_add(u64::from(ticks));
        next
    }

    fn signals(&self, _state: &Deal, action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
        out.push(action.discrete as f32);
    }

    fn digest(&self, state: &Deal) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for byte in state
            .cards
            .iter()
            .copied()
            .chain(state.history.iter().map(|&a| a as u8))
            .chain(state.tick.to_le_bytes())
        {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    fn result(&self) -> Option<MatchResultT> {
        self.deal.payoff().map(|score| {
            let mut result = MatchResultT::default();
            result.score = Some(score.to_vec());
            result.duration_ticks = self.deal.tick;
            result
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habit {
    Pass,
    Bet,
    Random,
}

pub struct Habitual {
    habit: Habit,
    me: PlayerId,
    rng: ChaCha8Rng,
}

impl Habitual {
    #[must_use]
    pub fn new(habit: Habit) -> Self {
        Self {
            habit,
            me: 0,
            rng: ChaCha8Rng::seed_from_u64(0),
        }
    }
}

impl Controller<Kuhn> for Habitual {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.rng =
            ChaCha8Rng::seed_from_u64(seed ^ 0x5bd1_e995_u64.wrapping_mul(u64::from(me) + 1));
    }

    fn on_tick(&mut self, _adapter: &Kuhn, obs: &ObservationT) -> Option<ActionT> {
        if obs.to_act != self.me as i8 {
            return None;
        }
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.discrete = match self.habit {
            Habit::Pass => PASS,
            Habit::Bet => BET,
            Habit::Random => (self.rng.next_u32() % 2) as i32,
        };
        Some(action)
    }
}

impl TurnGame for Kuhn {
    fn fresh() -> Self {
        Self::new()
    }

    fn reward(score: &[i64], me: usize) -> f32 {
        score.get(me).map_or(0.0, |&chips| chips as f32)
    }

    fn scripted(name: &str) -> Option<Box<dyn Controller<Self>>> {
        let habit = match name {
            "pass" => Habit::Pass,
            "bet" => Habit::Bet,
            "random" => Habit::Random,
            _ => return None,
        };
        Some(Box::new(Habitual::new(habit)))
    }
}
