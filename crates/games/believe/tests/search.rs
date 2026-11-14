use believe::minds::{CallerStyle, LiarStyle, Person};
use believe::search::{Cards, LayModel, Oracle, PolicyPrior, choose, world};
use believe::trained::{CHECKPOINTS, policy};
use believe::{Counts, MAX_RANKS, Move, Params, Table, by_rank};
use feint_ai::search::{Budget, Game, Prior, Search, Setup};
use feint_protocol::{ChaCha8Rng, ObservationT};
use rand_chacha::rand_core::SeedableRng;

fn budget(iterations: u32, depth: u32) -> Budget {
    Budget {
        iterations,
        depth,
        top_k: 8,
        explore: 1.5,
    }
}

fn prior() -> PolicyPrior {
    PolicyPrior::new(policy(CHECKPOINTS[0].id).unwrap_or_else(|e| panic!("{e}")))
}

fn person() -> Person {
    Person::new(
        CallerStyle::Static { call: 0.3 },
        LiarStyle::Static { bluff: 0.6 },
    )
}

fn view(game: &Cards, table: &Table, me: u8) -> ObservationT {
    let mut obs = ObservationT::default();
    game.observe(table, me, &mut obs);
    obs
}

fn mid_round(seed: u64) -> Option<Table> {
    let game = Cards::default();
    let mut t = Table::deal(game.params(), seed);
    let p = person();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    for step in 0..40u64 {
        if t.outcome.is_some() {
            return None;
        }
        if t.to_act == 0 && t.batches > 0 && t.last_by == 1 && step > 3 {
            return Some(t);
        }
        let m = if t.to_act == 0 {
            let legal = game.legal(&t, 0);
            *legal.iter().rev().find(|m| matches!(m, Move::Lay { .. }))?
        } else {
            let odds = p.moves(game.params(), &t, 1);
            let _ = &mut rng;
            odds.iter()
                .rev()
                .find(|o| o.1 > 0.0 && matches!(o.0, Move::Lay { .. }))?
                .0
        };
        t.play(game.params(), t.to_act, &m).ok()?;
    }
    None
}

fn swap_hidden(params: &Params, t: &Table) -> Option<Table> {
    let theirs = t.hands[1];
    let aside = t.aside;
    let known = by_rank(params, theirs | t.laid[1]);
    for a in 0..64u32 {
        if theirs & (1 << a) == 0 {
            continue;
        }
        for b in 0..64u32 {
            if aside & (1 << b) == 0 || params.rank_of(a) == params.rank_of(b) {
                continue;
            }
            let mut u = t.clone();
            u.hands[1] = (theirs & !(1 << a)) | (1 << b);
            u.aside = (aside & !(1 << b)) | (1 << a);
            let held = by_rank(params, u.hands[1] | u.laid[1]);
            let fine = t.known[0].iter().zip(held).all(|(k, h)| *k <= h);
            if fine && known != held && u.validate(params).is_ok() {
                return Some(u);
            }
        }
    }
    None
}

#[test]
fn two_tables_that_look_the_same_to_the_bot_get_the_same_search() {
    let game = Cards::default();
    let mut checked = 0;
    for seed in 0..60 {
        let Some(t) = mid_round(seed) else {
            continue;
        };
        let Some(u) = swap_hidden(game.params(), &t) else {
            continue;
        };
        assert_ne!(t.hands[1], u.hands[1]);
        let (a, b) = (view(&game, &t, 0), view(&game, &u, 0));
        assert_eq!(a, b, "{seed}");
        let model = Oracle {
            person: person(),
            me: 0,
        };
        let run = |obs: &ObservationT| {
            let mut search = Search::new();
            let mut p = prior();
            let mut setup = Setup {
                game: &game,
                model: &model,
                prior: &mut p,
                me: 0,
                budget: budget(200, 2),
            };
            let mut sample = |r: &mut ChaCha8Rng| world(&game, obs, &model, r);
            search.run(
                &mut setup,
                &mut sample,
                &mut ChaCha8Rng::seed_from_u64(seed),
            );
            search.root()
        };
        assert_eq!(run(&a), run(&b), "{seed}");
        checked += 1;
    }
    assert!(checked >= 5, "{checked}");
}

#[test]
fn a_sampled_world_keeps_the_bots_view_and_follows_the_players_laying_rule() {
    let game = Cards::default();
    let params = game.params();
    let model = Oracle {
        person: person(),
        me: 0,
    };
    let mut checked = 0;
    for seed in 0..60 {
        let Some(t) = mid_round(seed) else {
            continue;
        };
        let obs = view(&game, &t, 0);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        for _ in 0..20 {
            let w = world(&game, &obs, &model, &mut rng);
            assert!(w.validate(params).is_ok(), "{seed}");
            assert_eq!(view(&game, &w, 0), obs, "{seed}");
            let claim = usize::from(w.claim.unwrap_or(0));
            let last = by_rank(params, w.last);
            let n: u8 = last.iter().sum();
            let lie = last.iter().enumerate().any(|(r, &c)| c > 0 && r != claim);
            if lie {
                assert_eq!(n, 1, "the drawn player lies one card at a time");
            }
            checked += 1;
        }
    }
    assert!(checked >= 100, "{checked}");
}

