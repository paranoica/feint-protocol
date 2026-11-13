use believe::{Believe, Move, Params, Table, by_rank};
use feint_ai::{Controller, Opponent, RunHeader, replay_record, run_match};
use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId,
    TimeMode,
};
use rand_chacha::rand_core::SeedableRng;

fn cards(params: &Params, ranks: &[(u8, u8)]) -> u64 {
    cards_besides(params, 0, ranks)
}

fn cards_besides(params: &Params, used: u64, ranks: &[(u8, u8)]) -> u64 {
    let mut mask = 0u64;
    for &(rank, n) in ranks {
        let mut taken = 0;
        for copy in 0..params.copies {
            let id = rank * params.copies + copy;
            if (mask | used) & (1 << id) == 0 && taken < n {
                mask |= 1 << id;
                taken += 1;
            }
        }
        assert_eq!(taken, n);
    }
    mask
}

fn lay(claim: u8, pairs: &[(u8, u8)]) -> Move {
    let mut counts = [0u8; 8];
    for &(rank, n) in pairs {
        counts[usize::from(rank)] += n;
    }
    Move::Lay { claim, counts }
}

fn table(params: &Params, first: u64, second: u64) -> Table {
    assert_eq!(first & second, 0, "the hands share a card");
    let mut t = Table::empty(params);
    t.hands = [first, second];
    t.aside = params.deck_mask() & !(first | second);
    t
}

fn game_with(table: Table) -> Believe {
    Believe::from_table(Params::default(), table).unwrap()
}

fn action(game: &Believe, player: u8, m: &Move) -> ActionT {
    let mut a = ActionT::default();
    a.tick = game.live().tick;
    a.player = player;
    a.discrete = game.moves().index(m).unwrap();
    a
}

fn view(game: &Believe, player: u8) -> ObservationT {
    let mut obs = ObservationT::default();
    game.observe(game.live(), player, &mut obs);
    obs
}

fn bodies() -> [BodyParamsT; 2] {
    [BodyParamsT::default(), BodyParamsT::default()]
}

#[test]
fn the_spec_follows_the_params() {
    let game = Believe::new(Params::default()).unwrap();
    let spec = game.spec();
    assert_eq!(spec.game_id.as_deref(), Some("believe"));
    assert_eq!(spec.time_mode, TimeMode::TurnBased);
    assert_eq!(spec.tick_hz, 10);
    assert_eq!(spec.action_space.unwrap().discrete, 276);
    assert_eq!(spec.observation.unwrap().len(), 5 * 5 + 6);
    assert_eq!(spec.signals.unwrap().len(), 3);
    let obs = view(&game, 0);
    assert_eq!(obs.state.unwrap().len(), 31);
    assert_eq!(obs.legal.unwrap().len(), 276);
    assert!(
        Believe::new(Params {
            ranks: 1,
            ..Params::default()
        })
        .is_err()
    );
}

#[test]
fn the_view_is_the_same_whatever_real_cards_the_opponent_laid() {
    let params = Params::default();
    let a = cards(&params, &[(0, 3), (1, 2)]);
    let one = cards_besides(&params, a, &[(0, 1), (2, 1), (3, 2), (4, 1)]);
    let two = cards(&params, &[(2, 1), (3, 3), (4, 1)]);
    let mut first = game_with(table(&params, a, one));
    let mut second = game_with(table(&params, a, two));
    let script: [(u8, Move, Move); 8] = [
        (0, lay(0, &[(0, 1)]), lay(0, &[(0, 1)])),
        (1, lay(0, &[(0, 1)]), lay(0, &[(3, 1)])),
        (0, lay(0, &[(1, 1)]), lay(0, &[(1, 1)])),
        (1, lay(0, &[(4, 1)]), lay(0, &[(4, 1)])),
        (0, Move::Call, Move::Call),
        (0, lay(2, &[(0, 1)]), lay(2, &[(0, 1)])),
        (1, lay(2, &[(0, 2)]), lay(2, &[(3, 2)])),
        (0, lay(2, &[(0, 1)]), lay(2, &[(0, 1)])),
    ];
    assert_eq!(view(&first, 0), view(&second, 0));
    let mut events = Vec::new();
    for (step, (player, m1, m2)) in script.iter().enumerate() {
        first.apply(&action(&first, *player, m1)).unwrap();
        second.apply(&action(&second, *player, m2)).unwrap();
        first.advance(&mut events);
        second.advance(&mut events);
        assert_eq!(view(&first, 0), view(&second, 0), "after step {step}");
    }
    assert_ne!(first.live(), second.live());
    assert_eq!(first.live().outcome, None);
}

fn ids(list: &[u32]) -> u64 {
    list.iter().fold(0, |m, &id| m | (1 << id))
}

