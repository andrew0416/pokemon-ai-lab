//! Bounded extensive-form CFR with perfect recall and strategy-induced public beliefs.
//!
//! Histories remain separate in the tree; indistinguishable decision histories share a
//! regret table. Simultaneous choices are represented sequentially WITHOUT disclosing the
//! first choice. The certificate is for this fully built, finite, fixed-leaf game only.
//! No independent perfect-information continuation solves or belief-independent neural
//! value claims are made. See `builder` and `engine` for observation contracts.

pub mod builder;
#[cfg(feature = "experiment-paper-solvers")]
pub use workspace::cached::paper;
#[cfg(feature = "experiment-belief-workspace")]
pub mod compiler;
pub mod engine;
#[cfg(feature = "experiment-shared-final-passes")]
pub mod shared;
#[cfg(feature = "experiment-belief-workspace")]
pub mod workspace;
use super::{labels, probability, strategy, Config, Error};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Node {
    Terminal(f64),
    Chance(Vec<(f64, usize)>),
    Decision {
        player: usize,
        information: String,
        actions: Vec<String>,
        children: Vec<usize>,
    },
}
#[derive(Clone, Debug)]
enum Compiled {
    Terminal(f64),
    Chance(Vec<(f64, usize)>),
    Decision { info: usize, children: Vec<usize> },
}
#[derive(Clone, Debug)]
pub struct Information {
    pub player: usize,
    pub key: String,
    pub actions: Vec<String>,
    pub nodes: Vec<usize>,
    /// Validated identical own information/action sequence at every member history.
    pub own_sequence: Vec<(usize, usize)>,
}
#[derive(Clone, Debug)]
pub struct Boundary {
    pub node: usize,
    pub world: usize,
    pub public: usize,
    pub private: [usize; 2],
}
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Compiled>,
    root: usize,
    order: Vec<usize>,
    information: Vec<Information>,
    scale: f64,
    pub(crate) worlds: Vec<String>,
    pub(crate) public_keys: Vec<String>,
    pub(crate) private_keys: Vec<String>,
    pub(crate) boundaries: Vec<Boundary>,
}
pub type Policy = Vec<Vec<f64>>;

fn multiply(a: f64, b: f64) -> Result<f64, Error> {
    let v = a * b;
    if a > 0. && b > 0. && v == 0. {
        return Err(Error(
            "positive history reach underflow; history was not dropped".into(),
        ));
    }
    Ok(v)
}

