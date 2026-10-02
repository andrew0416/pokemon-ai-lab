//! Experimental, perfect-information selective matrix-game search.
//!
//! A completed node is a mixed equilibrium of its current continuation values. Those
//! values change as the tree grows; its local best-response gap is NOT an exploitability
//! certificate for the original game, and quality need not improve monotonically.
//! This is not public-belief-state CFR. No private-information safety is claimed.
//!
//! The budget charges BEFORE each transition enumeration (including failed calls).
//! Enumeration and RM+ are indivisible calls: cost units are a work proxy, not a wall
//! deadline. An interrupted update returns the LAST fully backed-up root strategy.
//! Probabilities are never truncated or renormalized. Switches are resolved before
//! publishing a parent, including at the turn horizon. Unsupported mechanics fail closed.

mod engine;
mod matrix;

pub use engine::{EngineDomain, Position};

use crate::nash::Equilibrium;
use matrix::Solution;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Turn,
    Switch,
    Terminal,
}

/// A fully observable, simultaneous, zero-sum game. Values are always from player 0.
/// Each call receives a borrowed position and must leave it unchanged, even on errors.
pub trait Domain {
    type Position: Clone;
    type Action: Clone;

    fn phase(&self, position: &Self::Position) -> Result<Phase, String>;
    fn actions(
        &self,
        position: &Self::Position,
        player: usize,
    ) -> Result<Vec<Self::Action>, String>;
    fn value(&self, position: &Self::Position) -> f32;
    fn transitions(
        &self,
        position: &Self::Position,
        actions: [&Self::Action; 2],
    ) -> Result<Vec<(f64, Self::Position)>, String>;
}

