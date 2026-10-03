//! Public-history-closed growing-tree CFR reference experiment.
//!
//! Admission expands every physical history of a public prefix, with every legal joint
//! action and chance outcome. It never expands just the world sampled by the PUCT walk.
//! CFR restarts after growth: old regrets/averages are not certificates for a new game.
//! The last committed, fully solved snapshot survives resource exhaustion. Its gap is
//! only for the current fixed-leaf surrogate, never for omitted continuations.

use super::super::{self as tree, Compiled, Information, Policy, Solution};
use super::*;
use crate::bayesian::Config as CfrConfig;
use std::collections::BTreeMap;
mod history;
use history::History;
#[cfg(feature = "experiment-belief-workspace")]
pub mod reuse;
#[cfg(feature = "experiment-parallel-transitions")]
type Outcomes<P> = Result<Vec<(f64, P)>, String>;
/// Sealed inside this crate: production implementation is the immutable engine only.
#[cfg(feature = "experiment-parallel-transitions")]
pub(crate) trait Batch<D: ObservedDomain> {
    fn width(&self) -> usize;
    fn run(
        &self,
        domain: &D,
        p: &D::Position,
        joints: &[[&D::Action; 2]],
    ) -> Vec<Outcomes<D::Position>>;
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Public-prefix admissions, including root and automatic switch closure.
    pub max_expansions: usize,
    /// Selection walks, including walks ending in an already terminal/horizon node.
    pub max_walks: usize,
    /// Dimensionless: action values are scaled by the current tree's utility scale.
    pub exploration: f64,
    pub seed: u64,
    pub solver: CfrConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_expansions: 16,
            max_walks: 256,
            exploration: 1.,
            seed: 1,
            solver: CfrConfig::default(),
        }
    }
}

/// The prior sees ONLY an information key, its owner and semantic menu. No true world
/// or physical state is supplied. Keys may contain the owner's legitimate private type.
pub trait Prior {
    fn weights(&self, information: &Information) -> Vec<f64>;
}
pub struct Uniform;
impl Prior for Uniform {
    fn weights(&self, information: &Information) -> Vec<f64> {
        vec![1.; information.actions.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    HorizonComplete,
    ExpansionLimit,
    TransitionLimit,
    NodeLimit,
    DecisionLimit,
    WalkLimit,
}
#[derive(Clone, Debug, Default)]
pub struct Work {
    /// Includes work in a rejected atomic admission; counters never roll back.
    pub attempted_transitions: usize,
    pub attempted_expansions: usize,
    pub committed_expansions: usize,
    pub walks: usize,
    pub solves: usize,
    pub cfr_iterations: usize,
}
pub struct ResultTree {
    pub built: Built,
    pub solution: Solution,
    pub stop: Stop,
    pub work: Work,
    pub frontier_histories: usize,
    pub frontier_public_groups: usize,
    /// Exact only for the requested turn horizon and supplied observations/evaluator.
    pub horizon_complete: bool,
}

#[derive(Debug)]
enum Failure {
    Limit(Stop),
    Invalid(Error),
}
impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Self::Invalid(e)
    }
}
impl From<String> for Failure {
    fn from(e: String) -> Self {
        Self::Invalid(Error(e))
    }
}
type Attempt<T> = Result<T, Failure>;