impl Tree {
    /// All nodes must form one reachable tree (no shared history nodes/cycles). Information
    /// keys can repeat only with the same owner, menu, and complete own action recall.
    pub fn new(raw: Vec<Node>, root: usize) -> Result<Self, Error> {
        if raw.is_empty() || root >= raw.len() {
            return Err(Error("invalid root".into()));
        }
        let mut nodes = Vec::with_capacity(raw.len());
        let mut information: Vec<Information> = Vec::new();
        let mut ids = HashMap::new();
        let mut scale = 0f64;
        for node in raw {
            nodes.push(match node {
                Node::Terminal(v) => {
                    if !v.is_finite() || v.abs() > f64::MAX / 8. {
                        return Err(Error("invalid terminal value".into()));
                    }
                    scale = scale.max(v.abs());
                    Compiled::Terminal(v)
                }
                Node::Chance(edges) => {
                    if edges.is_empty()
                        || edges
                            .iter()
                            .any(|(p, _)| !p.is_finite() || *p < 0. || *p > 1.)
                        || (edges.iter().map(|v| v.0).sum::<f64>() - 1.).abs() > 1e-10
                    {
                        return Err(Error("chance probabilities must sum to one".into()));
                    }
                    Compiled::Chance(edges)
                }
                Node::Decision {
                    player,
                    information: key,
                    actions,
                    children,
                } => {
                    if player > 1 || key.is_empty() || actions.len() != children.len() {
                        return Err(Error("invalid information/menu".into()));
                    }
                    labels(actions.iter().map(String::as_str), "action ID")?;
                    let info = if let Some(&i) = ids.get(&key) {
                        let existing: &Information = &information[i];
                        if existing.player != player || existing.actions != actions {
                            return Err(Error(format!("inconsistent information set: {key}")));
                        }
                        i
                    } else {
                        let i = information.len();
                        ids.insert(key.clone(), i);
                        information.push(Information {
                            player,
                            key,
                            actions,
                            nodes: Vec::new(),
                            own_sequence: Vec::new(),
                        });
                        i
                    };
                    Compiled::Decision { info, children }
                }
            });
        }
        let mut seen = vec![false; nodes.len()];
        let mut order = Vec::new();
        let mut stack = vec![(root, [Vec::new(), Vec::new()], 0usize)];
        while let Some((id, sequences, depth)) = stack.pop() {
            if id >= nodes.len() || seen[id] || depth > 512 {
                return Err(Error(
                    "tree has invalid/shared/cyclic nodes or exceeds depth 512".into(),
                ));
            }
            seen[id] = true;
            order.push(id);
            match &nodes[id] {
                Compiled::Terminal(_) => {}
                Compiled::Chance(edges) => {
                    for &(_, child) in edges.iter().rev() {
                        stack.push((child, sequences.clone(), depth + 1));
                    }
                }
                Compiled::Decision { info, children } => {
                    let i = &mut information[*info];
                    if i.nodes.is_empty() {
                        i.own_sequence = sequences[i.player].clone();
                    } else if i.own_sequence != sequences[i.player] {
                        return Err(Error(format!("imperfect recall in {}", i.key)));
                    }
                    i.nodes.push(id);
                    for (a, &child) in children.iter().enumerate().rev() {
                        let mut next = sequences.clone();
                        next[i.player].push((*info, a));
                        stack.push((child, next, depth + 1));
                    }
                }
            }
        }
        if seen.iter().any(|v| !*v) {
            return Err(Error("unreachable tree node".into()));
        }
        Ok(Self {
            nodes,
            root,
            order,
            information,
            scale: scale.max(f64::MIN_POSITIVE),
            worlds: Vec::new(),
            public_keys: Vec::new(),
            private_keys: Vec::new(),
            boundaries: Vec::new(),
        })
    }
    pub fn information(&self) -> &[Information] {
        &self.information
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// Developer/oracle export, NOT an observation supplied to the playing agent.
    #[cfg(feature = "experiment-growing-belief")]
    pub fn export_nodes(&self) -> Vec<Node> {
        self.nodes
            .iter()
            .map(|n| match n {
                Compiled::Terminal(v) => Node::Terminal(*v),
                Compiled::Chance(e) => Node::Chance(e.clone()),
                Compiled::Decision { info, children } => Node::Decision {
                    player: self.information[*info].player,
                    information: self.information[*info].key.clone(),
                    actions: self.information[*info].actions.clone(),
                    children: children.clone(),
                },
            })
            .collect()
    }
    pub fn root(&self) -> usize {
        self.root
    }
    pub fn worlds(&self) -> &[String] {
        &self.worlds
    }
    pub fn boundaries(&self) -> &[Boundary] {
        &self.boundaries
    }
    pub fn information_at(&self, node: usize) -> Option<usize> {
        match self.nodes.get(node) {
            Some(Compiled::Decision { info, .. }) => Some(*info),
            _ => None,
        }
    }
    pub fn uniform(&self) -> Policy {
        self.information
            .iter()
            .map(|i| vec![1. / i.actions.len() as f64; i.actions.len()])
            .collect()
    }
    fn check_policy(&self, policy: &Policy) -> Result<(), Error> {
        if policy.len() != self.information.len() {
            return Err(Error("information policy count mismatch".into()));
        }
        for (i, p) in self.information.iter().zip(policy) {
            probability(p, i.actions.len())?;
        }
        Ok(())
    }
    fn values(&self, policy: &Policy) -> Vec<f64> {
        let mut values = vec![0.; self.nodes.len()];
        for &id in self.order.iter().rev() {
            values[id] = match &self.nodes[id] {
                Compiled::Terminal(v) => v / self.scale,
                Compiled::Chance(edges) => edges.iter().map(|(p, n)| p * values[*n]).sum(),
                Compiled::Decision { info, children } => children
                    .iter()
                    .zip(&policy[*info])
                    .map(|(n, p)| p * values[*n])
                    .sum(),
            };
        }
        values
    }
    /// `omit` removes that player's realization reach for counterfactual regret/BR.
    fn reach(&self, policy: &Policy, omit: Option<usize>) -> Result<Vec<f64>, Error> {
        let mut reach = vec![0.; self.nodes.len()];
        reach[self.root] = 1.;
        for &id in &self.order {
            match &self.nodes[id] {
                Compiled::Terminal(_) => {}
                Compiled::Chance(edges) => {
                    for &(p, n) in edges {
                        reach[n] = multiply(reach[id], p)?;
                    }
                }
                Compiled::Decision { info, children } => {
                    for (a, &n) in children.iter().enumerate() {
                        reach[n] = multiply(
                            reach[id],
                            if omit == Some(self.information[*info].player) {
                                1.
                            } else {
                                policy[*info][a]
                            },
                        )?;
                    }
                }
            }
        }
        Ok(reach)
    }
    pub fn assess(&self, policy: &Policy) -> Result<Assessment, Error> {
        self.check_policy(policy)?;
        let value = self.values(policy)[self.root] * self.scale;
        let upper = self.best_response(policy, 0)?.0;
        let lower = self.best_response(policy, 1)?.0;
        Ok(Assessment {
            value,
            lower,
            upper,
            gap: (upper - lower).max(0.),
        })
    }
    /// A pure best response with ONE chosen action per information set, including
    /// off-policy histories. Maximizing each physical node independently is invalid.
    pub fn best_response(
        &self,
        policy: &Policy,
        player: usize,
    ) -> Result<(f64, Vec<Option<usize>>), Error> {
        self.response_with_reach(policy, player)
            .map(|(value, actions, _)| (value, actions))
    }
    #[allow(clippy::type_complexity)]
    fn response_with_reach(
        &self,
        policy: &Policy,
        player: usize,
    ) -> Result<(f64, Vec<Option<usize>>, Vec<f64>), Error> {
        self.check_policy(policy)?;
        if player > 1 {
            return Err(Error("invalid player".into()));
        }
        let cf = self.reach(policy, Some(player))?;
        let mut work = Response {
            tree: self,
            policy,
            player,
            cf,
            memo: vec![None; self.nodes.len()],
            chosen: vec![None; self.information.len()],
            visiting: vec![false; self.information.len()],
        };
        let value = work.node(self.root)? * self.scale;
        Ok((value, work.chosen, work.cf))
    }
    /// Diagnostics under the supplied strategy. Zero-reach public histories have no
    /// posterior (never an invented uniform belief). History weights retain hidden queued
    /// action alternatives even when those alternatives belong to the same world.
    pub fn public_beliefs(&self, policy: &Policy) -> Result<Vec<PublicBelief>, Error> {
        self.beliefs(policy, &self.public_keys, |b| Some(b.public))
    }
    /// A player's posterior also conditions on remembered own actions and private
    /// observations. Public belief alone is insufficient after a hidden own commitment.
    /// This works at horizon endpoints as well as at decision nodes.
    pub fn private_belief(
        &self,
        player: usize,
        key: &str,
        policy: &Policy,
    ) -> Result<Option<PublicBelief>, Error> {
        if player > 1 {
            return Err(Error("invalid belief owner".into()));
        }
        let id = self.private_keys.iter().position(|v| v == key);
        let keys = [key.to_owned()];
        let mut result = self.beliefs(policy, &keys, |b| {
            (Some(b.private[player]) == id).then_some(0)
        })?;
        let belief = result.remove(0);
        Ok((!belief.histories.is_empty()).then_some(belief))
    }
    fn beliefs(
        &self,
        policy: &Policy,
        keys: &[String],
        select: impl Fn(&Boundary) -> Option<usize>,
    ) -> Result<Vec<PublicBelief>, Error> {
        self.check_policy(policy)?;
        let reach = self.reach(policy, None)?;
        let values = self.values(policy);
        let mut result: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(id, key)| PublicBelief {
                id,
                key: key.clone(),
                reach: 0.,
                posterior: None,
                world_values: vec![None; self.worlds.len()],
                histories: Vec::new(),
            })
            .collect();
        let mut masses = vec![vec![0.; self.worlds.len()]; result.len()];
        let mut sums = masses.clone();
        for b in &self.boundaries {
            let Some(group) = select(b) else {
                continue;
            };
            let r = reach[b.node];
            let out = &mut result[group];
            out.reach += r;
            out.histories.push((b.node, r));
            masses[group][b.world] += r;
            if r > 0.0 {
                // Conditional averaging avoids multiplying a tiny world reach by a
                // small scaled payoff and then attempting to divide the underflow away.
                let alpha = r / masses[group][b.world];
                sums[group][b.world] += alpha * (values[b.node] - sums[group][b.world]);
            }
        }
        for (id, out) in result.iter_mut().enumerate() {
            if out.reach > 0. {
                out.posterior = Some(masses[id].iter().map(|v| v / out.reach).collect());
                for (w, v) in out.world_values.iter_mut().enumerate() {
                    if masses[id][w] > 0. {
                        *v = Some(sums[id][w] * self.scale);
                    }
                }
                for (_, r) in &mut out.histories {
                    *r /= out.reach;
                }
            }
        }
        Ok(result)
    }
}

