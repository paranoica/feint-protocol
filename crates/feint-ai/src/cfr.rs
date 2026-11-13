use std::collections::BTreeMap;

use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, ChaCha8Rng, GameAdapter, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

pub const TABLE_FORMAT: &str = "feint-table-v1";
pub const MAX_DEPTH: usize = 512;
pub const MAX_TABLE_BYTES: usize = 64 << 20;
pub const MAX_TABLE_ROWS: usize = 1_000_000;
pub const MAX_KEY_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfrError {
    NotTurnBased,
    NoActions,
    GameId,
    NoScore,
    Adapter(AdapterError),
    NoLegalAction,
    LegalLength { got: usize, want: usize },
    TooDeep,
}

impl std::fmt::Display for CfrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotTurnBased => write!(f, "the game is not turn-based"),
            Self::NoActions => write!(f, "the game has no discrete actions"),
            Self::GameId => write!(f, "the game id is empty or has whitespace"),
            Self::NoScore => write!(f, "a finished match has no score for every player"),
            Self::Adapter(e) => write!(f, "{e}"),
            Self::NoLegalAction => write!(f, "a player is to act with no legal action"),
            Self::LegalLength { got, want } => {
                write!(
                    f,
                    "a legal mask has {got} entries, the game has {want} actions"
                )
            }
            Self::TooDeep => write!(f, "a match did not end within {MAX_DEPTH} moves"),
        }
    }
}

impl std::error::Error for CfrError {}

#[must_use]
pub fn info_key(player: PlayerId, state: &[f32], legal: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(9 + 4 * state.len() + legal.len());
    key.push(player);
    key.extend_from_slice(&(state.len() as u32).to_le_bytes());
    for v in state {
        let canonical = if v.is_nan() { f32::NAN } else { v + 0.0 };
        key.extend_from_slice(&canonical.to_le_bytes());
    }
    key.extend_from_slice(&(legal.len() as u32).to_le_bytes());
    key.extend_from_slice(legal);
    key
}

fn uniform(legal: &[u8], out: &mut Vec<f64>) {
    let count = legal.iter().filter(|&&l| l == 1).count();
    out.extend(legal.iter().map(|&l| {
        if l == 1 && count > 0 {
            1.0 / count as f64
        } else {
            0.0
        }
    }));
}

pub fn regret_matching(regret: &[f64], legal: &[u8], out: &mut Vec<f64>) {
    out.clear();
    let positive = |i: usize| {
        if legal.get(i) == Some(&1) && regret[i] > 0.0 {
            regret[i]
        } else {
            0.0
        }
    };
    let total: f64 = (0..regret.len()).map(positive).sum();
    if total > 0.0 {
        out.extend((0..regret.len()).map(|i| positive(i) / total));
    } else {
        uniform(&legal[..regret.len().min(legal.len())], out);
        out.resize(regret.len(), 0.0);
    }
}

#[derive(Clone, Debug)]
struct Node {
    legal: Vec<u8>,
    regret: Vec<f64>,
    average: Vec<f64>,
}

pub struct Solver<A: GameAdapter> {
    game: A,
    actions: usize,
    players: usize,
    bodies: Vec<BodyParamsT>,
    nodes: BTreeMap<Vec<u8>, Node>,
    rng: ChaCha8Rng,
    seen: ObservationT,
}

impl<A: GameAdapter> Solver<A> {
    pub fn new(game: A, seed: u64) -> Result<Self, CfrError> {
        let spec = game.spec();
        if spec.time_mode != TimeMode::TurnBased {
            return Err(CfrError::NotTurnBased);
        }
        let actions = spec
            .action_space
            .as_ref()
            .and_then(|s| usize::try_from(s.discrete).ok())
            .filter(|&n| n > 0)
            .ok_or(CfrError::NoActions)?;
        let id = spec.game_id.as_deref().unwrap_or_default();
        if id.is_empty() || id.contains(char::is_whitespace) {
            return Err(CfrError::GameId);
        }
        let players = usize::from(spec.num_players);
        Ok(Self {
            game,
            actions,
            players,
            bodies: vec![BodyParamsT::default(); players],
            nodes: BTreeMap::new(),
            rng: ChaCha8Rng::seed_from_u64(seed),
            seen: ObservationT::default(),
        })
    }

