//! A minimal Kuhn-poker adapter: shows that `GameAdapter` carries a turn-based game with
//! hidden information and no body, as stage 3 needs, without changing the trait.

use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventT, GameAdapter, GameSpecT,
    MatchResultT, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

/// Build a generated `#[non_exhaustive]` protocol type: its default with the given fields.
macro_rules! make {
    ($t:ident { $($field:ident : $value:expr),* $(,)? }) => {{
        let mut built = $t::default();
        $(built.$field = $value;)*
        built
    }};
}

const PASS: i32 = 0;
const BET: i32 = 1;

#[derive(Clone, Debug, PartialEq)]
struct Deal {
    cards: [u8; 2],
    history: Vec<i32>,
    tick: u64,
}

impl Deal {
    fn to_act(&self) -> Option<PlayerId> {
        match self.history.as_slice() {
            [] | [PASS, BET] => Some(0),
            [_] => Some(1),
            _ => None,
        }
    }

    fn payoff(&self) -> Option<[i64; 2]> {
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
        self.tick += 1;
        Ok(())
    }
}

struct Kuhn {
    deal: Deal,
}

impl Kuhn {
    fn new() -> Self {
        Self {
            deal: Deal {
                cards: [0, 1],
                history: vec![],
                tick: 0,
            },
        }
    }
}

impl GameAdapter for Kuhn {
    type State = Deal;

    fn spec(&self) -> GameSpecT {
        make!(GameSpecT {
            game_id: Some("kuhn-poker".into()),
            game_version: 1,
            num_players: 2,
            time_mode: TimeMode::TurnBased,
            action_space: Some(Box::new(make!(ActionSpaceT {
                continuous: None,
                discrete: 2,
            }))),
        })
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

    fn advance(&mut self, _events: &mut Vec<EventT>) {}

    fn observe(&self, state: &Deal, player: PlayerId, out: &mut ObservationT) {
        let me = usize::from(player);
        out.tick = state.tick;
        out.player = player;
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
        let own = seen[0] as u8;
        let other = (own + 1 + (rng.next_u32() % 2) as u8) % 3;
        let mut cards = [own, other];
        if obs.player == 1 {
            cards.swap(0, 1);
        }
        Deal {
            cards,
            history: seen[1..]
                .iter()
                .filter(|&&a| a >= 0.0)
                .map(|&a| a as i32)
                .collect(),
            tick: obs.tick,
        }
    }

    fn simulate(&self, state: &Deal, actions: &[ActionT], ticks: u32) -> Deal {
        let mut next = state.clone();
        for action in actions.iter().take(ticks as usize) {
            let mut stamped = action.clone();
            stamped.tick = next.tick;
            if next.play(&stamped).is_err() {
                break;
            }
        }
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
        {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    fn result(&self) -> Option<MatchResultT> {
        self.deal.payoff().map(|score| {
            make!(MatchResultT {
                score: Some(score.to_vec()),
                duration_ticks: self.deal.tick,
                level: 0,
                seed: 0,
            })
        })
    }
}

fn act(tick: u64, player: u8, discrete: i32) -> ActionT {
    make!(ActionT {
        tick: tick,
        player: player,
        discrete: discrete,
    })
}

#[test]
fn opponent_card_never_reaches_the_observation() {
    let game = Kuhn::new();
    for own in 0..3u8 {
        let mut seen = Vec::new();
        for other in (0..3u8).filter(|&c| c != own) {
            let deal = Deal {
                cards: [own, other],
                history: vec![PASS],
                tick: 1,
            };
            let mut obs = ObservationT::default();
            game.observe(&deal, 0, &mut obs);
            seen.push(obs);
        }
        assert_eq!(
            seen[0], seen[1],
            "player 0's view depends on the hidden card"
        );
    }
}

#[test]
fn state_from_observation_keeps_what_is_seen_and_samples_the_rest() {
    let game = Kuhn::new();
    let truth = Deal {
        cards: [0, 2],
        history: vec![PASS, BET],
        tick: 2,
    };
    let mut obs = ObservationT::default();
    game.observe(&truth, 1, &mut obs);
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    let mut sampled = [false; 3];
    for _ in 0..100 {
        let guess = game.state_from_observation(&obs, &mut rng);
        assert_eq!(guess.cards[1], 2);
        assert_ne!(guess.cards[0], 2);
        assert_eq!(guess.history, truth.history);
        assert_eq!(guess.tick, truth.tick);
        sampled[usize::from(guess.cards[0])] = true;
    }
    assert_eq!(
        sampled,
        [true, true, false],
        "both hidden cards must be sampled"
    );
}

#[test]
fn turns_refusals_and_payoffs_follow_kuhn_rules() {
    let mut game = Kuhn::new();
    game.reset(3, &[BodyParamsT::default(), BodyParamsT::default()])
        .unwrap();
    let before = game.live().clone();
    assert_eq!(
        game.apply(&act(0, 1, BET)),
        Err(AdapterError::BadAction("not this player's turn"))
    );
    assert_eq!(
        game.apply(&act(5, 0, BET)),
        Err(AdapterError::WrongTick {
            expected: 0,
            got: 5
        })
    );
    assert_eq!(game.live(), &before, "a refused action changed the game");

    game.deal.cards = [2, 0];
    game.apply(&act(0, 0, PASS)).unwrap();
    game.apply(&act(1, 1, BET)).unwrap();
    game.apply(&act(2, 0, BET)).unwrap();
    assert_eq!(game.result().unwrap().score, Some(vec![2, -2]));
    assert_eq!(game.apply(&act(3, 1, PASS)), Err(AdapterError::MatchOver));

    let start = Deal {
        cards: [0, 2],
        history: vec![],
        tick: 0,
    };
    let end = game.simulate(
        &start,
        &[act(0, 0, PASS), act(0, 1, BET), act(0, 0, PASS)],
        3,
    );
    assert_eq!(end.payoff(), Some([-1, 1]));
    assert_eq!(game.digest(&start), game.digest(&start.clone()));
    assert_ne!(game.digest(&start), game.digest(&end));
}

#[test]
fn signals_show_the_bet_not_the_intent() {
    let game = Kuhn::new();
    let deal = game.live().clone();
    let mut bluff = act(0, 0, BET);
    bluff.intent = 1;
    bluff.deceptive = true;
    let mut value = act(0, 0, BET);
    value.intent = 2;
    let (mut a, mut b) = (Vec::new(), Vec::new());
    game.signals(&deal, &bluff, &mut a);
    game.signals(&deal, &value, &mut b);
    assert_eq!(a, b);
    assert_eq!(a, vec![1.0]);
}