/// Ranking only: it does not remove legal actions or replace their payoff values.
pub trait Prior<D: Domain> {
    fn weights(&self, position: &D::Position, player: usize, actions: &[D::Action]) -> Vec<f32>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Uniform;
impl<D: Domain> Prior<D> for Uniform {
    fn weights(&self, _: &D::Position, _: usize, actions: &[D::Action]) -> Vec<f32> {
        vec![1.0; actions.len()]
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub budget: u64,
    pub turn_cost: u64,
    pub switch_cost: u64,
    pub max_turns: u32,
    /// Stored tree nodes; a single engine enumeration can allocate before this check.
    pub max_nodes: usize,
    /// PUCT coefficient, in evaluator score units.
    pub exploration: f32,
    pub seed: u64,
    pub matrix_iterations: usize,
    pub matrix_tolerance: f32,
    pub double_oracle: bool,
    /// A deterministic frontier scan follows unsuccessful stochastic walks.
    pub walks_before_scan: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            budget: 1_000,
            turn_cost: 10,
            switch_cost: 1,
            max_turns: 3,
            max_nodes: 20_000,
            exploration: 25.0,
            seed: 1,
            matrix_iterations: 2_000,
            matrix_tolerance: 0.01,
            double_oracle: true,
            walks_before_scan: 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Budget,
    NodeLimit,
    /// Caller requested a stop at a completed root backup boundary.
    Observer,
    /// Every reachable continuation within the configured horizon has been expanded.
    FrontierExhausted,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    InvalidConfig(&'static str),
    Domain(String),
    InvalidValue,
    InvalidProbabilities,
    InvalidPrior,
    /// A broken backend must not recurse forever through non-turn decisions.
    SwitchChainLimit,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cost_used: u64,
    pub transitions: u64,
    pub turn_transitions: u64,
    pub switch_transitions: u64,
    pub evaluations: u64,
    pub expanded_nodes: usize,
    pub stored_nodes: usize,
    pub matrix_solves: u64,
    pub matrix_iterations: u64,
    pub committed_updates: u64,
    pub attempted_updates: u64,
    pub walks: u64,
    pub frontier_scans: u64,
    pub max_turn_depth: u32,
    pub response_sweeps: u64,
    pub skipped_backups: u64,
    pub retained_positions: usize,
    pub peak_positions: usize,
}

#[derive(Clone, Debug)]
pub struct Policy<A> {
    pub actions: [Vec<A>; 2],
    /// `exploitability` is the LOCAL leaf-valued matrix gap, not a full-game bound.
    pub equilibrium: Equilibrium,
    /// Exhausting the frontier does not imply RM+ reached its requested tolerance.
    pub local_tolerance_met: bool,
    pub known_cells: usize,
    pub total_cells: usize,
}

#[derive(Clone, Debug)]
pub struct Report<A> {
    /// None when the budget could not finish even the initial root game.
    pub policy: Option<Policy<A>>,
    pub terminal_value: Option<f32>,
    pub stop: Stop,
    /// Includes work in an interrupted, uncommitted final update.
    pub stats: Stats,
    pub seed: u64,
    pub max_turns: u32,
}

enum Control {
    Stop(Stop),
    Error(Error),
}
impl From<Error> for Control {
    fn from(e: Error) -> Self {
        Self::Error(e)
    }
}

struct Cell {
    outcomes: Vec<(f64, usize)>,
}

struct Node<P, A> {
    #[cfg(not(feature = "experiment-response-sweeps"))]
    position: P,
    #[cfg(feature = "experiment-response-sweeps")]
    position: Option<Box<P>>,
    #[cfg(feature = "experiment-response-sweeps")]
    complete: bool,
    #[cfg(feature = "experiment-response-sweeps")]
    incoming_cell: usize,
    phase: Phase,
    remaining: u32,
    turn_depth: u32,
    switch_chain: u32,
    actions: [Vec<A>; 2],
    priors: [Vec<f32>; 2],
    visits: [Vec<u64>; 2],
    #[cfg(not(feature = "experiment-response-sweeps"))]
    walk_visits: u64,
    cells: Vec<Option<Cell>>,
    solution: Option<Solution>,
    value: f32,
}

impl<P, A> Node<P, A> {
    fn position(&self) -> &P {
        #[cfg(feature = "experiment-response-sweeps")]
        { self.position.as_deref().expect("unfinished node retains its position") }
        #[cfg(not(feature = "experiment-response-sweeps"))]
        { &self.position }
    }
    fn leaf(&self) -> bool {
        self.phase == Phase::Terminal || (self.phase == Phase::Turn && self.remaining == 0)
    }
}

#[cfg(any(test, not(feature = "experiment-response-sweeps")))]
struct Rng(u64);
#[cfg(any(test, not(feature = "experiment-response-sweeps")))]
impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        ((z >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }
    #[cfg(not(feature = "experiment-response-sweeps"))]
    fn weighted(&mut self, weights: impl IntoIterator<Item = f64>) -> usize {
        let weights: Vec<_> = weights.into_iter().collect();
        let total: f64 = weights.iter().sum();
        let mut pick = self.unit() * total;
        let mut last_positive = 0;
        for (i, &w) in weights.iter().enumerate() {
            if w > 0.0 {
                last_positive = i;
            }
            if pick < w {
                return i;
            }
            pick -= w;
        }
        last_positive
    }
}

struct Work<'a, D: Domain, P: Prior<D>> {
    domain: &'a D,
    prior: &'a P,
    config: Config,
    nodes: Vec<Node<D::Position, D::Action>>,
    stats: Stats,
    #[cfg(not(feature = "experiment-response-sweeps"))]
    rng: Rng,
    #[cfg(feature = "experiment-response-sweeps")]
    response_visits: Vec<u64>,
}

struct Target {
    /// Root through target node, inclusive. A tree, not a state-merged DAG.
    path: Vec<usize>,
    cell: Option<usize>,
}

/// Finite-horizon experiment. No engine/default configuration is changed.
pub fn search<D: Domain, P: Prior<D>>(
    domain: &D,
    prior: &P,
    position: D::Position,
    config: Config,
) -> Result<Report<D::Action>, Error> {
    search_with_observer(domain, prior, position, config, |_, _| false)
}

/// Publishes only completed root backups. Returning true requests a stop.
/// The observer is not a hard deadline: enumeration and matrix solves are indivisible.
/// Its elapsed time counts towards callers' wall budgets. A failed search invalidates all
/// earlier observations; consumers must retain errors as well as completed policies.
pub fn search_with_observer<D: Domain, P: Prior<D>>(
    domain: &D,
    prior: &P,
    position: D::Position,
    config: Config,
    mut observer: impl FnMut(&Policy<D::Action>, &Stats) -> bool,
) -> Result<Report<D::Action>, Error> {
    if config.turn_cost == 0
        || config.switch_cost == 0
        || config.max_nodes == 0
        || config.max_turns == 0
        || config.max_turns > 16
        || config.matrix_iterations == 0
        || config.walks_before_scan > 1_024
    {
        return Err(Error::InvalidConfig(
            "positive costs/nodes/iterations, horizon 1..16, walks <=1024",
        ));
    }
    if !config.exploration.is_finite()
        || config.exploration < 0.0
        || !config.matrix_tolerance.is_finite()
        || config.matrix_tolerance <= 0.0
    {
        return Err(Error::InvalidConfig(
            "finite nonnegative exploration and positive tolerance",
        ));
    }
    let mut w = Work {
        domain,
        prior,
        #[cfg(not(feature = "experiment-response-sweeps"))]
        rng: Rng(config.seed),
        config,
        nodes: Vec::new(),
        stats: Stats::default(),
        #[cfg(feature = "experiment-response-sweeps")]
        response_visits: Vec::new(),
    };
    w.push_node(position, w.config.max_turns, 0, 0)?;
    let mut incumbent = None;
    let terminal_value = (w.nodes[0].phase == Phase::Terminal).then_some(w.nodes[0].value);
    let result = if terminal_value.is_some() {
        Ok(())
    } else {
        (|| -> Result<(), Control> {
            w.solve_node(0)?;
            incumbent = Some(w.policy());
            w.stats.stored_nodes = w.nodes.len();
            if observer(incumbent.as_ref().unwrap(), &w.stats) {
                return Err(Control::Stop(Stop::Observer));
            }
            loop {
                let target = w.select();
                let Some(target) = target else {
                    return Ok(());
                };
                w.stats.attempted_updates += 1;
                let id = *target.path.last().expect("root path");
                if let Some(cell) = target.cell {
                    w.ensure_cell(id, cell)?;
                }
                #[cfg(not(feature = "experiment-response-sweeps"))]
                for &ancestor in target.path.iter().rev() { w.solve_node(ancestor)?; }
                #[cfg(feature = "experiment-response-sweeps")]
                w.backup(&target)?;
                // Every affected ancestor is now solved against the new continuations.
                // Only this boundary makes the new root visible to callers.
                incumbent = Some(w.policy());
                w.stats.committed_updates += 1;
                w.stats.stored_nodes = w.nodes.len();
                if observer(incumbent.as_ref().unwrap(), &w.stats) {
                    return Err(Control::Stop(Stop::Observer));
                }
            }
        })()
    };
    let stop = match result {
        Ok(()) => Stop::FrontierExhausted,
        Err(Control::Stop(stop)) => stop,
        Err(Control::Error(e)) => return Err(e),
    };
    w.stats.stored_nodes = w.nodes.len();
    Ok(Report {
        policy: incumbent,
        terminal_value,
        stop,
        stats: w.stats,
        seed: w.config.seed,
        max_turns: w.config.max_turns,
    })
}

impl<D: Domain, P: Prior<D>> Work<'_, D, P> {
    fn push_node(
        &mut self,
        position: D::Position,
        remaining: u32,
        turn_depth: u32,
        switch_chain: u32,
    ) -> Result<usize, Error> {
        let phase = self.domain.phase(&position).map_err(Error::Domain)?;
        if phase == Phase::Switch && switch_chain > 64 {
            return Err(Error::SwitchChainLimit);
        }
        let value = self.domain.value(&position);
        if !value.is_finite() || value.abs() > 1_000_000.0 {
            return Err(Error::InvalidValue);
        }
        self.stats.evaluations += 1;
        self.stats.max_turn_depth = self.stats.max_turn_depth.max(turn_depth);
        let id = self.nodes.len();
        #[cfg(feature = "experiment-response-sweeps")]
        let complete = phase == Phase::Terminal || (phase == Phase::Turn && remaining == 0);
        #[cfg(feature = "experiment-response-sweeps")]
        let position = if complete { None } else {
            self.stats.retained_positions += 1;
            self.stats.peak_positions = self.stats.peak_positions.max(self.stats.retained_positions);
            Some(Box::new(position))
        };
        self.nodes.push(Node {
            position,
            #[cfg(feature = "experiment-response-sweeps")]
            complete,
            #[cfg(feature = "experiment-response-sweeps")]
            incoming_cell: 0,
            phase,
            remaining,
            turn_depth,
            switch_chain,
            actions: [Vec::new(), Vec::new()],
            priors: [Vec::new(), Vec::new()],
            visits: [Vec::new(), Vec::new()],
            #[cfg(not(feature = "experiment-response-sweeps"))]
            walk_visits: 0,
            cells: Vec::new(),
            solution: None,
            value,
        });
        Ok(id)
    }