struct Frontier<P> {
    node: usize,
    position: P,
    world: usize,
    memory: History,
    public: Vec<String>,
    turns: u32,
    decisions: usize,
    phase: Phase,
}
impl<P: Clone> Clone for Frontier<P> {
    fn clone(&self) -> Self {
        Self {
            node: self.node,
            position: self.position.clone(),
            world: self.world,
            memory: self.memory.clone(),
            public: self.public.clone(),
            turns: self.turns,
            decisions: self.decisions,
            phase: self.phase,
        }
    }
}
struct Growing<'a, D: ObservedDomain> {
    #[cfg(feature = "experiment-parallel-transitions")]
    batch: Option<&'a dyn Batch<D>>,
    b: Builder<'a, D>,
    root: usize,
    worlds: Vec<String>,
    frontier: BTreeMap<Vec<String>, Vec<Frontier<D::Position>>>,
}
impl<'a, D: ObservedDomain> Growing<'a, D> {
    fn fork(&self) -> Self {
        Self {
            b: Builder {
                domain: self.b.domain,
                limits: self.b.limits,
                nodes: self.b.nodes.clone(),
                stats: self.b.stats.clone(),
                menus: self.b.menus.clone(),
                public: self.b.public.clone(),
                public_keys: self.b.public_keys.clone(),
                private: self.b.private.clone(),
                private_keys: self.b.private_keys.clone(),
                boundaries: self.b.boundaries.clone(),
            },
            #[cfg(feature = "experiment-parallel-transitions")]
            batch: self.batch,
            root: self.root,
            worlds: self.worlds.clone(),
            frontier: self.frontier.clone(),
        }
    }
    fn compile(&self) -> Result<Tree, Error> {
        let mut t = Tree::new(self.b.nodes.clone(), self.root)?;
        t.worlds = self.worlds.clone();
        t.public_keys = self.b.public_keys.clone();
        t.private_keys = self.b.private_keys.clone();
        t.boundaries = self.b.boundaries.clone();
        Ok(t)
    }
    fn push(&mut self, n: Node) -> Attempt<usize> {
        if self.b.nodes.len() >= self.b.limits.max_nodes {
            return Err(Failure::Limit(Stop::NodeLimit));
        }
        Ok(self.b.push(n)?)
    }
    fn leaf(&mut self, mut h: Frontier<D::Position>) -> Attempt<usize> {
        let value = {
            #[cfg(feature = "experiment-phase-cost")]
            let _eval = crate::bayesian::tree::phase_cost::Span::new(
                crate::bayesian::tree::phase_cost::Phase::Evaluate,
            );
            f64::from(self.b.domain.value(&h.position))
        };
        if !value.is_finite() || value.abs() > f64::MAX / 8. {
            return Err(Error("invalid frontier evaluation".into()).into());
        }
        let node = self.push(Node::Terminal(value))?;
        h.node = node;
        self.b.stats.leaves += 1;
        let keys = {
            #[cfg(feature = "experiment-phase-cost")]
            let _keys = crate::bayesian::tree::phase_cost::Span::new(
                crate::bayesian::tree::phase_cost::Phase::Keys,
            );
            h.memory.keys()
        };
        self.b.mark(node, h.world, h.public.clone(), &keys);
        if h.phase != Phase::Terminal && !(h.phase == Phase::Turn && h.turns == 0) {
            #[cfg(feature = "experiment-phase-cost")]
            let _frontier = crate::bayesian::tree::phase_cost::Span::new(
                crate::bayesian::tree::phase_cost::Phase::Frontier,
            );
            let group = self.frontier.entry(h.public.clone()).or_default();
            if group
                .first()
                .is_some_and(|old| old.phase != h.phase || old.turns != h.turns)
            {
                return Err(
                    Error("public observation must distinguish decision phases".into()).into(),
                );
            }
            group.push(h);
        }
        Ok(node)
    }
    fn expand(&mut self, key: &[String], work: &mut Work, cfg: Config) -> Attempt<()> {
        if work.attempted_expansions >= cfg.max_expansions {
            return Err(Failure::Limit(Stop::ExpansionLimit));
        }
        work.attempted_expansions += 1;
        let group = self
            .frontier
            .remove(key)
            .ok_or_else(|| Error("selection targeted an absent public frontier".into()))?;
        // All histories (including zero-current-policy ones) are mandatory. Reaching only
        // some of these histories under today's policy does not license dropping the rest.
        for h in group {
            if h.decisions >= self.b.limits.max_decisions {
                return Err(Failure::Limit(Stop::DecisionLimit));
            }
            let p = &h.position;
            #[cfg(feature = "experiment-phase-cost")]
            let menu_span = crate::bayesian::tree::phase_cost::Span::new(
                crate::bayesian::tree::phase_cost::Phase::Menu,
            );
            let left = self.b.domain.actions(p, 0)?;
            let right = self.b.domain.actions(p, 1)?;
            let rows: Vec<_> = left
                .iter()
                .map(|a| self.b.domain.action_id(p, 0, a))
                .collect();
            let cols: Vec<_> = right
                .iter()
                .map(|a| self.b.domain.action_id(p, 1, a))
                .collect();
            let keys = h.memory.keys();
            self.b.menu(&keys[0], &rows)?;
            self.b.menu(&keys[1], &cols)?;
            #[cfg(feature = "experiment-phase-cost")]
            drop(menu_span);
            let turns = if h.phase == Phase::Turn {
                self.b.stats.turn_decisions += 1;
                h.turns - 1
            } else {
                self.b.stats.switch_decisions += 1;
                h.turns
            };
            self.b.stats.leaves -= 1;
            #[cfg(feature = "experiment-parallel-transitions")]
            let mut prefetched = std::collections::VecDeque::new();
            let mut row_children = Vec::new();
            for (r, row) in left.iter().enumerate() {
                let col_node = self.push(Node::Terminal(0.))?;
                row_children.push(col_node);
                let mut col_children = Vec::new();
                for (c, col) in right.iter().enumerate() {
                    #[cfg(feature = "experiment-parallel-transitions")]
                    if let Some(batch) = self.batch {
                        if prefetched.is_empty() {
                            let remaining =
                                self.b.limits.max_transitions - work.attempted_transitions;
                            if remaining == 0 {
                                return Err(Failure::Limit(Stop::TransitionLimit));
                            }
                            let start = r * right.len() + c;
                            let count = batch
                                .width()
                                .min(4)
                                .min(remaining)
                                .min(left.len() * right.len() - start);
                            if count == 0 {
                                return Err(Error("empty transition batch".into()).into());
                            }
                            let joints: Vec<_> = (start..start + count)
                                .map(|n| [&left[n / right.len()], &right[n % right.len()]])
                                .collect();
                            // Every submitted engine call is attempted work, even if a
                            // preceding result or node allocation rejects the admission.
                            work.attempted_transitions += count;
                            let results = {
                                #[cfg(feature = "experiment-phase-cost")]
                                let _transition = crate::bayesian::tree::phase_cost::Span::new(
                                    crate::bayesian::tree::phase_cost::Phase::Transitions,
                                );
                                batch.run(self.b.domain, p, &joints)
                            };
                            if results.len() != count {
                                return Err(Error("transition batch size mismatch".into()).into());
                            }
                            prefetched = results.into();
                        }
                    }
                    let mut serial = || -> Result<_, Failure> {
                        if work.attempted_transitions >= self.b.limits.max_transitions {
                            return Err(Failure::Limit(Stop::TransitionLimit));
                        }
                        work.attempted_transitions += 1;
                        #[cfg(feature = "experiment-phase-cost")]
                        let _transition = crate::bayesian::tree::phase_cost::Span::new(
                            crate::bayesian::tree::phase_cost::Phase::Transitions,
                        );
                        Ok(self.b.domain.transitions(p, [row, col])?)
                    };
                    #[cfg(feature = "experiment-parallel-transitions")]
                    let children = match prefetched.pop_front() {
                        Some(v) => v?,
                        None => serial()?,
                    };
                    #[cfg(not(feature = "experiment-parallel-transitions"))]
                    let children = serial()?;
                    self.b.stats.transitions += 1;
                    if children.is_empty()
                        || children
                            .iter()
                            .any(|(q, _)| !q.is_finite() || *q < 0. || *q > 1.)
                        || (children.iter().map(|(q, _)| q).sum::<f64>() - 1.).abs() > 1e-10
                    {
                        return Err(Error(
                            "chance mass must be one; no truncation or renormalization".into(),
                        )
                        .into());
                    }
                    self.b.stats.chance_outcomes = self
                        .b
                        .stats
                        .chance_outcomes
                        .checked_add(children.len())
                        .ok_or_else(|| Error("outcome count overflow".into()))?;
                    let chance = self.push(Node::Terminal(0.))?;
                    col_children.push(chance);
                    let mut edges = Vec::new();
                    for (probability, position) in children {
                        let obs = {
                            #[cfg(feature = "experiment-phase-cost")]
                            let _obs = crate::bayesian::tree::phase_cost::Span::new(
                                crate::bayesian::tree::phase_cost::Phase::Observation,
                            );
                            self.b.domain.observation(&position)?
                        };
                        let phase = {
                            #[cfg(feature = "experiment-phase-cost")]
                            let _phase = crate::bayesian::tree::phase_cost::Span::new(
                                crate::bayesian::tree::phase_cost::Phase::PositionPhase,
                            );
                            self.b.domain.phase(&position)?
                        };
                        #[cfg(feature = "experiment-phase-cost")]
                        let history_span = crate::bayesian::tree::phase_cost::Span::new(
                            crate::bayesian::tree::phase_cost::Phase::History,
                        );
                        let memory = h.memory.advance([&rows[r], &cols[c]], &obs);
                        let mut public = h.public.clone();
                        public.push(obs.public);
                        #[cfg(feature = "experiment-phase-cost")]
                        drop(history_span);
                        let child = self.leaf(Frontier {
                            node: 0,
                            position,
                            world: h.world,
                            memory,
                            public,
                            turns,
                            decisions: h.decisions + 1,
                            phase,
                        })?;
                        edges.push((probability, child));
                    }
                    self.b.nodes[chance] = Node::Chance(edges);
                }
                // Column information must not contain the hidden row commitment.
                self.b.nodes[col_node] = Node::Decision {
                    player: 1,
                    information: keys[1].clone(),
                    actions: cols.clone(),
                    children: col_children,
                };
            }
            self.b.nodes[h.node] = Node::Decision {
                player: 0,
                information: keys[0].clone(),
                actions: rows,
                children: row_children,
            };
        }
        Ok(())
    }
    fn admit(&mut self, key: &[String], work: &mut Work, cfg: Config) -> Attempt<()> {
        #[cfg(feature = "experiment-phase-cost")]
        let _admit = crate::bayesian::tree::phase_cost::Span::new(
            crate::bayesian::tree::phase_cost::Phase::Admission,
        );
        self.expand(key, work, cfg)?;
        // Publishing a snapshot while a pending replacement is priced as a heuristic
        // leaf would change the previous solver's horizon contract. Close ALL switches.
        loop {
            let next = self
                .frontier
                .iter()
                .find(|(_, g)| g[0].phase == Phase::Switch)
                .map(|(k, _)| k.clone());
            match next {
                Some(k) => self.expand(&k, work, cfg)?,
                None => return Ok(()),
            }
        }
    }
}