struct Response<'a> {
    tree: &'a Tree,
    policy: &'a Policy,
    player: usize,
    cf: Vec<f64>,
    memo: Vec<Option<f64>>,
    chosen: Vec<Option<usize>>,
    visiting: Vec<bool>,
}
impl Response<'_> {
    fn node(&mut self, id: usize) -> Result<f64, Error> {
        if let Some(v) = self.memo[id] {
            return Ok(v);
        }
        let v = match &self.tree.nodes[id] {
            Compiled::Terminal(v) => v / self.tree.scale,
            Compiled::Chance(edges) => {
                let mut v = 0.;
                for &(p, n) in edges {
                    v += p * self.node(n)?;
                }
                v
            }
            Compiled::Decision { info, children } => {
                let i = *info;
                if self.tree.information[i].player != self.player {
                    let mut v = 0.;
                    for (a, &n) in children.iter().enumerate() {
                        v += self.policy[i][a] * self.node(n)?;
                    }
                    v
                } else {
                    if self.chosen[i].is_none() {
                        if self.visiting[i] {
                            return Err(Error("cyclic information dependencies".into()));
                        }
                        self.visiting[i] = true;
                        let mut scores = vec![0.; children.len()];
                        for &member in &self.tree.information[i].nodes {
                            let Compiled::Decision { children, .. } = &self.tree.nodes[member]
                            else {
                                unreachable!()
                            };
                            for (a, &child) in children.iter().enumerate() {
                                scores[a] += self.cf[member] * self.node(child)?;
                            }
                        }
                        let sign = if self.player == 0 { 1. } else { -1. };
                        let mut best = 0;
                        for a in 1..scores.len() {
                            if sign * scores[a] > sign * scores[best] {
                                best = a;
                            }
                        }
                        self.chosen[i] = Some(best);
                        self.visiting[i] = false;
                    }
                    self.node(children[self.chosen[i].unwrap()])?
                }
            }
        };
        self.memo[id] = Some(v);
        Ok(v)
    }
}