#[test]
fn a_turned_card_does_not_tell_which_copy_was_laid() {
    let params = Params::default();
    let a = ids(&[2, 4, 5, 16, 17]);
    let mut first = game_with(table(&params, a, ids(&[0, 8, 9, 12])));
    let mut second = game_with(table(&params, a, ids(&[13, 8, 9, 12])));
    let script: [(u8, Move); 5] = [
        (0, lay(0, &[(0, 1)])),
        (1, Move::Call),
        (0, lay(1, &[(1, 1)])),
        (1, lay(1, &[(0, 1)])),
        (0, Move::Call),
    ];
    let mut events = Vec::new();
    for (step, (player, m)) in script.iter().enumerate() {
        first.apply(&action(&first, *player, m)).unwrap();
        second.apply(&action(&second, *player, m)).unwrap();
        first.advance(&mut events);
        second.advance(&mut events);
        assert_eq!(view(&first, 0), view(&second, 0), "after step {step}");
    }
    assert_ne!(
        first.live().hands[1].count_ones(),
        0,
        "the match must still be running"
    );
}

#[test]
fn simulate_skips_an_action_out_of_turn() {
    let params = Params::default();
    let game = game_with(table(
        &params,
        cards(&params, &[(0, 2), (1, 1)]),
        cards(&params, &[(2, 2)]),
    ));
    let mut state = game.live().clone();
    state.play(&params, 0, &lay(0, &[(0, 1)])).unwrap();
    let mine = action(&game, 0, &lay(0, &[(0, 1)]));
    let theirs = action(&game, 1, &Move::Call);
    let next = game.simulate(&state, &[mine, theirs.clone()], 1);
    let direct = game.simulate(&state, &[theirs], 1);
    assert_eq!(next, direct);
    assert_eq!(next.batches, 0, "the call was applied");
    assert_eq!(
        game.simulate(&next, &[], u32::MAX).tick,
        1 + u64::from(u32::MAX)
    );
    let mut late = next.clone();
    late.tick = u64::MAX;
    assert_eq!(game.simulate(&late, &[], 5).tick, u64::MAX);
}

#[test]
fn a_sampled_state_calls_the_viewers_last_batch_as_it_was() {
    let params = Params::default();
    let mut game = game_with(table(
        &params,
        cards(&params, &[(0, 1), (1, 1), (4, 1)]),
        cards(&params, &[(2, 2), (3, 2)]),
    ));
    let mut events = Vec::new();
    for (player, m) in [
        (0u8, lay(0, &[(1, 1)])),
        (1, lay(0, &[(2, 1)])),
        (0, lay(0, &[(0, 1)])),
    ] {
        game.apply(&action(&game, player, &m)).unwrap();
        game.advance(&mut events);
    }
    let obs = view(&game, 0);
    let call = action(&game, 1, &Move::Call);
    let truth = game.simulate(game.live(), std::slice::from_ref(&call), 0);
    let mut rng = ChaCha8Rng::seed_from_u64(4);
    for _ in 0..40 {
        let guess = game.state_from_observation(&obs, &mut rng);
        let after = game.simulate(&guess, std::slice::from_ref(&call), 0);
        assert_eq!(
            by_rank(&params, after.hands[0]),
            by_rank(&params, truth.hands[0]),
            "the honest nine was called as a lie in a sampled state"
        );
        assert_eq!(after.outcome, truth.outcome);
    }
}

#[test]
fn an_ended_match_samples_back_to_the_same_view() {
    let params = Params::default();
    let mut game = game_with(table(
        &params,
        cards(&params, &[(0, 2)]),
        cards(&params, &[(2, 2)]),
    ));
    game.apply(&action(&game, 0, &lay(0, &[(0, 2)]))).unwrap();
    game.apply(&action(&game, 1, &Move::Call)).unwrap();
    assert!(game.result().is_some());
    for viewer in 0..2u8 {
        let obs = view(&game, viewer);
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let guess = game.state_from_observation(&obs, &mut rng);
        let mut again = ObservationT::default();
        game.observe(&guess, viewer, &mut again);
        assert_eq!(again, obs, "viewer {viewer}");
    }
}

#[test]
fn a_table_with_impossible_cards_is_refused() {
    let params = Params::default();
    let a = cards(&params, &[(0, 2)]);
    let b = cards(&params, &[(2, 2)]);
    let good = table(&params, a, b);
    assert!(Believe::from_table(params.clone(), good.clone()).is_ok());
    let mut outside = good.clone();
    outside.hands[0] |= 1 << 63;
    let mut shared = good.clone();
    shared.hands[1] |= a;
    let mut aside = good.clone();
    aside.aside |= a;
    let mut stray = good.clone();
    stray.last = 1 << 19;
    let mut player = good;
    player.to_act = 2;
    for (name, bad) in [
        ("outside", outside),
        ("shared", shared),
        ("aside", aside),
        ("stray", stray),
        ("player", player),
    ] {
        assert!(Believe::from_table(params.clone(), bad).is_err(), "{name}");
    }
}

