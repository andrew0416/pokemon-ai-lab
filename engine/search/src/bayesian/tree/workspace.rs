//! Allocation-only CFR workspace experiment. Every physical node belongs to exactly one
//! reachable tree, so values/reach overwrite each entry before its next read. No clearing
//! or history pruning is needed. Arithmetic and update order match the reference solver.
use super::*;
pub mod cached;

fn values_into(tree: &Tree, policy: &Policy, values: &mut [f64]) {
    for &id in tree.order.iter().rev() {
        values[id] = match &tree.nodes[id] {
            Compiled::Terminal(v) => v / tree.scale,
            Compiled::Chance(edges) => edges.iter().map(|(p, n)| p * values[*n]).sum(),
            Compiled::Decision { info, children } => children
                .iter()
                .zip(&policy[*info])
                .map(|(n, p)| p * values[*n])
                .sum(),
        };
    }
}
fn reach_into(tree: &Tree, policy: &Policy, player: usize, reach: &mut [f64]) -> Result<(), Error> {
    reach[tree.root] = 1.;
    for &id in &tree.order {
        match &tree.nodes[id] {
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
                        if player == tree.information[*info].player {
                            1.
                        } else {
                            policy[*info][a]
                        },
                    )?;
                }
            }
        }
    }
    Ok(())
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
    let mut values = vec![0.; tree.nodes.len()];
    let mut cf = vec![0.; tree.nodes.len()];
    for t in 1..=config.iterations {
        let weight = t as f64 / config.iterations as f64;
        for player in 0..2 {
            values_into(tree, &policy, &mut values);
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
