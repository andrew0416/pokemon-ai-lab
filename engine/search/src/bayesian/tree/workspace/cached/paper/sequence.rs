//! Sparse sequence-form payoff contraction for perfect-recall trees.
//! Terminal histories with the same LAST own-action sequence pair contribute to
//! one chance-weighted coefficient. Information sets and realization constraints
//! remain distinct. No observation/history abstraction or terminal sampling.
use super::*;
pub(super) struct Kernel {
    offsets: Vec<usize>,
    parent: Vec<usize>,
    counts: [usize; 2],
    order: Vec<usize>,
    entries: Vec<(usize, usize, f64)>,
    min_chance: f64,
    realization: Vec<f64>,
    gradient: Vec<f64>,
    safe: bool,
}
impl Kernel {
    pub(super) fn new(tree: &Tree) -> Option<Self> {
        let mut offsets = vec![0; tree.information.len()];
        let mut counts = [1usize; 2];
        for (i, info) in tree.information.iter().enumerate() {
            offsets[i] = counts[info.player];
            counts[info.player] = counts[info.player].checked_add(info.actions.len())?;
        }
        if counts.iter().sum::<usize>() > 1_048_576 {
            return None;
        }
        let parent: Vec<_> = tree
            .information
            .iter()
            .map(|i| i.own_sequence.last().map_or(0, |&(j, a)| offsets[j] + a))
            .collect();
        let mut order: Vec<_> = (0..tree.information.len()).collect();
        order.sort_by_key(|&i| tree.information[i].own_sequence.len());
        let mut coefficients = std::collections::BTreeMap::<(usize, usize), f64>::new();
        let mut min_chance = 1f64;
        let mut stack = vec![(tree.root, [0usize; 2], 1.)];
        while let Some((n, seq, chance)) = stack.pop() {
            match &tree.nodes[n] {
                Compiled::Terminal(value) => {
                    *coefficients.entry((seq[0], seq[1])).or_default() +=
                        chance * (value / tree.scale);
                    if chance > 0. {
                        min_chance = min_chance.min(chance);
                    }
                    if coefficients.len() > 1_048_576 {
                        return None;
                    }
                }
                Compiled::Chance(edges) => {
                    for &(p, c) in edges.iter().rev() {
                        stack.push((c, seq, multiply(chance, p).ok()?));
                    }
                }
                Compiled::Decision { info, children } => {
                    let player = tree.information[*info].player;
                    if seq[player] != parent[*info] {
                        return None;
                    }
                    for (a, &c) in children.iter().enumerate().rev() {
                        let mut next = seq;
                        next[player] = offsets[*info] + a;
                        stack.push((c, next, chance));
                    }
                }
            }
        }
        if coefficients.values().any(|v| !v.is_finite()) {
            return None;
        }
        Some(Self {
            offsets,
            parent,
            counts,
            order,
            entries: coefficients
                .into_iter()
                .map(|((a, b), v)| (a, b, v))
                .collect(),
            min_chance,
            realization: vec![0.; counts[0].max(counts[1])],
            gradient: vec![0.; counts[0].max(counts[1])],
            safe: false,
        })
    }
    pub(super) fn entries(&self) -> usize {
        self.entries.len()
    }
    // Unsafe tiny products use the original per-history pass for this update.
    pub(super) fn prepare(&mut self, tree: &Tree, policy: &Policy, player: usize) -> bool {
        let opponent = 1 - player;
        self.realization[..self.counts[opponent]].fill(0.);
        self.realization[0] = 1.;
        self.safe = true;
        let mut minimum = 1f64;
        for &i in &self.order {
            if tree.information[i].player != opponent {
                continue;
            }
            let reach = self.realization[self.parent[i]];
            for (a, &p) in policy[i].iter().enumerate() {
                let next = reach * p;
                if reach > 0. && p > 0. && next == 0. {
                    self.safe = false;
                }
                self.realization[self.offsets[i] + a] = next;
                if next > 0. {
                    minimum = minimum.min(next);
                }
            }
        }
        self.safe &= self.min_chance * minimum > 0.;
        self.safe
    }
    pub(super) fn accumulate(
        &mut self,
        tree: &Tree,
        policy: &Policy,
        player: usize,
        regrets: &mut Policy,
        weight: f64,
    ) {
        debug_assert!(self.safe);
        self.gradient[..self.counts[player]].fill(0.);
        for &(a, b, v) in &self.entries {
            let (own, other) = if player == 0 { (a, b) } else { (b, a) };
            self.gradient[own] += v * self.realization[other];
        }
        let sign = if player == 0 { 1. } else { -1. };
        for &i in self.order.iter().rev() {
            if tree.information[i].player != player {
                continue;
            }
            let start = self.offsets[i];
            let value: f64 = policy[i]
                .iter()
                .enumerate()
                .map(|(a, p)| p * self.gradient[start + a])
                .sum();
            for (a, r) in regrets[i].iter_mut().enumerate() {
                *r += weight * sign * (self.gradient[start + a] - value);
            }
            self.gradient[self.parent[i]] += value;
        }
    }
}
