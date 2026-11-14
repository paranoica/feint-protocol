use feint_ai::search::{Budget, Game, Model, Prior, Search, Setup, Uniform, decide};
use feint_protocol::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

#[derive(Clone, Debug, PartialEq)]
struct Trail {
    moves: Vec<u8>,
    hidden: u8,
}

struct Trap;

const OPPONENT_ONE: f64 = 0.7;

impl Trap {
    fn terminal(m: &[u8]) -> f64 {
        match m {
            [0, _, _] => 0.2,
            [1, 1, 1] => 0.9,
            [1, 1, 0] => 0.1,
            [1, 0, _] => 0.5,
            _ => 0.0,
        }
    }
}

impl Game for Trap {
    type World = Trail;
    type Action = u8;

    fn to_act(&self, w: &Trail) -> Option<u8> {
        match w.moves.len() {
            0 | 2 => Some(0),
            1 => Some(1),
            _ => None,
        }
    }

    fn legal(&self, _w: &Trail, _player: u8) -> Vec<u8> {
        vec![0, 1]
    }

    fn apply(&self, w: &mut Trail, _player: u8, a: u8) {
        w.moves.push(a);
    }

    fn value(&self, w: &Trail, _me: u8) -> f64 {
        match w.moves.as_slice() {
            m if m.len() == 3 => Self::terminal(m),
            [0, ..] => 0.6,
            [1, ..] => 0.4,
            _ => 0.5,
        }
    }

    fn public(&self, _w: &Trail, _player: u8, a: u8) -> u64 {
        u64::from(a)
    }
}

#[derive(Clone)]
struct Coin;

impl Model<Trap> for Coin {
    fn policy(&self, _g: &Trap, _w: &Trail, _player: u8) -> Vec<(u8, f64)> {
        vec![(0, 1.0 - OPPONENT_ONE), (1, OPPONENT_ONE)]
    }

    fn observe(&mut self, _g: &Trap, _before: &Trail, _player: u8, _a: u8) {}
}

fn budget(iterations: u32, depth: u32) -> Budget {
    Budget {
        iterations,
        depth,
        top_k: 8,
        explore: 1.5,
    }
}

fn fresh(_rng: &mut ChaCha8Rng) -> Trail {
    Trail {
        moves: Vec::new(),
        hidden: 0,
    }
}

fn run(iterations: u32, depth: u32, seed: u64) -> Search<u8> {
    let mut search = Search::new();
    let mut prior = Uniform;
    let mut setup = Setup {
        game: &Trap,
        model: &Coin,
        prior: &mut prior,
        me: 0,
        budget: budget(iterations, depth),
    };
    search.run(&mut setup, &mut fresh, &mut ChaCha8Rng::seed_from_u64(seed));
    search
}

#[test]
fn a_known_trap_is_found_at_depth_two_and_missed_at_depth_one() {
    assert_eq!(run(2_000, 1, 1).best(), Some(0));
    assert_eq!(run(2_000, 2, 1).best(), Some(1));
}

#[test]
fn the_values_converge_to_expectimax() {
    let deep = OPPONENT_ONE * 0.9 + (1.0 - OPPONENT_ONE) * 0.5;
    let root = run(40_000, 2, 3).root();
    let one = root.iter().find(|e| e.action == 1).map(|e| e.mean);
    let zero = root.iter().find(|e| e.action == 0).map(|e| e.mean);
    assert!(
        one.is_some_and(|v| (v - deep).abs() < 0.03),
        "{root:?} vs {deep}"
    );
    assert!(zero.is_some_and(|v| (v - 0.2).abs() < 1e-9), "{root:?}");
    let shallow = run(4_000, 1, 3).root();
    for e in &shallow {
        let want = if e.action == 0 { 0.6 } else { 0.4 };
        assert!((e.mean - want).abs() < 1e-9 || e.visits == 0, "{shallow:?}");
    }
}

#[test]
fn a_seed_repeats_the_search() {
    assert_eq!(run(500, 2, 9).root(), run(500, 2, 9).root());
}

struct Lean(Vec<f64>);

impl Prior<Trap> for Lean {
    fn prior(&mut self, _g: &Trap, _w: &Trail, _me: u8, legal: &[u8]) -> Vec<f64> {
        legal.iter().map(|&a| self.0[usize::from(a)]).collect()
    }
}