#[test]
fn aside_cards_never_reach_the_observation() {
    let params = Params::default();
    let a = cards(&params, &[(0, 2), (1, 2)]);
    let one = cards(&params, &[(2, 2), (3, 2)]);
    let mut t1 = table(&params, a, one);
    let mut t2 = t1.clone();
    let opponent_card = 1u64 << (2 * params.copies);
    let aside_card = 1u64 << t1.aside.trailing_zeros();
    t2.hands[1] = (t2.hands[1] & !opponent_card) | aside_card;
    t2.aside = (t2.aside & !aside_card) | opponent_card;
    t1.to_act = 1;
    t2.to_act = 1;
    let g1 = game_with(t1);
    let g2 = game_with(t2);
    assert_ne!(g1.live(), g2.live());
    assert_eq!(view(&g1, 0), view(&g2, 0));
}

#[test]
fn signals_carry_the_claim_and_count_only() {
    let params = Params::default();
    let game = game_with(table(
        &params,
        cards(&params, &[(0, 2), (1, 2)]),
        cards(&params, &[(2, 2)]),
    ));
    let (mut honest, mut lie, mut call) = (Vec::new(), Vec::new(), Vec::new());
    game.signals(
        game.live(),
        &action(&game, 0, &lay(0, &[(0, 2)])),
        &mut honest,
    );
    game.signals(
        game.live(),
        &action(&game, 0, &lay(0, &[(0, 1), (1, 1)])),
        &mut lie,
    );
    let mut a = ActionT::default();
    a.discrete = 0;
    game.signals(game.live(), &a, &mut call);
    assert_eq!(honest, lie);
    assert_eq!(honest, vec![0.0, 0.0, 2.0]);
    assert_eq!(call, vec![1.0, -1.0, 0.0]);
}

#[test]
fn refused_actions_leave_the_state_unchanged() {
    let params = Params::default();
    let mut game = game_with(table(
        &params,
        cards(&params, &[(0, 2)]),
        cards(&params, &[(2, 2)]),
    ));
    let before = game.live().clone();
    let mut wrong_tick = action(&game, 0, &lay(0, &[(0, 1)]));
    wrong_tick.tick = 3;
    assert_eq!(
        game.apply(&wrong_tick),
        Err(AdapterError::WrongTick {
            expected: 0,
            got: 3
        })
    );
    let mut stranger = action(&game, 0, &lay(0, &[(0, 1)]));
    stranger.player = 2;
    assert_eq!(game.apply(&stranger), Err(AdapterError::BadPlayer(2)));
    let mut unknown = action(&game, 0, &lay(0, &[(0, 1)]));
    unknown.discrete = 276;
    assert_eq!(
        game.apply(&unknown),
        Err(AdapterError::BadAction("no such action"))
    );
    assert_eq!(
        game.apply(&action(&game, 1, &lay(0, &[(2, 1)]))),
        Err(AdapterError::BadAction("not this player's turn"))
    );
    assert_eq!(
        game.apply(&action(&game, 0, &lay(0, &[(2, 1)]))),
        Err(AdapterError::BadAction("cards not in hand"))
    );
    assert_eq!(game.live(), &before);
    game.apply(&action(&game, 0, &lay(0, &[(0, 2)]))).unwrap();
    game.apply(&action(&game, 1, &Move::Call)).unwrap();
    let result = game.result().unwrap();
    assert_eq!(result.score, Some(vec![1, 0]));
    let ended = game.live().clone();
    assert_eq!(
        game.apply(&action(&game, 0, &lay(0, &[(2, 1)]))),
        Err(AdapterError::MatchOver)
    );
    assert_eq!(game.live(), &ended);
    assert_eq!(view(&game, 0).to_act, -1);
    assert_eq!(view(&game, 1).score, Some(vec![1, 0]));
}