#[derive(Clone, Debug)]
pub struct Assessment {
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
    pub gap: f64,
}
#[derive(Clone, Debug)]
pub struct PublicBelief {
    pub id: usize,
    pub key: String,
    pub reach: f64,
    pub posterior: Option<Vec<f64>>,
    pub world_values: Vec<Option<f64>>,
    pub histories: Vec<(usize, f64)>,
}
#[derive(Clone, Debug)]
pub struct Solution {
    pub policy: Policy,
    pub assessment: Assessment,
    pub iterations: usize,
    pub converged: bool,
}

/// Full-tree alternating linear CFR. Each pass freezes all policies, accumulates the
/// counterfactual regret of ALL histories in an information set, then updates that side.
/// The average uses t * own realization reach ONCE per information set, not chance reach
/// or number of member histories. This distinction matters beyond a single matrix game.
pub fn solve(tree: &Tree, config: Config) -> Result<Solution, Error> {
    if config.iterations == 0
        || config.iterations > 10_000_000
        || config.check_every == 0
        || !config.tolerance.is_finite()
        || config.tolerance < 0.
    {
        return Err(Error("invalid CFR configuration".into()));
    }
    let mut policy = tree.uniform();
    let mut regrets: Policy = policy.iter().map(|p| vec![0.; p.len()]).collect();
    let mut sums = regrets.clone();
    let mut total = vec![0.; policy.len()];
    for t in 1..=config.iterations {
        let weight = t as f64 / config.iterations as f64;
        for player in 0..2 {
            let values = tree.values(&policy);
            let cf = tree.reach(&policy, Some(player))?;
            let sign = if player == 0 { 1. } else { -1. };
            for (i, info) in tree.information.iter().enumerate() {
                if info.player == player {
                    for &node in &info.nodes {
                        let Compiled::Decision { children, .. } = &tree.nodes[node] else {
                            unreachable!()
                        };
                        for (a, &child) in children.iter().enumerate() {
                            regrets[i][a] +=
                                weight * cf[node] * sign * (values[child] - values[node]);
                        }
                    }
                }
            }
            for (i, info) in tree.information.iter().enumerate() {
                if info.player == player {
                    strategy(&regrets[i], &mut policy[i]);
                }
            }
        }
        for (i, info) in tree.information.iter().enumerate() {
            let mut own = 1.;
            for &(j, a) in &info.own_sequence {
                own = multiply(own, policy[j][a])?;
            }
            let w = multiply(weight, own)?;
            total[i] += w;
            for (s, &p) in sums[i].iter_mut().zip(&policy[i]) {
                *s += w * p;
            }
        }
        if t % config.check_every == 0 || t == config.iterations {
            let average: Policy = sums
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if total[i] > 0. {
                        p.iter().map(|v| v / total[i]).collect()
                    } else {
                        vec![1. / p.len() as f64; p.len()]
                    }
                })
                .collect();
            let assessment = tree.assess(&average)?;
            let converged = assessment.gap <= config.tolerance;
            if converged || t == config.iterations {
                return Ok(Solution {
                    policy: average,
                    assessment,
                    iterations: t,
                    converged,
                });
            }
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests;
