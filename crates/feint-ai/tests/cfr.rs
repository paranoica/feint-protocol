use feint_ai::cfr::{
    CfrError, MAX_TABLE_BYTES, MAX_TABLE_ROWS, Solver, Table, TableError, info_key, regret_matching,
};
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, EventT, GameAdapter, GameSpecT,
    MatchResultT, ObservationT, PlayerId, TimeMode,
};

#[derive(Clone, Debug)]
struct Moves {
    made: Vec<i32>,
    tick: u64,
}

#[derive(Clone)]
struct Matrix {
    payoff: Vec<Vec<i64>>,
    columns: usize,
    mode: TimeMode,
    stuck: bool,
    twice: bool,
    moves: Moves,
}

impl Matrix {
    fn new(payoff: Vec<Vec<i64>>) -> Self {
        let columns = payoff[0].len();
        Self {
            payoff,
            columns,
            mode: TimeMode::TurnBased,
            stuck: false,
            twice: false,
            moves: Moves {
                made: vec![],
                tick: 0,
            },
        }
    }

    fn actions(&self) -> usize {
        self.payoff.len().max(self.columns)
    }

    fn turns(&self) -> usize {
        if self.twice { 3 } else { 2 }
    }

    fn mover(&self, made: usize) -> usize {
        usize::from(made + 1 == self.turns())
    }

    fn width(&self, player: usize) -> usize {
        if player == 0 {
            self.payoff.len()
        } else {
            self.columns
        }
    }
}

impl GameAdapter for Matrix {
    type State = Moves;

    fn spec(&self) -> GameSpecT {
        let mut spec = GameSpecT::default();
        spec.game_id = Some("matrix".into());
        spec.num_players = 2;
        spec.time_mode = self.mode;
        let mut space = ActionSpaceT::default();
        space.discrete = self.actions() as u32;
        spec.action_space = Some(Box::new(space));
        spec
    }

    fn reset(&mut self, _seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        if bodies.len() != 2 {
            return Err(AdapterError::BadBody("one entry per player"));
        }
        self.moves = Moves {
            made: vec![],
            tick: 0,
        };
        Ok(())
    }

    fn live(&self) -> &Moves {
        &self.moves
    }