struct Random(u64);
impl Random {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 * (1. / ((1u64 << 53) as f64))
    }
    fn sample(&mut self, weights: &[f64]) -> usize {
        let mut x = self.unit();
        for (i, &p) in weights.iter().enumerate() {
            if x < p {
                return i;
            }
            x -= p;
        }
        weights
            .iter()
            .rposition(|p| *p > 0.)
            .expect("validated probability vector")
    }
}
type Visits = HashMap<String, Vec<u64>>;

/// CF action values aggregate all member histories before selection. No sampled-world
/// value or node-wise best response is used as a substitute for information-set Q.
fn scores(t: &Tree, policy: &Policy) -> Result<Vec<Vec<f64>>, Error> {
    #[cfg(feature = "experiment-phase-cost")]
    let _scores = crate::bayesian::tree::phase_cost::Span::new(
        crate::bayesian::tree::phase_cost::Phase::Scores,
    );
    let values = t.values(policy);
    let cf = [t.reach(policy, Some(0))?, t.reach(policy, Some(1))?];
    Ok(scores_from(t, &values, &cf))
}
fn scores_from(t: &Tree, values: &[f64], cf: &[Vec<f64>; 2]) -> Vec<Vec<f64>> {
    let mut result = Vec::new();
    for info in t.information() {
        let mut q = vec![0.; info.actions.len()];
        let mass: f64 = info.nodes.iter().map(|n| cf[info.player][*n]).sum();
        if mass > 0. {
            for &n in &info.nodes {
                let Compiled::Decision { children, .. } = &t.nodes[n] else {
                    unreachable!()
                };
                for (a, &child) in children.iter().enumerate() {
                    q[a] += cf[info.player][n] / mass
                        * values[child]
                        * if info.player == 0 { 1. } else { -1. };
                }
            }
        }
        result.push(q);
    }
    result
}