#[derive(Clone)]
struct BigLiar;

impl LayModel for BigLiar {
    fn lay_odds(&self, params: &Params, hand: &Counts, claim: usize, n: u32) -> Vec<(Counts, f64)> {
        let mut counts = [0u8; MAX_RANKS];
        let mut left = n;
        for (rank, &h) in hand.iter().enumerate().take(usize::from(params.ranks)) {
            if rank == claim || left == 0 {
                continue;
            }
            let k = h.min(left as u8);
            counts[rank] = k;
            left -= u32::from(k);
        }
        if left == 0 {
            vec![(counts, 1.0)]
        } else {
            Vec::new()
        }
    }
}

#[test]
fn a_player_who_lies_with_several_cards_gets_worlds_where_it_did() {
    let game = Cards::default();
    let params = game.params();
    let mut t = Table::deal(params, 3);
    t.hands = [0, 0];
    t.aside = params.deck_mask();
    let take = |t: &mut Table, ids: &[u32]| {
        ids.iter().fold(0u64, |m, &i| {
            t.aside &= !(1 << i);
            m | (1 << i)
        })
    };
    t.hands[0] = take(&mut t, &[0, 1, 8, 9, 16]);
    t.hands[1] = take(&mut t, &[4, 5, 12, 13, 17]);
    t.laid[1] = take(&mut t, &[18, 19]);
    t.last = t.laid[1];
    t.last_by = 1;
    t.claim = Some(0);
    t.batches = 1;
    t.to_act = 0;
    assert!(t.validate(params).is_ok());
    let obs = view(&game, &t, 0);
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let mut lies = 0;
    for _ in 0..50 {
        let w = world(&game, &obs, &BigLiar, &mut rng);
        let last = by_rank(params, w.last);
        if last[0] == 0 && last.iter().sum::<u8>() == 2 {
            lies += 1;
        }
    }
    assert!(lies >= 45, "{lies}");
}

#[test]
fn the_prior_is_the_policys_probability_of_each_legal_move() {
    let game = Cards::default();
    let t = Table::deal(game.params(), 11);
    let legal = game.legal(&t, t.to_act);
    let mut p = prior();
    let values = p.prior(&game, &t, t.to_act, &legal);
    assert_eq!(values.len(), legal.len());
    let sum: f64 = values.iter().sum();
    assert!((sum - 1.0).abs() < 1e-4, "{sum}");
    assert!(values.iter().all(|&v| (0.0..=1.0).contains(&v)));
}

#[test]
fn the_search_plays_a_legal_move_and_repeats_for_a_seed() {
    let game = Cards::default();
    for seed in 0..8 {
        let t = Table::deal(game.params(), seed);
        let me = t.to_act;
        let obs = view(&game, &t, me);
        let model = Oracle {
            person: person(),
            me,
        };
        let pick = |s| {
            choose(
                &game,
                &obs,
                &model,
                &mut prior(),
                budget(150, 2),
                &mut ChaCha8Rng::seed_from_u64(s),
            )
        };
        let m = pick(seed).unwrap_or_else(|| panic!("a move"));
        assert!(game.legal(&t, me).contains(&m));
        assert_eq!(pick(seed), Some(m));
    }
}

fn choose_k(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

#[test]
fn worlds_believe_a_lie_as_often_as_the_players_rule_and_the_deal_make_it() {
    let game = Cards::default();
    let params = game.params();
    let mut t = Table::empty(params);
    let take = |t: &mut Table, ids: &[u32]| ids.iter().fold(0u64, |m, &i| m | (1 << i));
    t.hands[0] = take(&mut t, &[0, 1, 4, 5, 8, 9, 12, 13]);
    t.laid[1] = 1 << 16;
    t.hands[1] = take(&mut t, &[2, 6, 10, 14, 17, 18]);
    t.aside = params.deck_mask() & !(t.hands[0] | t.hands[1] | t.laid[1]);
    t.last = t.laid[1];
    t.last_by = 1;
    t.claim = Some(0);
    t.batches = 1;
    t.to_act = 0;
    assert!(t.validate(params).is_ok());
    let (q, b) = (0.3, 0.6);
    let model = Oracle {
        person: Person::new(
            CallerStyle::Static { call: q },
            LiarStyle::Static { bluff: b },
        ),
        me: 0,
    };
    let unseen = 20 - 8;
    let claim_unseen = 2;
    let s = 7;
    let p0 = choose_k(claim_unseen, 0) * choose_k(unseen - claim_unseen, s) / choose_k(unseen, s);
    let p1 =
        choose_k(claim_unseen, 1) * choose_k(unseen - claim_unseen, s - 1) / choose_k(unseen, s);
    let want = p0 * b / (p0 * b + p1);
    let obs = view(&game, &t, 0);
    let mut rng = ChaCha8Rng::seed_from_u64(4);
    let n = 4_000;
    let mut lies = 0u32;
    for _ in 0..n {
        let w = world(&game, &obs, &model, &mut rng);
        if by_rank(params, w.last)[0] == 0 {
            lies += 1;
        }
    }
    let got = f64::from(lies) / f64::from(n);
    let se = (want * (1.0 - want) / f64::from(n)).sqrt();
    assert!((got - want).abs() < 4.0 * se, "{got} vs {want}");
}
