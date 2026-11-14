use feint_protocol::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

pub trait Game {
    type World: Clone;
    type Action: Copy + PartialEq;

    fn to_act(&self, world: &Self::World) -> Option<u8>;

    fn legal(&self, world: &Self::World, player: u8) -> Vec<Self::Action>;

    fn apply(&self, world: &mut Self::World, player: u8, action: Self::Action);

    fn value(&self, world: &Self::World, me: u8) -> f64;

    fn public(&self, world: &Self::World, player: u8, action: Self::Action) -> u64;

    fn private(&self, world: &Self::World, me: u8) -> u64 {
        let _ = (world, me);
        0
    }
}

pub trait Model<G: Game>: Clone {
    fn policy(&self, game: &G, world: &G::World, player: u8) -> Vec<(G::Action, f64)>;

    fn observe(&mut self, game: &G, before: &G::World, player: u8, action: G::Action);
}

pub trait Prior<G: Game> {
    fn prior(&mut self, game: &G, world: &G::World, me: u8, legal: &[G::Action]) -> Vec<f64>;
}

pub struct Uniform;

impl<G: Game> Prior<G> for Uniform {
    fn prior(&mut self, _game: &G, _world: &G::World, _me: u8, legal: &[G::Action]) -> Vec<f64> {
        let n = legal.len().max(1) as f64;
        vec![1.0 / n; legal.len()]
    }
}

pub struct Setup<'a, G: Game, M, P> {
    pub game: &'a G,
    pub model: &'a M,
    pub prior: &'a mut P,
    pub me: u8,
    pub budget: Budget,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    pub iterations: u32,
    pub depth: u32,
    pub top_k: usize,
    pub explore: f64,
}

pub const UNVISITED: f64 = 0.5;

struct Edge<A> {
    action: A,
    prior: f64,
    visits: u32,
    available: u32,
    total: f64,
    children: Vec<(u64, usize)>,
}