    fn prepare(&mut self, id: usize) -> Result<(), Control> {
        if !self.nodes[id].cells.is_empty() {
            return Ok(());
        }
        let node = &mut self.nodes[id];
        for player in 0..2 {
            let actions = self
                .domain
                .actions(node.position(), player)
                .map_err(Error::Domain)?;
            if actions.is_empty() {
                return Err(Error::Domain("nonterminal empty menu".into()).into());
            }
            let mut priors = self.prior.weights(node.position(), player, &actions);
            if priors.len() != actions.len() || priors.iter().any(|p| !p.is_finite() || *p < 0.0) {
                return Err(Error::InvalidPrior.into());
            }
            let total: f64 = priors.iter().map(|&p| f64::from(p)).sum();
            if total <= 0.0 {
                return Err(Error::InvalidPrior.into());
            }
            for p in &mut priors {
                *p = (f64::from(*p) / total) as f32;
            }
            node.visits[player] = vec![0; actions.len()];
            node.actions[player] = actions;
            node.priors[player] = priors;
        }
        let cells = node.actions[0]
            .len()
            .checked_mul(node.actions[1].len())
            .ok_or(Error::InvalidConfig("menu product overflow"))?;
        // A separate matrix allocation bound, since nodes do not limit a huge menu.
        if cells > 1_000_000 {
            return Err(Error::InvalidConfig("matrix over 1M cells").into());
        }
        node.cells = (0..cells).map(|_| None).collect();
        Ok(())
    }

