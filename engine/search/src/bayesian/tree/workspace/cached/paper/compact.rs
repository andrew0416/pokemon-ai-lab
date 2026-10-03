//! Exact mathematical normal-form specialization, never state/history merging.
//! Both players must choose exactly once on every root-to-leaf path and each must
//! have one information set. Original tree and scale remain the certificate oracle.
use super::*;
pub(super) struct Matrix {
    info: [usize; 2],
    rows: usize,
    cols: usize,
    payoffs: Vec<f64>,
    min_chance: f64,
    action_values: Vec<f64>,
}
impl Matrix {
    pub(super) fn new(tree: &Tree) -> Option<Self> {
        if tree.information.len() != 2 {
            return None;
        }
        let info = [
            tree.information.iter().position(|i| i.player == 0)?,
            tree.information.iter().position(|i| i.player == 1)?,
        ];
        if tree.information.iter().any(|i| !i.own_sequence.is_empty()) {
            return None;
        }
        let rows = tree.information[info[0]].actions.len();
        let cols = tree.information[info[1]].actions.len();
        let len = rows.checked_mul(cols)?;
        // Explicit bounded memory; large/unusual menus keep the general scalar path.
        if len > 1_048_576 {
            return None;
        }
        let mut payoffs = vec![0.; len];
        let mut min_chance = 1f64;
        let mut stack = vec![(tree.root, [None, None], 1.)];
        while let Some((node, chosen, chance)) = stack.pop() {
            match &tree.nodes[node] {
                Compiled::Terminal(value) => {
                    let row = chosen[0]?;
                    let col = chosen[1]?;
                    payoffs[row * cols + col] += chance * (*value / tree.scale);
                    if chance > 0. {
                        min_chance = min_chance.min(chance);
                    }
                }
                Compiled::Chance(edges) => {
                    for &(p, n) in edges.iter().rev() {
                        // Do not introduce an unconditional underflow error when a policy
                        // might make this history unreachable. Fall back to the original.
                        stack.push((n, chosen, multiply(chance, p).ok()?));
                    }
                }
                Compiled::Decision { info: i, children } => {
                    let player = tree.information[*i].player;
                    if chosen[player].is_some() || *i != info[player] {
                        return None;
                    }
                    for (a, &child) in children.iter().enumerate().rev() {
                        let mut next = chosen;
                        next[player] = Some(a);
                        stack.push((child, next, chance));
                    }
                }
            }
        }
        if payoffs.iter().any(|v| !v.is_finite()) {
            return None;
        }
        Some(Self {
            info,
            rows,
            cols,
            payoffs,
            min_chance,
            action_values: vec![0.; rows.max(cols)],
        })
    }
    pub(super) fn entries(&self) -> usize {
        self.payoffs.len()
    }
    pub(super) fn reach_safe(&self, policy: &Policy, player: usize) -> bool {
        let minimum = policy[self.info[1 - player]]
            .iter()
            .copied()
            .filter(|p| *p > 0.)
            .fold(1., f64::min);
        self.min_chance * minimum > 0.
    }
    pub(super) fn accumulate(
        &mut self,
        policy: &Policy,
        player: usize,
        regrets: &mut Policy,
        weight: f64,
    ) {
        let p = &policy[self.info[0]];
        let q = &policy[self.info[1]];
        let (n, sign) = if player == 0 {
            (self.rows, 1.)
        } else {
            (self.cols, -1.)
        };
        for a in 0..n {
            self.action_values[a] = if player == 0 {
                (0..self.cols)
                    .map(|c| self.payoffs[a * self.cols + c] * q[c])
                    .sum()
            } else {
                (0..self.rows)
                    .map(|r| self.payoffs[r * self.cols + a] * p[r])
                    .sum()
            };
        }
        let value: f64 = self.action_values[..n]
            .iter()
            .zip(&policy[self.info[player]])
            .map(|(v, p)| v * p)
            .sum();
        for (r, v) in regrets[self.info[player]]
            .iter_mut()
            .zip(&self.action_values[..n])
        {
            *r += weight * sign * (v - value);
        }
    }
}