    fn apply(&mut self, _action: &ActionT) -> Result<(), AdapterError> {
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {}

    fn observe(&self, state: &Moves, player: PlayerId, out: &mut ObservationT) {
        out.tick = state.tick;
        out.player = player;
        let n = state.made.len();
        let done = n >= self.turns();
        out.to_act = if done { -1 } else { self.mover(n) as i8 };
        let mut seen = vec![f32::from(player)];
        if self.twice && player == 0 {
            seen.push(state.made.first().map_or(-1.0, |&a| a as f32));
        }
        out.state = Some(seen);
        let mine = !done && usize::from(player) == self.mover(n);
        out.legal = Some(
            (0..self.actions())
                .map(|a| u8::from(mine && a < self.width(usize::from(player))))
                .collect(),
        );
        let score = if done {
            let last = &state.made[n - 2..];
            let v = self.payoff[last[0] as usize][last[1] as usize];
            vec![v, -v]
        } else {
            vec![0, 0]
        };
        out.score = Some(score);
    }

    fn state_from_observation(&self, _obs: &ObservationT, _rng: &mut ChaCha8Rng) -> Moves {
        self.moves.clone()
    }

    fn simulate(&self, state: &Moves, actions: &[ActionT], ticks: u32) -> Moves {
        let mut next = state.clone();
        if !self.stuck {
            next.made.extend(actions.iter().map(|a| a.discrete));
        }
        next.tick += u64::from(ticks);
        next
    }

    fn signals(&self, _state: &Moves, _action: &ActionT, out: &mut Vec<f32>) {
        out.clear();
    }

    fn digest(&self, _state: &Moves) -> u64 {
        0
    }

    fn result(&self) -> Option<MatchResultT> {
        None
    }
}

fn average(table: &Table, player: u8, width: usize, actions: usize) -> Vec<f64> {
    let legal: Vec<u8> = (0..actions).map(|a| u8::from(a < width)).collect();
    table
        .get(&info_key(player, &[f32::from(player)], &legal))
        .expect("the set was visited")
        .to_vec()
}

fn solved(payoff: Vec<Vec<i64>>, iterations: u64, seed: u64) -> Table {
    let mut solver = Solver::new(Matrix::new(payoff), seed).unwrap();
    solver.run(iterations).unwrap();
    solver.table()
}

#[test]
fn matching_pennies_with_a_skewed_payoff_converges_to_its_mixed_equilibrium() {
    let table = solved(vec![vec![3, -1], vec![-1, 1]], 200_000, 1);
    for player in 0..2 {
        let p = average(&table, player, 2, 2);
        assert!((p[0] - 1.0 / 3.0).abs() < 0.02, "player {player}: {p:?}");
        assert!((p[0] + p[1] - 1.0).abs() < 1e-9);
    }
}

#[test]
fn rock_paper_scissors_converges_to_a_third_each() {
    let table = solved(
        vec![vec![0, -1, 1], vec![1, 0, -1], vec![-1, 1, 0]],
        200_000,
        2,
    );
    for player in 0..2 {
        for p in average(&table, player, 3, 3) {
            assert!((p - 1.0 / 3.0).abs() < 0.02, "player {player}: {p}");
        }
    }
}

#[test]
fn a_dominated_action_gets_no_weight() {
    let table = solved(vec![vec![1, -1], vec![-1, 1], vec![-2, -2]], 100_000, 3);
    let p = average(&table, 0, 3, 3);
    assert!(p[2] < 0.01, "{p:?}");
    let q = average(&table, 1, 2, 3);
    assert_eq!(q[2], 0.0, "an illegal action keeps no weight");
}

#[test]
fn the_same_seed_gives_the_same_table_and_another_seed_another() {
    let pennies = || vec![vec![3, -1], vec![-1, 1]];
    let a = solved(pennies(), 5_000, 7).write();
    let b = solved(pennies(), 5_000, 7).write();
    let c = solved(pennies(), 5_000, 8).write();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn a_pinned_run_has_a_pinned_hash() {
    let text = solved(vec![vec![3, -1], vec![-1, 1]], 2_000, 11).write();
    assert!(text.starts_with("feint-table-v1 matrix 2\n"), "{text}");
    assert_eq!(fnv(&text), PINNED, "{text}");
}

const PINNED: u64 = 11_685_026_898_983_963_548;

#[test]
fn regret_matching_with_no_positive_regret_is_uniform_over_legal_actions() {
    let mut out = Vec::new();
    regret_matching(&[-1.0, 0.0, -3.0, 5.0], &[1, 1, 1, 0], &mut out);
    assert_eq!(out, vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 0.0]);
    regret_matching(&[0.0, 0.0], &[0, 1], &mut out);
    assert_eq!(out, vec![0.0, 1.0]);
}

#[test]
fn regret_matching_is_proportional_to_positive_regret() {
    let mut out = Vec::new();
    regret_matching(&[3.0, -2.0, 1.0, 9.0], &[1, 1, 1, 0], &mut out);
    assert_eq!(out, vec![0.75, 0.0, 0.25, 0.0]);
}

#[test]
fn a_table_after_one_iteration_reads_back() {
    let mut game = Matrix::new(vec![vec![3, -1], vec![-1, 1]]);
    game.twice = true;
    let mut solver = Solver::new(game, 5).unwrap();
    solver.run(1).unwrap();
    let table = solver.table();
    let text = table.write();
    let back = Table::read(&text, "matrix", 2).unwrap();
    assert_eq!(back.write(), text);
    assert_eq!(back.rows.len(), 4, "{text}");
    for first in [0.0, 1.0] {
        let p = back.get(&info_key(0, &[0.0, first], &[1, 1])).unwrap();
        assert!((p[0] + p[1] - 1.0).abs() < 1e-9, "{text}");
    }
}

#[test]
fn info_keys_carry_lengths_so_different_splits_differ() {
    let a = info_key(0, &[1.0], &[1, 1, 1, 1]);
    let b = info_key(0, &[1.0, f32::from_bits(0x0101_0101)], &[]);
    assert_ne!(a, b);
    assert_eq!(
        info_key(1, &[2.0], &[1, 0]),
        [
            vec![1],
            1u32.to_le_bytes().to_vec(),
            2.0f32.to_le_bytes().to_vec(),
            2u32.to_le_bytes().to_vec(),
            vec![1, 0]
        ]
        .concat()
    );
}

#[test]
fn a_game_that_is_not_turn_based_is_refused() {
    let mut game = Matrix::new(vec![vec![1, -1], vec![-1, 1]]);
    game.mode = TimeMode::Realtime;
    assert!(matches!(Solver::new(game, 1), Err(CfrError::NotTurnBased)));
}

#[test]
fn a_walk_that_never_ends_is_refused() {
    let mut game = Matrix::new(vec![vec![1, -1], vec![-1, 1]]);
    game.stuck = true;
    let mut solver = Solver::new(game, 1).unwrap();
    assert!(matches!(solver.run(1), Err(CfrError::TooDeep)));
}

fn table_text(rows: &[&str]) -> String {
    let mut text = String::from("feint-table-v1 matrix 2\n");
    for row in rows {
        text += row;
        text += "\n";
    }
    text
}

#[test]
fn the_table_reader_refuses_broken_files() {
    let good = table_text(&["00ff 0.25 0.75"]);
    let table = Table::read(&good, "matrix", 2).unwrap();
    assert_eq!(table.get(&[0x00, 0xff]), Some(&[0.25, 0.75][..]));
    let cases: [(String, TableError); 13] = [
        (table_text(&["00 0.5 0.5 0.25"]), TableError::Probability(2)),
        ("feint-table-v2 matrix 2\n".into(), TableError::Header),
        (good.replace("matrix", "other"), TableError::Game),
        (good.replace("matrix 2", "matrix 3"), TableError::Actions),
        (table_text(&["0f0 0.5 0.5"]), TableError::Key(2)),
        (table_text(&["0G 0.5 0.5"]), TableError::Key(2)),
        (table_text(&["0F 0.5 0.5"]), TableError::Key(2)),
        (table_text(&["00 NaN 0.5"]), TableError::Probability(2)),
        (table_text(&["00 -0.5 1.5"]), TableError::Probability(2)),
        (table_text(&["00 0.5"]), TableError::Probability(2)),
        (table_text(&["00 0.5 0.6"]), TableError::Sum(2)),
        (
            table_text(&["00 0.5 0.5", "00 0.25 0.75"]),
            TableError::Duplicate(3),
        ),
        (
            table_text(&[&format!("{} 0.5 0.5", "ab".repeat(4097))]),
            TableError::Key(2),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(
            Table::read(&text, "matrix", 2).err(),
            Some(want),
            "{text:.80}"
        );
    }
    let huge = "x".repeat(MAX_TABLE_BYTES + 1);
    assert_eq!(
        Table::read(&huge, "matrix", 2).err(),
        Some(TableError::TooLarge)
    );
    let mut many = String::from("feint-table-v1 matrix 2\n");
    for i in 0..=MAX_TABLE_ROWS {
        many += &format!("{i:08x} 1 0\n");
    }
    assert_eq!(
        Table::read(&many, "matrix", 2).err(),
        Some(TableError::TooManyRows)
    );
}