    fn ensure_cell(&mut self, id: usize, cell: usize) -> Result<(), Control> {
        if self.nodes[id].cells[cell].is_some() {
            return Ok(());
        }
        let node = &self.nodes[id];
        let is_turn = node.phase == Phase::Turn;
        let cost = if is_turn {
            self.config.turn_cost
        } else {
            self.config.switch_cost
        };
        if cost > self.config.budget - self.stats.cost_used {
            return Err(Control::Stop(Stop::Budget));
        }
        self.stats.cost_used += cost;
        self.stats.transitions += 1;
        if is_turn {
            self.stats.turn_transitions += 1;
        } else {
            self.stats.switch_transitions += 1;
        }
        let m = node.actions[1].len();
        let outcomes = self
            .domain
            .transitions(
                node.position(),
                [&node.actions[0][cell / m], &node.actions[1][cell % m]],
            )
            .map_err(Error::Domain)?;
        let total: f64 = outcomes.iter().map(|x| x.0).sum();
        if outcomes.is_empty()
            || outcomes.iter().any(|x| !x.0.is_finite() || x.0 < 0.0)
            || !total.is_finite()
            || (total - 1.0).abs() > 1e-9
        {
            return Err(Error::InvalidProbabilities.into());
        }
        let count = outcomes.iter().filter(|x| x.0 > 0.0).count();
        if count > self.config.max_nodes - self.nodes.len() {
            return Err(Control::Stop(Stop::NodeLimit));
        }
        let remaining = node.remaining - u32::from(is_turn);
        let turn_depth = node.turn_depth + u32::from(is_turn);
        let switch_chain = if is_turn { 0 } else { node.switch_chain + 1 };
        let mut edges = Vec::with_capacity(count);
        for (probability, position) in outcomes {
            if probability == 0.0 {
                continue;
            }
            // Resolving a preceding sibling's forced switches may have filled the arena.
            if self.nodes.len() >= self.config.max_nodes {
                return Err(Control::Stop(Stop::NodeLimit));
            }
            let child = self.push_node(position, remaining, turn_depth, switch_chain)?;
            #[cfg(feature = "experiment-response-sweeps")]
            { self.nodes[child].incoming_cell = cell; }
            // A switch is a decision, not a terminal heuristic leaf. Resolve its matrix
            // even when this turn used the last normal-turn depth unit.
            if self.nodes[child].phase == Phase::Switch {
                self.solve_node(child)?;
            }
            edges.push((probability, child));
        }
        self.nodes[id].cells[cell] = Some(Cell { outcomes: edges });
        Ok(())
    }