    pub fn run(&mut self, iterations: u64) -> Result<(), CfrError> {
        for _ in 0..iterations {
            let seed = self.rng.next_u64();
            self.game
                .reset(seed, &self.bodies)
                .map_err(CfrError::Adapter)?;
            let root = self.game.live().clone();
            for traverser in 0..self.players {
                self.walk(&root, traverser, 0)?;
            }
        }
        Ok(())
    }

    fn mover(&mut self, state: &A::State) -> Option<PlayerId> {
        self.game.observe(state, 0, &mut self.seen);
        PlayerId::try_from(self.seen.to_act).ok()
    }

    fn child(&self, state: &A::State, tick: u64, player: PlayerId, action: usize) -> A::State {
        let mut act = ActionT::default();
        act.tick = tick;
        act.player = player;
        act.discrete = action as i32;
        self.game.simulate(state, &[act], 1)
    }

    fn sample(&mut self, sigma: &[f64]) -> usize {
        let u = f64::from(self.rng.next_u32()) / 4_294_967_296.0;
        let mut cumulative = 0.0;
        let mut last = 0;
        for (a, &p) in sigma.iter().enumerate() {
            if p > 0.0 {
                cumulative += p;
                last = a;
                if u < cumulative {
                    return a;
                }
            }
        }
        last
    }

    fn walk(&mut self, state: &A::State, traverser: usize, depth: usize) -> Result<f64, CfrError> {
        if depth > MAX_DEPTH {
            return Err(CfrError::TooDeep);
        }
        let Some(player) = self.mover(state) else {
            let me = PlayerId::try_from(traverser).unwrap_or(PlayerId::MAX);
            self.game.observe(state, me, &mut self.seen);
            let score = self.seen.score.as_deref().unwrap_or_default();
            if score.len() < self.players {
                return Err(CfrError::NoScore);
            }
            return Ok(score[traverser] as f64);
        };
        self.game.observe(state, player, &mut self.seen);
        let tick = self.seen.tick;
        let legal = self.seen.legal.clone().unwrap_or_default();
        if legal.len() != self.actions {
            return Err(CfrError::LegalLength {
                got: legal.len(),
                want: self.actions,
            });
        }
        if !legal.contains(&1) {
            return Err(CfrError::NoLegalAction);
        }
        let key = info_key(
            player,
            self.seen.state.as_deref().unwrap_or_default(),
            &legal,
        );
        let actions = self.actions;
        let node = self.nodes.entry(key.clone()).or_insert_with(|| Node {
            legal: legal.clone(),
            regret: vec![0.0; actions],
            average: vec![0.0; actions],
        });
        let mut sigma = Vec::with_capacity(actions);
        regret_matching(&node.regret, &legal, &mut sigma);
        if usize::from(player) != traverser {
            for (avg, s) in node.average.iter_mut().zip(&sigma) {
                *avg += s;
            }
            let a = self.sample(&sigma);
            let next = self.child(state, tick, player, a);
            return self.walk(&next, traverser, depth + 1);
        }
        let mut values = vec![0.0; actions];
        let mut expected = 0.0;
        for a in (0..actions).filter(|&a| legal[a] == 1) {
            let next = self.child(state, tick, player, a);
            values[a] = self.walk(&next, traverser, depth + 1)?;
            expected += sigma[a] * values[a];
        }
        if let Some(node) = self.nodes.get_mut(&key) {
            for a in (0..actions).filter(|&a| legal[a] == 1) {
                node.regret[a] += values[a] - expected;
            }
        }
        Ok(expected)
    }

