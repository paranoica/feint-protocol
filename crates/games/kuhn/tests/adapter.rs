use feint_ai::{Controller, Opponent, RunHeader, replay_record, run_match};
use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId,
    TimeMode,
};
use kuhn::{BET, Deal, Kuhn, PASS};
use rand_chacha::rand_core::SeedableRng;

fn act(tick: u64, player: u8, discrete: i32) -> ActionT {
    let mut a = ActionT::default();
    a.tick = tick;
    a.player = player;
    a.discrete = discrete;
    a
}

fn bodies() -> [BodyParamsT; 2] {
    [BodyParamsT::default(), BodyParamsT::default()]
}

#[test]
fn the_spec_is_turn_based_with_two_discrete_actions() {
    let spec = Kuhn::new().spec();
    assert_eq!(spec.game_id.as_deref(), Some("kuhn-poker"));
    assert_eq!(spec.time_mode, TimeMode::TurnBased);
    assert_eq!(spec.num_players, 2);
    assert_eq!(spec.action_space.unwrap().discrete, 2);
    assert!(spec.tick_hz > 0);
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
    game.reset(3, &bodies()).unwrap();
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
    assert_eq!(
        game.apply(&act(0, 0, 2)),
        Err(AdapterError::BadAction("pass or bet only"))
    );
    assert_eq!(game.live(), &before, "a refused action changed the game");

    let mut game = Kuhn::from_deal(Deal {
        cards: [2, 0],
        history: vec![],
        tick: 0,
    });
    game.apply(&act(0, 0, PASS)).unwrap();
    game.apply(&act(0, 1, BET)).unwrap();
    game.apply(&act(0, 0, BET)).unwrap();
    assert_eq!(game.result().unwrap().score, Some(vec![2, -2]));
    assert_eq!(game.apply(&act(0, 1, PASS)), Err(AdapterError::MatchOver));

    let start = Deal {
        cards: [0, 2],
        history: vec![],
        tick: 0,
    };
    let end = game.simulate(
        &start,
        &[act(0, 0, PASS), act(0, 1, BET), act(0, 0, PASS)],
        0,
    );
    assert_eq!(end.payoff(), Some([-1, 1]));
    assert_eq!(end.tick, 0);
    assert_eq!(game.simulate(&start, &[], 4).tick, 4);
    assert_eq!(game.digest(&start), game.digest(&start.clone()));
    assert_ne!(game.digest(&start), game.digest(&end));
}

#[test]
fn every_terminal_history_pays_by_kuhn_rules() {
    let cases: [(&[i32], [u8; 2], [i64; 2]); 6] = [
        (&[PASS, PASS], [2, 1], [1, -1]),
        (&[PASS, PASS], [0, 1], [-1, 1]),
        (&[BET, PASS], [0, 2], [1, -1]),
        (&[PASS, BET, PASS], [2, 0], [-1, 1]),
        (&[BET, BET], [0, 1], [-2, 2]),
        (&[PASS, BET, BET], [1, 0], [2, -2]),
    ];
    for (history, cards, pay) in cases {
        let deal = Deal {
            cards,
            history: history.to_vec(),
            tick: 0,
        };
        assert_eq!(deal.payoff(), Some(pay), "{history:?} {cards:?}");
        assert_eq!(deal.to_act(), None);
    }
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

#[test]
fn the_tick_advances_with_time_not_with_actions() {
    let mut game = Kuhn::new();
    game.reset(1, &bodies()).unwrap();
    let mut events = Vec::new();
    game.advance(&mut events);
    game.advance(&mut events);
    assert_eq!(game.live().tick, 2);
    assert!(events.is_empty());
    assert_eq!(
        game.apply(&act(0, 0, PASS)),
        Err(AdapterError::WrongTick {
            expected: 2,
            got: 0
        })
    );
    game.apply(&act(2, 0, PASS)).unwrap();
    assert_eq!(game.live().tick, 2);
}

struct Slow {
    me: PlayerId,
    wait: u64,
    moves: Vec<i32>,
    since: u64,
}

impl Controller<Kuhn> for Slow {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.since = 0;
    }

    fn on_tick(&mut self, _adapter: &Kuhn, obs: &ObservationT) -> Option<ActionT> {
        if obs.to_act != self.me as i8 {
            self.since = 0;
            return None;
        }
        self.since += 1;
        if self.since <= self.wait || self.moves.is_empty() {
            return None;
        }
        self.since = 0;
        Some(act(obs.tick, self.me, self.moves.remove(0)))
    }
}

#[test]
fn idle_steps_replay_to_the_same_digest() {
    let mut game = Kuhn::new();
    let mut first = Slow {
        me: 0,
        wait: 7,
        moves: vec![PASS, BET],
        since: 0,
    };
    let mut second = Slow {
        me: 1,
        wait: 3,
        moves: vec![BET],
        since: 0,
    };
    let header = RunHeader {
        game_id: "kuhn-poker".into(),
        game_version: 1,
        seed: 11,
        bodies: bodies().to_vec(),
        levels: [0, 0],
        opponent: Opponent::HandMade,
    };
    let report = run_match(&mut game, [&mut first, &mut second], header, 1_000).unwrap();
    assert!(report.finished);
    assert_eq!(report.actions.len(), 3);
    assert!(report.ticks > 3, "the match had no idle steps");
    let mut again = Kuhn::new();
    assert_eq!(
        replay_record(&mut again, &report.record()),
        Ok(report.digest)
    );
}

#[test]
fn simulate_skips_an_action_out_of_turn() {
    let game = Kuhn::new();
    let start = Deal {
        cards: [0, 2],
        history: vec![PASS],
        tick: 3,
    };
    let next = game.simulate(&start, &[act(0, 0, BET), act(0, 1, BET)], 1);
    assert_eq!(next.history, vec![PASS, BET]);
    assert_eq!(next.tick, 4);
    let mut late = start.clone();
    late.tick = u64::MAX;
    assert_eq!(game.simulate(&late, &[], 2).tick, u64::MAX);
}

#[test]
fn a_hostile_observation_samples_a_real_card() {
    let game = Kuhn::new();
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    for card in [f32::INFINITY, 255.0, 1e9, -4.0, f32::NAN] {
        let mut obs = ObservationT::default();
        obs.player = 1;
        obs.state = Some(vec![card, 7.0, 1e9, -1.0]);
        let guess = game.state_from_observation(&obs, &mut rng);
        assert!(
            guess.cards.iter().all(|&c| c < 3),
            "{card}: {:?}",
            guess.cards
        );
        assert_ne!(guess.cards[0], guess.cards[1]);
    }
}

#[test]
fn a_stranger_or_the_end_of_time_never_panics() {
    let mut game = Kuhn::from_deal(Deal {
        cards: [0, 1],
        history: vec![],
        tick: u64::MAX,
    });
    let mut events = Vec::new();
    game.advance(&mut events);
    assert_eq!(game.live().tick, u64::MAX);
    let mut obs = ObservationT::default();
    game.observe(game.live(), 2, &mut obs);
    assert_eq!(obs.to_act, -1);
    assert!(obs.state.unwrap_or_default().is_empty());
}