#[derive(Default)]
struct Node<A> {
    edges: Vec<Edge<A>>,
    outcomes: Vec<((u64, u64), usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RootEdge<A> {
    pub action: A,
    pub prior: f64,
    pub visits: u32,
    pub mean: f64,
}

pub struct Search<A> {
    nodes: Vec<Node<A>>,
}

fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

fn top(prior: &[f64], k: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..prior.len()).collect();
    order.sort_by(|&a, &b| prior[b].total_cmp(&prior[a]).then(a.cmp(&b)));
    order.truncate(k.max(1));
    order.sort_unstable();
    order
}

impl<A: Copy + PartialEq> Search<A> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                edges: Vec::new(),
                outcomes: Vec::new(),
            }],
        }
    }

    fn fresh(&mut self) -> usize {
        self.nodes.push(Node {
            edges: Vec::new(),
            outcomes: Vec::new(),
        });
        self.nodes.len() - 1
    }

    fn extend<G, P>(
        edges: &mut Vec<Edge<A>>,
        game: &G,
        world: &G::World,
        me: u8,
        prior: &mut P,
        k: usize,
        legal: &[A],
    ) where
        G: Game<Action = A>,
        P: Prior<G>,
    {
        let mut p = prior.prior(game, world, me, legal);
        p.resize(legal.len(), 0.0);
        let kept = top(&p, k);
        let mass: f64 = kept.iter().map(|&i| p[i].max(0.0)).sum();
        let even = 1.0 / kept.len().max(1) as f64;
        for i in kept {
            if edges.iter().any(|e| e.action == legal[i]) {
                continue;
            }
            edges.push(Edge {
                action: legal[i],
                prior: if mass > 0.0 {
                    p[i].max(0.0) / mass
                } else {
                    even
                },
                visits: 0,
                available: 0,
                total: 0.0,
                children: Vec::new(),
            });
        }
    }

    fn select(&mut self, node: usize, legal: &[A], explore: f64) -> Option<usize> {
        let edges = &mut self.nodes[node].edges;
        let mut best: Option<(usize, f64)> = None;
        for (i, e) in edges.iter_mut().enumerate() {
            if !legal.contains(&e.action) {
                continue;
            }
            e.available += 1;
            let q = if e.visits == 0 {
                UNVISITED
            } else {
                e.total / f64::from(e.visits)
            };
            let u = explore * e.prior * f64::from(e.available).sqrt() / (1.0 + f64::from(e.visits));
            let score = q + u;
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((i, score));
            }
        }
        best.map(|(i, _)| i)
    }

    pub fn run<G, M, P>(
        &mut self,
        setup: &mut Setup<'_, G, M, P>,
        sample: &mut dyn FnMut(&mut ChaCha8Rng) -> G::World,
        rng: &mut ChaCha8Rng,
    ) where
        G: Game<Action = A>,
        M: Model<G>,
        P: Prior<G>,
    {
        let (game, me, budget) = (setup.game, setup.me, setup.budget);
        let mut path: Vec<(usize, usize)> = Vec::new();
        for _ in 0..budget.iterations {
            let mut world = sample(rng);
            let mut mind = setup.model.clone();
            let mut node = 0usize;
            let mut left = budget.depth;
            path.clear();
            while let Some(player) = game.to_act(&world) {
                if player == me {
                    if left == 0 {
                        break;
                    }
                    let legal = game.legal(&world, me);
                    let edges = &self.nodes[node].edges;
                    let missing = legal.iter().any(|a| !edges.iter().any(|e| e.action == *a));
                    let stuck = !edges.iter().any(|e| legal.contains(&e.action));
                    if stuck || (missing && edges.len() < budget.top_k) {
                        Self::extend(
                            &mut self.nodes[node].edges,
                            game,
                            &world,
                            me,
                            setup.prior,
                            budget.top_k,
                            &legal,
                        );
                    }
                    let Some(i) = self.select(node, &legal, budget.explore) else {
                        break;
                    };
                    path.push((node, i));
                    let action = self.nodes[node].edges[i].action;
                    mind.observe(game, &world, me, action);
                    game.apply(&mut world, me, action);
                    let key = game.private(&world, me);
                    let found = self.nodes[node].edges[i]
                        .children
                        .iter()
                        .find(|c| c.0 == key)
                        .map(|c| c.1);
                    node = match found {
                        Some(c) => c,
                        None => {
                            let c = self.fresh();
                            self.nodes[node].edges[i].children.push((key, c));
                            c
                        }
                    };
                    left -= 1;
                } else {
                    let odds = mind.policy(game, &world, player);
                    let total: f64 = odds.iter().map(|&(_, p)| p.max(0.0)).sum();
                    if total <= 0.0 {
                        break;
                    }
                    let u = unit(rng) * total;
                    let mut acc = 0.0;
                    let mut pick = None;
                    for &(a, p) in &odds {
                        acc += p.max(0.0);
                        if u < acc {
                            pick = Some(a);
                            break;
                        }
                    }
                    let Some(action) =
                        pick.or_else(|| odds.iter().rev().find(|x| x.1 > 0.0).map(|x| x.0))
                    else {
                        break;
                    };
                    let public = game.public(&world, player, action);
                    mind.observe(game, &world, player, action);
                    game.apply(&mut world, player, action);
                    let key = (public, game.private(&world, me));
                    node = match self.nodes[node].outcomes.iter().find(|o| o.0 == key) {
                        Some(&(_, c)) => c,
                        None => {
                            let c = self.fresh();
                            self.nodes[node].outcomes.push((key, c));
                            c
                        }
                    };
                }
            }
            let v = game.value(&world, me);
            for &(n, i) in &path {
                let e = &mut self.nodes[n].edges[i];
                e.visits += 1;
                e.total += v;
            }
        }
    }

    #[must_use]
    pub fn root(&self) -> Vec<RootEdge<A>> {
        self.nodes[0]
            .edges
            .iter()
            .map(|e| RootEdge {
                action: e.action,
                prior: e.prior,
                visits: e.visits,
                mean: if e.visits == 0 {
                    UNVISITED
                } else {
                    e.total / f64::from(e.visits)
                },
            })
            .collect()
    }

    #[must_use]
    pub fn best(&self) -> Option<A> {
        let mut best: Option<&Edge<A>> = None;
        for e in &self.nodes[0].edges {
            let better = match best {
                None => true,
                Some(b) => e.visits > b.visits || (e.visits == b.visits && e.prior > b.prior),
            };
            if better {
                best = Some(e);
            }
        }
        best.map(|e| e.action)
    }
}

impl<A: Copy + PartialEq> Default for Search<A> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn decide<G, M, P>(
    setup: &mut Setup<'_, G, M, P>,
    sample: &mut dyn FnMut(&mut ChaCha8Rng) -> G::World,
    rng: &mut ChaCha8Rng,
) -> Option<G::Action>
where
    G: Game,
    M: Model<G>,
    P: Prior<G>,
{
    let mut search = Search::new();
    search.run(setup, sample, rng);
    search.best()
}