    fn payoff(&mut self, id: usize, cell: usize) -> Result<f32, Control> {
        self.ensure_cell(id, cell)?;
        let value: f64 = self.nodes[id].cells[cell]
            .as_ref()
            .unwrap()
            .outcomes
            .iter()
            .map(|&(p, child)| p * f64::from(self.nodes[child].value))
            .sum();
        let value = value as f32;
        if !value.is_finite() {
            return Err(Error::InvalidValue.into());
        }
        Ok(value)
    }

    fn solve_node(&mut self, id: usize) -> Result<(), Control> {
        if self.nodes[id].leaf() {
            return Ok(());
        }
        self.prepare(id)?;
        let rows = self.nodes[id].actions[0].len();
        let cols = self.nodes[id].actions[1].len();
        let mut work = matrix::Counts::default();
        let result = matrix::solve(
            rows,
            cols,
            self.config.matrix_iterations,
            self.config.matrix_tolerance,
            self.config.double_oracle,
            &mut work,
            |cell| self.payoff(id, cell),
        );
        self.stats.matrix_solves += work.solves;
        self.stats.matrix_iterations += work.iterations;
        let solution = result?;
        let node = &mut self.nodes[id];
        if node.solution.is_none() {
            self.stats.expanded_nodes += 1;
        }
        node.value = solution.equilibrium.value;
        node.solution = Some(solution);
        #[cfg(feature = "experiment-response-sweeps")]
        self.refresh_complete(id);
        Ok(())
    }

    /// A matrix entry with zero probability on BOTH axes contributes to neither
    /// unilateral best response. Changing it leaves this equilibrium, its value,
    /// all action values, and the measured local gap valid (including approximate RM+).
    #[cfg(feature = "experiment-response-sweeps")]
    fn irrelevant_cell(&self, id: usize, cell: Option<usize>) -> bool {
        let node = &self.nodes[id];
        let (Some(solution), Some(cell)) = (&node.solution, cell) else { return false; };
        let cols = node.actions[1].len();
        solution.equilibrium.rows[cell / cols] == 0.0
            && solution.equilibrium.cols[cell % cols] == 0.0
    }

    #[cfg(feature = "experiment-response-sweeps")]
    fn backup(&mut self, target: &Target) -> Result<(), Control> {
        let mut changed = true;
        for index in (0..target.path.len()).rev() {
            let id = target.path[index];
            let cell = if index + 1 == target.path.len() { target.cell }
                else { Some(self.nodes[target.path[index + 1]].incoming_cell) };
            let old = self.nodes[id].value.to_bits();
            if changed && !self.irrelevant_cell(id, cell) {
                self.solve_node(id)?;
            } else {
                self.stats.skipped_backups += 1;
            }
            // Completion must propagate even when the scalar payoff did not change.
            self.refresh_complete(id);
            changed = old != self.nodes[id].value.to_bits();
        }
        Ok(())
    }

    #[cfg(feature = "experiment-response-sweeps")]
    fn refresh_complete(&mut self, id: usize) {
        if self.nodes[id].complete { return; }
        let node = &self.nodes[id];
        let complete = node.solution.is_some() && node.cells.iter().all(|edge| {
            edge.as_ref().is_some_and(|edge| edge.outcomes.iter().all(|&(_, child)| self.nodes[child].complete))
        });
        if complete {
            self.nodes[id].complete = true;
            if self.nodes[id].position.take().is_some() { self.stats.retained_positions -= 1; }
        }
    }

    fn policy(&self) -> Policy<D::Action> {
        let root = &self.nodes[0];
        Policy {
            actions: root.actions.clone(),
            equilibrium: root.solution.as_ref().unwrap().equilibrium.clone(),
            local_tolerance_met: root.solution.as_ref().unwrap().equilibrium.exploitability
                <= self.config.matrix_tolerance,
            known_cells: root.cells.iter().filter(|c| c.is_some()).count(),
            total_cells: root.cells.len(),
        }
    }

    fn select(&mut self) -> Option<Target> {
        #[cfg(feature = "experiment-response-sweeps")]
        { self.response_target() }
        #[cfg(not(feature = "experiment-response-sweeps"))]
        {
        for _ in 0..self.config.walks_before_scan {
            self.stats.walks += 1;
            if let Some(target) = self.walk() {
                return Some(target);
            }
        }
        self.stats.frontier_scans += 1;
        self.scan(0, &mut Vec::new())
        }
    }