    #[must_use]
    pub fn table(&self) -> Table {
        let mut rows = BTreeMap::new();
        for (key, node) in &self.nodes {
            let total: f64 = node.average.iter().sum();
            let mut p = Vec::with_capacity(self.actions);
            if total > 0.0 {
                p.extend(node.average.iter().map(|v| v / total));
            } else {
                uniform(&node.legal, &mut p);
            }
            rows.insert(key.clone(), p);
        }
        Table {
            game: self.game.spec().game_id.unwrap_or_default(),
            actions: self.actions,
            rows,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    TooLarge,
    Header,
    Game,
    Actions,
    Key(usize),
    Probability(usize),
    Sum(usize),
    Duplicate(usize),
    TooManyRows,
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "the table is larger than {MAX_TABLE_BYTES} bytes"),
            Self::Header => write!(f, "the first line is not a {TABLE_FORMAT} header"),
            Self::Game => write!(f, "the table is for another game"),
            Self::Actions => write!(f, "the table has another number of actions"),
            Self::Key(line) => write!(f, "line {line}: the key is not lowercase hex"),
            Self::Probability(line) => {
                write!(f, "line {line}: not one finite probability per action")
            }
            Self::Sum(line) => write!(f, "line {line}: the probabilities do not sum to 1"),
            Self::Duplicate(line) => write!(f, "line {line}: the key repeats"),
            Self::TooManyRows => write!(f, "more than {MAX_TABLE_ROWS} rows"),
        }
    }
}

impl std::error::Error for TableError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub game: String,
    pub actions: usize,
    pub rows: BTreeMap<Vec<u8>, Vec<f64>>,
}

fn hex_key(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) || bytes.len() > 2 * MAX_KEY_BYTES {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(digit(pair[0])? * 16 + digit(pair[1])?))
        .collect()
}

impl Table {
    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<&[f64]> {
        self.rows.get(key).map(Vec::as_slice)
    }

    #[must_use]
    pub fn write(&self) -> String {
        let mut out = format!("{TABLE_FORMAT} {} {}\n", self.game, self.actions);
        for (key, p) in &self.rows {
            for byte in key {
                out += &format!("{byte:02x}");
            }
            for v in p {
                out += &format!(" {v}");
            }
            out.push('\n');
        }
        out
    }

    pub fn read(text: &str, game: &str, actions: usize) -> Result<Self, TableError> {
        if text.len() > MAX_TABLE_BYTES {
            return Err(TableError::TooLarge);
        }
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split(' ').collect();
        if header.len() != 3 || header[0] != TABLE_FORMAT {
            return Err(TableError::Header);
        }
        if header[1] != game {
            return Err(TableError::Game);
        }
        if header[2].parse::<usize>().ok() != Some(actions) {
            return Err(TableError::Actions);
        }
        let mut rows = BTreeMap::new();
        for (index, line) in lines.enumerate() {
            let number = index + 2;
            if rows.len() >= MAX_TABLE_ROWS {
                return Err(TableError::TooManyRows);
            }
            let mut parts = line.split(' ');
            let key = hex_key(parts.next().unwrap_or_default()).ok_or(TableError::Key(number))?;
            let p: Vec<f64> = parts
                .take(actions.saturating_add(1))
                .map(|v| v.parse::<f64>().ok().filter(|v| (0.0..=1.0).contains(v)))
                .collect::<Option<_>>()
                .filter(|p: &Vec<f64>| p.len() == actions)
                .ok_or(TableError::Probability(number))?;
            let sum: f64 = p.iter().sum();
            if (sum - 1.0).abs() > 1e-6 {
                return Err(TableError::Sum(number));
            }
            if rows.insert(key, p).is_some() {
                return Err(TableError::Duplicate(number));
            }
        }
        Ok(Self {
            game: game.into(),
            actions,
            rows,
        })
    }
}
