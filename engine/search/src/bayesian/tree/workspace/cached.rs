//! Policy-independent value backups, rebuilt for each fixed compiled tree.
//! Chance outcomes and terminal evaluations are never pruned. Only values whose entire
//! subtree contains no decision node are constant. Every arithmetic reduction preserves
//! the reference child order. Reach/underflow checks and final assessment are unchanged.
use super::*;

struct CachedValues {
    values: Vec<f64>,
    dynamic: Vec<usize>,
}
impl CachedValues {
    fn new(tree: &Tree) -> Self {
        let mut values = vec![0.; tree.nodes.len()];
        let mut fixed = vec![false; tree.nodes.len()];
        let mut dynamic = Vec::new();
        for &id in tree.order.iter().rev() {
            match &tree.nodes[id] {
                Compiled::Terminal(v) => {
                    values[id] = v / tree.scale;
                    fixed[id] = true;
                }
                Compiled::Chance(edges) if edges.iter().all(|(_, n)| fixed[*n]) => {
                    values[id] = edges.iter().map(|(p, n)| p * values[*n]).sum();
                    fixed[id] = true;
                }
                _ => dynamic.push(id),
            }
        }
        Self { values, dynamic }
    }
    fn update(&mut self, tree: &Tree, policy: &Policy) {
        for &id in &self.dynamic {
            self.values[id] = match &tree.nodes[id] {
                Compiled::Chance(edges) => edges.iter().map(|(p, n)| p * self.values[*n]).sum(),
                Compiled::Decision { info, children } => children
                    .iter()
                    .zip(&policy[*info])
                    .map(|(n, p)| p * self.values[*n])
                    .sum(),
                Compiled::Terminal(_) => unreachable!(),
            };
        }
    }
}

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
    let mut cache = CachedValues::new(tree);
    let mut cf = vec![0.; tree.nodes.len()];
    for t in 1..=config.iterations {
        let weight = t as f64 / config.iterations as f64;
        for player in 0..2 {
            cache.update(tree, &policy);
            let values = &cache.values;
            reach_into(tree, &policy, player, &mut cf)?;
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