fn select<P: Prior>(
    t: &Tree,
    policy: &Policy,
    q: &[Vec<f64>],
    prior: &P,
    visits: &mut Visits,
    random: &mut Random,
    exploration: f64,
) -> Result<usize, Error> {
    let mut n = t.root();
    loop {
        n = match &t.nodes[n] {
            Compiled::Terminal(_) => return Ok(n),
            Compiled::Chance(edges) => {
                edges[random.sample(&edges.iter().map(|(p, _)| *p).collect::<Vec<_>>())].1
            }
            Compiled::Decision { info, children } => {
                let information = &t.information[*info];
                let mut p = prior.weights(information);
                if p.len() != children.len() {
                    return Err(Error("prior menu width mismatch".into()));
                }
                normalize(&mut p)?;
                let counts = visits
                    .entry(information.key.clone())
                    .or_insert_with(|| vec![0; children.len()]);
                if counts.len() != children.len() {
                    return Err(Error("changed selection menu".into()));
                }
                let total = counts.iter().map(|v| *v as f64).sum::<f64>();
                let mut best = 0;
                let mut score = f64::NEG_INFINITY;
                for a in 0..children.len() {
                    let s = q[*info][a]
                        + exploration * p[a] * (total + 1.).sqrt() / (1. + counts[a] as f64);
                    if !s.is_finite() {
                        return Err(Error("nonfinite PUCT score".into()));
                    }
                    if s > score {
                        score = s;
                        best = a;
                    }
                }
                let a = if random.unit() < 0.5 {
                    best
                } else {
                    random.sample(&policy[*info])
                };
                counts[a] = counts[a]
                    .checked_add(1)
                    .ok_or_else(|| Error("visit count overflow".into()))?;
                children[a]
            }
        };
    }
}

