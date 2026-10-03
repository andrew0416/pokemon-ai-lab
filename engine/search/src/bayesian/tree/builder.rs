//! Full finite-tree compiler from a simultaneous transition domain and explicit observations.
//! Public histories and each player's own observations/actions are remembered separately.
//! A domain author must never put hidden commitments/state in an observation or action ID.
use super::{Boundary, Node, Tree};
use crate::bayesian::{labels, normalize, Error};
use crate::budgeted::{Domain, Phase};
use std::collections::HashMap;

mod history;
#[cfg(feature = "experiment-interned-history-observer")]
pub use history::observer as history_observer;
use history::{Histories, Plain};

#[cfg(feature = "experiment-growing-belief")]
pub mod growing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub public: String,
    pub private: [String; 2],
}
pub trait ObservedDomain: Domain {
    fn observation(&self, position: &Self::Position) -> Result<Observation, String>;
    /// Semantic identity in THIS player's menu, not a hidden-world/global array index.
    fn action_id(&self, position: &Self::Position, player: usize, action: &Self::Action) -> String;
}
#[derive(Clone, Debug)]
pub struct Seed<P> {
    pub id: String,
    pub weight: f64,
    pub position: P,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub turns: u32,
    pub max_nodes: usize,
    pub max_transitions: usize,
    pub max_decisions: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            turns: 2,
            max_nodes: 50_000,
            max_transitions: 20_000,
            max_decisions: 32,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub transitions: usize,
    pub chance_outcomes: usize,
    pub turn_decisions: usize,
    pub switch_decisions: usize,
    pub leaves: usize,
}
pub struct Built {
    pub tree: Tree,
    pub stats: Stats,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Memory<S = String> {
    Type(S),
    Observe(S, S),
    Action(S),
}

/// Live continuation lookup uses only the acting player's memory. Player 0 cannot
/// supply a world ID. A pending opponent commitment is never part of this cursor.
#[derive(Clone, Debug)]
pub struct Cursor {
    player: usize,
    memory: Vec<Memory>,
}
impl Cursor {
    pub fn new(player: usize, world: Option<&str>, observed: &Observation) -> Result<Self, Error> {
        if player > 1 || (player == 0 && world.is_some()) || (player == 1 && world.is_none()) {
            return Err(Error(
                "cursor requires no world for player 0, a private type for player 1".into(),
            ));
        }
        let mut memory = Vec::new();
        if let Some(id) = world {
            memory.push(Memory::Type(id.into()));
        }
        memory.push(Memory::Observe(
            observed.public.clone(),
            observed.private[player].clone(),
        ));
        Ok(Self { player, memory })
    }
    pub fn advance(&mut self, own_action: &str, observed: &Observation) {
        self.memory.push(Memory::Action(own_action.into()));
        self.memory.push(Memory::Observe(
            observed.public.clone(),
            observed.private[self.player].clone(),
        ));
    }
    pub fn key(&self) -> String {
        format!("{}:{:?}", self.player, self.memory)
    }
    pub fn information(&self, tree: &Tree) -> Option<usize> {
        tree.information().iter().position(|i| i.key == self.key())
    }
    pub fn belief(
        &self,
        tree: &Tree,
        policy: &super::Policy,
    ) -> Result<Option<super::PublicBelief>, Error> {
        tree.private_belief(self.player, &self.key(), policy)
    }
}

struct Builder<'a, D: ObservedDomain, H: Histories = Plain> {
    history: H,
    domain: &'a D,
    limits: Limits,
    nodes: Vec<Node>,
    stats: Stats,
    menus: HashMap<String, Vec<String>>,
    public: HashMap<Vec<String>, usize>,
    public_keys: Vec<String>,
    private: HashMap<String, usize>,
    private_keys: Vec<String>,
    boundaries: Vec<Boundary>,
}