    /// Fairly revisit responses to EITHER player's current strategy. A product
    /// distribution can starve a decisive counter just because its present opponent
    /// probability is zero. The uniform floor also preserves eventual full coverage.
    #[cfg(feature = "experiment-response-sweeps")]
    fn response_target(&mut self) -> Option<Target> {
        if self.nodes[0].complete { return None; }
        self.stats.response_sweeps += 1;
        self.response_visits.resize(self.nodes[0].cells.len(), 0);
        let root = &self.nodes[0];
        let eq = &root.solution.as_ref().expect("initial root solved").equilibrium;
        let cols = root.actions[1].len();
        let floor = 1.0 / (root.actions[0].len() + cols) as f64;
        let mut ranked: Vec<_> = (0..root.cells.len()).map(|cell| {
            let score = (f64::from(eq.rows[cell / cols]) + f64::from(eq.cols[cell % cols]) + floor)
                / (1.0 + self.response_visits[cell] as f64);
            (cell, score)
        }).collect();
        ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (cell, _) in ranked {
            let target = if let Some(edge) = &self.nodes[0].cells[cell] {
                edge.outcomes.iter().find_map(|&(_, child)| self.scan(child, &mut vec![0]))
            } else { Some(Target { path: vec![0], cell: Some(cell) }) };
            if let Some(target) = target {
                self.response_visits[cell] += 1;
                return Some(target);
            }
        }
        None
    }

    #[cfg(not(feature = "experiment-response-sweeps"))]
    fn walk(&mut self) -> Option<Target> {
        let mut id = 0;
        let mut path = Vec::new();
        loop {
            path.push(id);
            let node = &mut self.nodes[id];
            if node.leaf() {
                return None;
            }
            let Some(solution) = &node.solution else {
                return Some(Target { path, cell: None });
            };
            node.walk_visits += 1;
            let mut actions = [0; 2];
            for (player, action) in actions.iter_mut().enumerate() {
                let strategy = if player == 0 {
                    &solution.equilibrium.rows
                } else {
                    &solution.equilibrium.cols
                };
                *action = if self.rng.unit() < 0.5 {
                    self.rng.weighted(strategy.iter().map(|&p| f64::from(p)))
                } else {
                    let q = if player == 0 {
                        &solution.row_values
                    } else {
                        &solution.col_values
                    };
                    let mut best = (0, f64::NEG_INFINITY);
                    for (a, &value) in q.iter().enumerate() {
                        let signed = if player == 0 { value } else { -value };
                        let score = f64::from(signed)
                            + f64::from(self.config.exploration)
                                * f64::from(node.priors[player][a])
                                * (node.walk_visits as f64).sqrt()
                                / (1.0 + node.visits[player][a] as f64);
                        if score > best.1 {
                            best = (a, score);
                        }
                    }
                    best.0
                };
                node.visits[player][*action] += 1;
            }
            let cell = actions[0] * node.actions[1].len() + actions[1];
            let Some(edge) = &node.cells[cell] else {
                return Some(Target {
                    path,
                    cell: Some(cell),
                });
            };
            let selected = self.rng.weighted(edge.outcomes.iter().map(|x| x.0));
            id = edge.outcomes[selected].1;
        }
    }

    fn scan(&self, id: usize, path: &mut Vec<usize>) -> Option<Target> {
        let node = &self.nodes[id];
        #[cfg(feature = "experiment-response-sweeps")]
        if node.complete { return None; }
        if node.leaf() {
            return None;
        }
        path.push(id);
        if node.solution.is_none() {
            return Some(Target {
                path: path.clone(),
                cell: None,
            });
        }
        for (cell, edge) in node.cells.iter().enumerate() {
            if edge.is_none() {
                return Some(Target {
                    path: path.clone(),
                    cell: Some(cell),
                });
            }
        }
        for edge in node.cells.iter().flatten() {
            for &(_, child) in &edge.outcomes {
                if let Some(target) = self.scan(child, path) {
                    return Some(target);
                }
            }
        }
        path.pop();
        None
    }
}

#[cfg(test)]
mod tests;