#[test]
fn state_from_observation_keeps_what_is_seen_and_samples_the_rest() {
    let params = Params::default();
    let mut game = Believe::new(params.clone()).unwrap();
    game.reset(5, &bodies()).unwrap();
    let mut events = Vec::new();
    for step in 0..9 {
        let me = game.live().to_act;
        let legal = view(&game, me).legal.unwrap();
        let pick = if step == 4 && legal[0] == 1 {
            0
        } else {
            1 + legal.iter().skip(1).position(|&l| l == 1).unwrap()
        };
        let mut a = ActionT::default();
        a.tick = game.live().tick;
        a.player = me;
        a.discrete = pick as i32;
        game.apply(&a).unwrap();
        game.advance(&mut events);
    }
    assert!(game.result().is_none());
    assert!(
        game.live().known.iter().flatten().any(|&c| c > 0),
        "the call should have shown someone a card the other holds"
    );
    for viewer in 0..2u8 {
        let obs = view(&game, viewer);
        let mut rng = ChaCha8Rng::seed_from_u64(9);
        let mut hands = std::collections::BTreeSet::new();
        for _ in 0..50 {
            let guess = game.state_from_observation(&obs, &mut rng);
            let mut again = ObservationT::default();
            game.observe(&guess, viewer, &mut again);
            assert_eq!(again, obs, "viewer {viewer}");
            assert_eq!(
                by_rank(&params, guess.hands[usize::from(viewer)]),
                by_rank(&params, game.live().hands[usize::from(viewer)])
            );
            assert_eq!(
                guess.hands[0].count_ones()
                    + guess.hands[1].count_ones()
                    + (guess.laid[0] | guess.laid[1]).count_ones()
                    + guess.aside.count_ones(),
                params.deck()
            );
            assert_eq!(
                guess.hands[0] & guess.hands[1] | (guess.hands[0] | guess.hands[1]) & guess.aside,
                0
            );
            hands.insert(guess.hands[usize::from(1 - viewer)]);
        }
        assert!(hands.len() > 1, "the hidden hand was never sampled");
    }
}

struct Slow {
    me: PlayerId,
    wait: u64,
    since: u64,
}

impl Controller<Believe> for Slow {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.since = 0;
    }

    fn on_tick(&mut self, _adapter: &Believe, obs: &ObservationT) -> Option<ActionT> {
        if obs.to_act != self.me as i8 {
            return None;
        }
        self.since += 1;
        if self.since <= self.wait {
            return None;
        }
        self.since = 0;
        let legal = obs.legal.as_deref().unwrap_or_default();
        let pick = legal.iter().rposition(|&l| l == 1)?;
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        a.discrete = pick as i32;
        Some(a)
    }
}

#[test]
fn idle_steps_replay_to_the_same_digest() {
    let mut game = Believe::new(Params::default()).unwrap();
    let mut first = Slow {
        me: 0,
        wait: 4,
        since: 0,
    };
    let mut second = Slow {
        me: 1,
        wait: 2,
        since: 0,
    };
    let header = RunHeader {
        game_id: "believe".into(),
        game_version: believe::GAME_VERSION,
        seed: 21,
        bodies: bodies().to_vec(),
        levels: [0, 0],
        opponent: Opponent::HandMade,
    };
    let report = run_match(&mut game, [&mut first, &mut second], header, 100_000).unwrap();
    assert!(report.finished);
    assert!(report.ticks > report.actions.len() as u64 * 2);
    let mut again = Believe::new(Params::default()).unwrap();
    assert_eq!(
        replay_record(&mut again, &report.record()),
        Ok(report.digest)
    );
}

#[test]
fn a_full_64_card_deck_deals_observes_and_samples() {
    let params = Params {
        ranks: 8,
        copies: 8,
        hand: 20,
        ..Params::default()
    };
    let mut game = Believe::new(params.clone()).unwrap();
    game.reset(3, &bodies()).unwrap();
    assert_eq!(game.live().aside.count_ones(), 24);
    let obs = view(&game, 0);
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let guess = game.state_from_observation(&obs, &mut rng);
    let mut again = ObservationT::default();
    game.observe(&guess, 0, &mut again);
    assert_eq!(again, obs);
}

#[test]
fn a_hostile_observation_samples_without_panicking() {
    let game = Believe::new(Params::default()).unwrap();
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    for fill in [f32::NAN, -1.0, 1e30, f32::INFINITY, 4.0e9] {
        let mut obs = ObservationT::default();
        obs.player = 7;
        obs.to_act = 5;
        obs.state = Some(vec![fill; 31]);
        obs.score = Some(vec![3, 4, 5]);
        let guess = game.state_from_observation(&obs, &mut rng);
        let mut seen = ObservationT::default();
        game.observe(&guess, 0, &mut seen);
        for index in 0..276 {
            let mut a = ActionT::default();
            a.discrete = index;
            a.player = guess.to_act;
            let _ = game.simulate(&guess, &[a], 1);
        }
        let mut bot = believe::Scripted::new(believe::Style::Counter);
        bot.new_match(&game.spec(), 0, 1);
        obs.player = 0;
        obs.to_act = 0;
        let _ = bot.on_tick(&game, &obs);
        obs.state = Some(vec![fill; 3]);
        let _ = game.state_from_observation(&obs, &mut rng);
        let _ = bot.on_tick(&game, &obs);
    }
}