#[test]
fn the_prior_is_kept_to_the_top_k_and_renormalised() {
    let mut search = Search::new();
    let mut prior = Lean(vec![0.3, 0.1]);
    let mut b = budget(10, 2);
    b.top_k = 1;
    let mut setup = Setup {
        game: &Trap,
        model: &Coin,
        prior: &mut prior,
        me: 0,
        budget: b,
    };
    search.run(&mut setup, &mut fresh, &mut ChaCha8Rng::seed_from_u64(1));
    let root = search.root();
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].action, 0);
    assert!((root[0].prior - 1.0).abs() < 1e-12);
}

struct Hidden;

impl Game for Hidden {
    type World = Trail;
    type Action = u8;

    fn to_act(&self, w: &Trail) -> Option<u8> {
        (w.moves.is_empty()).then_some(0)
    }

    fn legal(&self, _w: &Trail, _player: u8) -> Vec<u8> {
        vec![0, 1]
    }

    fn apply(&self, w: &mut Trail, _player: u8, a: u8) {
        w.moves.push(a);
    }

    fn value(&self, w: &Trail, _me: u8) -> f64 {
        match w.moves.first() {
            Some(&a) if a == w.hidden => 1.0,
            Some(_) => 0.0,
            None => 0.5,
        }
    }

    fn public(&self, _w: &Trail, _player: u8, a: u8) -> u64 {
        u64::from(a)
    }
}

#[derive(Clone)]
struct Nobody;

impl Model<Hidden> for Nobody {
    fn policy(&self, _g: &Hidden, _w: &Trail, _p: u8) -> Vec<(u8, f64)> {
        Vec::new()
    }

    fn observe(&mut self, _g: &Hidden, _b: &Trail, _p: u8, _a: u8) {}
}

#[test]
fn the_search_weighs_hidden_states_by_how_often_the_sampler_draws_them() {
    let mut prior = Uniform;
    let mut setup = Setup {
        game: &Hidden,
        model: &Nobody,
        prior: &mut prior,
        me: 0,
        budget: budget(4_000, 1),
    };
    let mut mostly_one = |rng: &mut ChaCha8Rng| {
        use rand_chacha::rand_core::RngCore;
        Trail {
            moves: Vec::new(),
            hidden: u8::from(rng.next_u32() % 10 < 7),
        }
    };
    let mut rng = ChaCha8Rng::seed_from_u64(5);
    assert_eq!(decide(&mut setup, &mut mostly_one, &mut rng), Some(1));
}

struct Narrow {
    illegal: std::cell::Cell<u32>,
}

impl Game for Narrow {
    type World = Trail;
    type Action = u8;

    fn to_act(&self, w: &Trail) -> Option<u8> {
        (w.moves.len() < 2).then_some(0)
    }

    fn legal(&self, w: &Trail, _player: u8) -> Vec<u8> {
        if w.hidden == 1 { vec![0, 1] } else { vec![0] }
    }

    fn apply(&self, w: &mut Trail, player: u8, a: u8) {
        if !self.legal(w, player).contains(&a) {
            self.illegal.set(self.illegal.get() + 1);
        }
        w.moves.push(a);
    }

    fn value(&self, w: &Trail, _me: u8) -> f64 {
        f64::from(w.moves.iter().map(|&m| u32::from(m)).sum::<u32>()) / 2.0
    }

    fn public(&self, _w: &Trail, _player: u8, a: u8) -> u64 {
        u64::from(a)
    }
}

impl Model<Narrow> for Nobody {
    fn policy(&self, _g: &Narrow, _w: &Trail, _p: u8) -> Vec<(u8, f64)> {
        Vec::new()
    }

    fn observe(&mut self, _g: &Narrow, _b: &Trail, _p: u8, _a: u8) {}
}

#[test]
fn an_action_is_only_tried_in_worlds_where_it_is_legal() {
    let game = Narrow {
        illegal: std::cell::Cell::new(0),
    };
    let mut prior = Uniform;
    let mut setup = Setup {
        game: &game,
        model: &Nobody,
        prior: &mut prior,
        me: 0,
        budget: budget(2_000, 2),
    };
    let mut alternate = |rng: &mut ChaCha8Rng| {
        use rand_chacha::rand_core::RngCore;
        Trail {
            moves: Vec::new(),
            hidden: u8::from(rng.next_u32().is_multiple_of(2)),
        }
    };
    let mut search = Search::new();
    search.run(
        &mut setup,
        &mut alternate,
        &mut ChaCha8Rng::seed_from_u64(2),
    );
    assert_eq!(game.illegal.get(), 0);
    let root = search.root();
    let one = root.iter().find(|e| e.action == 1).map_or(0, |e| e.visits);
    assert!(one > 0 && one < 2_000, "{root:?}");
}