impl<D: ObservedDomain, H: Histories> Builder<'_, D, H> {
    fn mark(&mut self, node: usize, world: usize, public: Vec<String>, keys: &[H::Key; 2]) {
        let public_id = if let Some(&id) = self.public.get(&public) {
            id
        } else {
            let id = self.public_keys.len();
            self.public_keys.push(format!("{public:?}"));
            self.public.insert(public, id);
            id
        };
        let mut private = [0; 2];
        for (side, key) in keys.iter().enumerate() {
            private[side] = if let Some(&id) = self.private.get(key.as_ref()) {
                id
            } else {
                let id = self.private_keys.len();
                self.private_keys.push(key.as_ref().to_owned());
                self.private.insert(key.as_ref().to_owned(), id);
                id
            };
        }
        self.boundaries.push(Boundary {
            node,
            world,
            public: public_id,
            private,
        });
    }
    fn push(&mut self, node: Node) -> Result<usize, Error> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(Error(
                "tree node cap exceeded; no partial solution returned".into(),
            ));
        }
        let id = self.nodes.len();
        self.nodes.push(node);
        Ok(id)
    }
    fn menu(&mut self, key: &str, actions: &[String]) -> Result<(), Error> {
        labels(actions.iter().map(String::as_str), "action ID")?;
        if let Some(old) = self.menus.get(key) {
            if old != actions {
                return Err(Error("indistinguishable histories have different menus; observation contract is incomplete".into()));
            }
        } else {
            self.menus.insert(key.into(), actions.to_vec());
        }
        Ok(())
    }
    fn walk(
        &mut self,
        pos: &D::Position,
        world: usize,
        memory: [H::Path; 2],
        public: Vec<String>,
        turns: u32,
        decisions: usize,
    ) -> Result<usize, Error> {
        let phase = self.domain.phase(pos)?;
        let keys = self.history.keys(&memory);
        if phase == Phase::Terminal || (turns == 0 && phase == Phase::Turn) {
            let value = f64::from(self.domain.value(pos));
            self.stats.leaves += 1;
            let node = self.push(Node::Terminal(value))?;
            self.mark(node, world, public, &keys);
            return Ok(node);
        }
        if decisions >= self.limits.max_decisions {
            return Err(Error(
                "decision depth cap exceeded; pending switches were not evaluated as leaves".into(),
            ));
        }
        let left = self.domain.actions(pos, 0)?;
        let right = self.domain.actions(pos, 1)?;
        let row_ids: Vec<_> = left
            .iter()
            .map(|a| self.domain.action_id(pos, 0, a))
            .collect();
        let col_ids: Vec<_> = right
            .iter()
            .map(|a| self.domain.action_id(pos, 1, a))
            .collect();
        self.menu(keys[0].as_ref(), &row_ids)?;
        self.menu(keys[1].as_ref(), &col_ids)?;
        let next_turns = if phase == Phase::Turn {
            self.stats.turn_decisions += 1;
            turns - 1
        } else {
            self.stats.switch_decisions += 1;
            turns
        };
        let row_node = self.push(Node::Terminal(0.))?;
        self.mark(row_node, world, public.clone(), &keys);
        let mut row_children = Vec::new();
        for (r, row) in left.iter().enumerate() {
            let col_node = self.push(Node::Terminal(0.))?;
            row_children.push(col_node);
            let mut col_children = Vec::new();
            for (c, column) in right.iter().enumerate() {
                if self.stats.transitions >= self.limits.max_transitions {
                    return Err(Error(
                        "transition cap exceeded; no partial solution returned".into(),
                    ));
                }
                self.stats.transitions += 1;
                let children = self.domain.transitions(pos, [row, column])?;
                self.stats.chance_outcomes = self
                    .stats
                    .chance_outcomes
                    .checked_add(children.len())
                    .ok_or_else(|| Error("outcome count overflow".into()))?;
                if children.is_empty()
                    || children
                        .iter()
                        .any(|(p, _)| !p.is_finite() || *p < 0. || *p > 1.)
                    || (children.iter().map(|v| v.0).sum::<f64>() - 1.).abs() > 1e-10
                {
                    return Err(Error("chance mass must be one; no renormalization".into()));
                }
                let chance = self.push(Node::Terminal(0.))?;
                col_children.push(chance);
                let mut edges = Vec::new();
                for (p, child) in children {
                    let observed = self.domain.observation(&child)?;
                    let next = self
                        .history
                        .advance(&memory, [&row_ids[r], &col_ids[c]], &observed);
                    let mut trace = public.clone();
                    trace.push(observed.public);
                    let target =
                        self.walk(&child, world, next, trace, next_turns, decisions + 1)?;
                    edges.push((p, target));
                }
                self.nodes[chance] = Node::Chance(edges);
            }
            // Same key for every hidden row choice: player 1 does NOT observe it.
            self.nodes[col_node] = Node::Decision {
                player: 1,
                information: keys[1].as_ref().to_owned(),
                actions: col_ids.clone(),
                children: col_children,
            };
        }
        self.nodes[row_node] = Node::Decision {
            player: 0,
            information: keys[0].as_ref().to_owned(),
            actions: row_ids,
            children: row_children,
        };
        Ok(row_node)
    }
}

pub fn build<D: ObservedDomain>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
) -> Result<Built, Error> {
    build_with::<D, Plain>(domain, seeds, limits)
}

#[cfg(feature = "experiment-interned-history")]
pub fn build_interned<D: ObservedDomain>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
) -> Result<Built, Error> {
    build_with::<D, history::Interned>(domain, seeds, limits)
}

fn build_with<D: ObservedDomain, H: Histories>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
) -> Result<Built, Error> {
    if limits.turns == 0
        || limits.turns > 32
        || limits.max_decisions == 0
        || limits.max_decisions > 128
        || limits.max_nodes == 0
        || limits.max_transitions == 0
    {
        return Err(Error("invalid finite-tree limits".into()));
    }
    labels(seeds.iter().map(|s| s.id.as_str()), "world ID")?;
    let mut weights: Vec<_> = seeds.iter().map(|s| s.weight).collect();
    normalize(&mut weights)?;
    let first = domain.observation(&seeds[0].position)?;
    let mut b = Builder {
        history: H::default(),
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
    };
    let root = b.push(Node::Terminal(0.))?;
    let mut edges = Vec::new();
    for (world, (seed, weight)) in seeds.iter().zip(weights).enumerate() {
        if domain.phase(&seed.position)? != Phase::Turn {
            return Err(Error(
                "seed must be a normal turn; use the built continuation policy for switches".into(),
            ));
        }
        let obs = domain.observation(&seed.position)?;
        if obs.public != first.public || obs.private[0] != first.private[0] {
            return Err(Error(
                "root worlds do not share our initial information".into(),
            ));
        }
        let memory = b.history.root(&seed.id, &obs);
        let child = b.walk(
            &seed.position,
            world,
            memory,
            vec![obs.public],
            limits.turns,
            0,
        )?;
        edges.push((weight, child));
    }
    b.nodes[root] = Node::Chance(edges);
    let mut tree = Tree::new(b.nodes, root)?;
    tree.worlds = seeds.iter().map(|s| s.id.clone()).collect();
    tree.public_keys = b.public_keys;
    tree.private_keys = b.private_keys;
    tree.boundaries = b.boundaries;
    Ok(Built {
        tree,
        stats: b.stats,
    })
}