/// Bounded correctness reference for growing-tree CFR with the Pokai/SoG 50:50 PUCT /
/// average-policy selection rule. This is not a learned CFV network or safe re-solving.
pub fn search<D: ObservedDomain, P: Prior>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
    cfg: Config,
    prior: &P,
) -> Result<ResultTree, Error> {
    if cfg.max_expansions == 0
        || cfg.max_walks == 0
        || !cfg.exploration.is_finite()
        || cfg.exploration < 0.
        || limits.turns == 0
        || limits.turns > 32
        || limits.max_decisions == 0
        || limits.max_decisions > 128
        || limits.max_nodes == 0
        || limits.max_transitions == 0
    {
        return Err(Error("invalid growing-tree configuration".into()));
    }
    // Validate solver configuration before any domain calls or transition work.
    if cfg.solver.iterations == 0
        || cfg.solver.iterations > 10_000_000
        || cfg.solver.check_every == 0
        || !cfg.solver.tolerance.is_finite()
        || cfg.solver.tolerance < 0.
    {
        return Err(Error("invalid CFR configuration".into()));
    }
    labels(seeds.iter().map(|s| s.id.as_str()), "world ID")?;
    let mut weights: Vec<_> = seeds.iter().map(|s| s.weight).collect();
    normalize(&mut weights)?;
    let first = domain.observation(&seeds[0].position)?;
    let mut growing = Growing {
        #[cfg(feature = "experiment-parallel-transitions")]
        batch: None,
        b: Builder {
            domain,
            limits,
            nodes: Vec::new(),
            stats: Stats::default(),
            menus: HashMap::new(),
            public: HashMap::new(),
            public_keys: Vec::new(),
            private: HashMap::new(),
            private_keys: Vec::new(),
            boundaries: Vec::new(),
        },
        root: 0,
        worlds: seeds.iter().map(|s| s.id.clone()).collect(),
        frontier: BTreeMap::new(),
    };
    let initial = (|| -> Attempt<()> {
        growing.root = growing.push(Node::Terminal(0.))?;
        let mut edges = Vec::new();
        for (world, (s, weight)) in seeds.iter().zip(weights).enumerate() {
            let obs = domain.observation(&s.position)?;
            if obs.public != first.public
                || obs.private[0] != first.private[0]
                || domain.phase(&s.position)? != Phase::Turn
            {
                return Err(Error(
                    "root worlds must share our initial information at a normal turn".into(),
                )
                .into());
            }
            let memory = [
                Cursor::new(0, None, &obs)?.memory,
                Cursor::new(1, Some(&s.id), &obs)?.memory,
            ];
            let n = growing.leaf(Frontier {
                node: 0,
                position: s.position.clone(),
                world,
                memory: History::raw(memory),
                public: vec![obs.public],
                turns: limits.turns,
                decisions: 0,
                phase: Phase::Turn,
            })?;
            edges.push((weight, n));
        }
        growing.b.nodes[growing.root] = Node::Chance(edges);
        Ok(())
    })();
    let fatal = |e: Failure| match e {
        Failure::Invalid(e) => e,
        Failure::Limit(s) => Error(format!("root incomplete: {s:?}; no strategy returned")),
    };
    initial.map_err(fatal)?;
    let mut work = Work::default();
    growing
        .admit(&[first.public], &mut work, cfg)
        .map_err(fatal)?;
    work.committed_expansions = work.attempted_expansions;
    let mut t = growing.compile()?;
    let mut solved = tree::solve(&t, cfg.solver)?;
    work.solves += 1;
    work.cfr_iterations += solved.iterations;
    let mut q = scores(&t, &solved.policy)?;
    let mut random = Random(cfg.seed);
    let mut visits = Visits::new();
    let stop = loop {
        if growing.frontier.is_empty() {
            break Stop::HorizonComplete;
        }
        if work.attempted_expansions >= cfg.max_expansions {
            break Stop::ExpansionLimit;
        }
        if work.walks >= cfg.max_walks {
            break Stop::WalkLimit;
        }
        work.walks += 1;
        let selected = select(
            &t,
            &solved.policy,
            &q,
            prior,
            &mut visits,
            &mut random,
            cfg.exploration,
        )?;
        let key = growing
            .frontier
            .iter()
            .find(|(_, g)| g.iter().any(|h| h.node == selected))
            .map(|(k, _)| k.clone());
        let Some(key) = key else { continue };
        let mut candidate = growing.fork();
        let before = work.attempted_expansions;
        match candidate.admit(&key, &mut work, cfg) {
            Err(Failure::Limit(s)) => break s,
            Err(Failure::Invalid(e)) => return Err(e),
            Ok(()) => {}
        }
        let next = candidate.compile()?;
        let solution = tree::solve(&next, cfg.solver)?;
        q = scores(&next, &solution.policy)?;
        work.committed_expansions += work.attempted_expansions - before;
        work.solves += 1;
        work.cfr_iterations += solution.iterations;
        growing = candidate;
        t = next;
        solved = solution;
    };
    Ok(ResultTree {
        built: Built {
            tree: t,
            stats: growing.b.stats,
        },
        solution: solved,
        stop,
        frontier_histories: growing.frontier.values().map(Vec::len).sum(),
        frontier_public_groups: growing.frontier.len(),
        horizon_complete: growing.frontier.is_empty(),
        work,
    })
}

#[cfg(test)]
mod tests;
