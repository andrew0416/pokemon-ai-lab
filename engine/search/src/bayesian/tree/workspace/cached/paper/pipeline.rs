use super::*;
#[cfg(test)]
mod tests;
use crate::bayesian::tree::{
    compiler::owned::Delta,
    pipeline::{Metrics, Settings as PipelineSettings},
};
#[derive(Default)]
pub(crate) struct Context {
    sequence: sequence::incremental::Cache,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_from(
    tree: &Tree,
    config: Config,
    settings: Settings,
    previous: Option<&reuse::Snapshot>,
    pipeline: PipelineSettings,
    compiler_delta: Option<&Delta>,
    context: &mut Context,
    metrics: &mut Metrics,
) -> Result<Run, Error> {
    #[cfg(feature = "experiment-phase-cost")]
    let _solver = crate::bayesian::tree::phase_cost::Span::new(
        crate::bayesian::tree::phase_cost::Phase::Solver,
    );
    if config.iterations == 0
        || config.iterations > 10_000_000
        || config.check_every == 0
        || !config.tolerance.is_finite()
        || config.tolerance < 0.
    {
        return Err(Error("invalid CFR configuration".into()));
    }
    if settings.warm_iterations > 1_000_000
        || (settings.warm_iterations > 0 && settings.rule != Rule::CfrSimultaneous)
    {
        return Err(Error(
            "warm initialization requires simultaneous CFR and at most 1000000 virtual iterations"
                .into(),
        ));
    }
    crate::bayesian::tree::pipeline::validate(pipeline, settings)?;
    let mut stats = Stats::default();
    let mut seed = None;
    if settings.reuse_policy || settings.warm_iterations > 0 {
        if let Some(previous) = previous {
            let (policy, mapped) = previous.project(tree)?;
            stats.mapped_information = mapped;
            // A completely unrelated seed cannot save work; use the reference start.
            if mapped > 0 {
                if settings.reuse_policy {
                    stats.reuse_attempted = true;
                    let assessment = {
                        #[cfg(feature = "experiment-phase-cost")]
                        let _certificate = crate::bayesian::tree::phase_cost::Span::new(
                            crate::bayesian::tree::phase_cost::Phase::Certificate,
                        );
                        tree.assess(&policy)?
                    };
                    stats.assessments += 1;
                    if assessment.gap <= config.tolerance {
                        stats.reused = true;
                        return Ok(Run {
                            solution: Solution {
                                policy,
                                assessment,
                                iterations: 0,
                                converged: true,
                            },
                            stats,
                        });
                    }
                }
                seed = Some(policy);
            }
        }
    }
    let mut policy = tree.uniform();
    let mut regrets: Policy = policy.iter().map(|p| vec![0.; p.len()]).collect();
    let mut delta = regrets.clone();
    let mut sums = regrets.clone();
    let mut total = vec![0.; policy.len()];
    if let Some(seed) = seed.filter(|_| settings.warm_iterations > 0) {
        stats.warm_attempted = true;
        if let Some(initial) = warm::initialize(tree, &seed, settings.warm_iterations)? {
            stats.warm_applied = true;
            stats.virtual_iterations = settings.warm_iterations;
            stats.substitute_root_sum = Some(initial.root_sum);
            regrets = initial.regrets;
            sums = initial.sums;
            total = initial.total;
            for (r, p) in regrets.iter().zip(&mut policy) {
                strategy(r, p);
            }
        }
    }
    #[cfg(feature = "experiment-phase-cost")]
    let kernel_phase = crate::bayesian::tree::phase_cost::Span::new(
        crate::bayesian::tree::phase_cost::Phase::Kernel,
    );
    let mut matrix = if settings.compact {
        compact::Matrix::new(tree)
    } else {
        None
    };
    stats.compact = matrix.is_some();
    stats.matrix_entries = matrix.as_ref().map_or(0, |m| m.entries());
    let mut local_sequence =
        if matrix.is_none() && settings.sequence && !pipeline.incremental_sequence {
            sequence::Kernel::new(tree)
        } else {
            None
        };
    let mut sequence = if matrix.is_none() && settings.sequence && pipeline.incremental_sequence {
        context.sequence.update(tree, compiler_delta, metrics);
        context.sequence.kernel.as_mut()
    } else {
        local_sequence.as_mut()
    };
    stats.sequence = sequence.is_some();
    stats.sequence_entries = sequence.as_ref().map_or(0, |k| k.entries());
    #[cfg(feature = "experiment-phase-cost")]
    drop(kernel_phase);
    // Do not allocate/initialize the full value workspace if matrix compilation succeeds.
    let mut cache = if matrix.is_none() && sequence.is_none() {
        Some(CachedValues::new(tree))
    } else {
        None
    };
    let mut cf = vec![
        0.;
        if matrix.is_none() && sequence.is_none() {
            tree.nodes.len()
        } else {
            0
        }
    ];
    for t in 1..=config.iterations {
        #[cfg(feature = "experiment-phase-cost")]
        let _cfr = crate::bayesian::tree::phase_cost::Span::new(
            crate::bayesian::tree::phase_cost::Phase::Cfr,
        );
        let (positive, negative, average_discount) =
            settings.rule.discounts(t - 1, config.iterations);
        let weight = if settings.rule == Rule::Lcfr {
            t as f64 / config.iterations as f64
        } else {
            1.
        };
        let simultaneous = settings.rule == Rule::CfrSimultaneous;
        if simultaneous {
            warm::average(tree, &policy, &mut sums, &mut total, 1.)?;
        }
        for player in 0..2 {
            if settings.rule.predictive() {
                for (i, info) in tree.information.iter().enumerate() {
                    if info.player == player {
                        delta[i].fill(0.);
                    }
                }
            } else if !matches!(
                settings.rule,
                Rule::Lcfr | Rule::Cfr | Rule::CfrSimultaneous
            ) {
                for (i, info) in tree.information.iter().enumerate() {
                    if info.player == player {
                        for r in &mut regrets[i] {
                            *r *= if *r > 0. { positive } else { negative };
                        }
                    }
                }
            }
            let target = if settings.rule.predictive() {
                &mut delta
            } else {
                &mut regrets
            };
            if let Some(m) = matrix.as_mut() {
                if !m.reach_safe(&policy, player) {
                    // Conservative lower bound could underflow even if actual reaches do
                    // not. Preserve the exact original error semantics by checking them.
                    let _ = tree.reach(&policy, Some(player))?;
                    stats.scalar_reach_fallbacks += 1;
                }
                m.accumulate(&policy, player, target, weight);
            } else if sequence
                .as_mut()
                .is_some_and(|k| k.prepare(tree, &policy, player))
            {
                sequence
                    .as_mut()
                    .unwrap()
                    .accumulate(tree, &policy, player, target, weight);
            } else {
                if sequence.is_some() {
                    stats.scalar_reach_fallbacks += 1;
                }
                let cache = cache.get_or_insert_with(|| CachedValues::new(tree));
                if cf.len() != tree.nodes.len() {
                    cf.resize(tree.nodes.len(), 0.);
                }
                cache.update(tree, &policy);
                reach_into(tree, &policy, player, &mut cf)?;
                let sign = if player == 0 { 1. } else { -1. };
                for (i, info) in tree.information.iter().enumerate() {
                    if info.player == player {
                        for &node in &info.nodes {
                            let Compiled::Decision { children, .. } = &tree.nodes[node] else {
                                unreachable!()
                            };
                            for (a, &child) in children.iter().enumerate() {
                                // LCFR retains reference per-history arithmetic order.
                                target[i][a] += weight
                                    * cf[node]
                                    * sign
                                    * (cache.values[child] - cache.values[node]);
                            }
                        }
                    }
                }
            }
            for (i, info) in tree.information.iter().enumerate() {
                if info.player != player {
                    continue;
                }
                if settings.rule.predictive() {
                    let prediction = if settings.rule == Rule::SapcfrPlus {
                        1. / 3.
                    } else {
                        1.
                    };
                    for ((r, d), p) in regrets[i].iter_mut().zip(&delta[i]).zip(&mut policy[i]) {
                        *r = (*r + d).max(0.);
                        *p = (*r + prediction * d).max(0.);
                    }
                    let sum: f64 = policy[i].iter().sum();
                    let uniform = 1. / policy[i].len() as f64;
                    for p in &mut policy[i] {
                        *p = if sum > 0. { *p / sum } else { uniform };
                    }
                } else if !simultaneous {
                    strategy(&regrets[i], &mut policy[i]);
                }
            }
        }
        if simultaneous {
            for (r, p) in regrets.iter().zip(&mut policy) {
                strategy(r, p);
            }
        }
        for (i, info) in tree.information.iter().enumerate() {
            if simultaneous {
                break;
            }
            let mut own = 1.;
            for &(j, a) in &info.own_sequence {
                own = multiply(own, policy[j][a])?;
            }
            let w = multiply(weight, own)?;
            if !matches!(settings.rule, Rule::Lcfr | Rule::Cfr) {
                total[i] *= average_discount;
                for s in &mut sums[i] {
                    *s *= average_discount;
                }
            }
            total[i] += w;
            for (s, &p) in sums[i].iter_mut().zip(&policy[i]) {
                *s += w * p;
            }
        }
        let check = match settings.checks {
            Checks::Periodic => t % config.check_every == 0,
            Checks::Geometric => t.is_power_of_two(),
        };
        if check || t == config.iterations {
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
            if pipeline.compressed_checks && t != config.iterations {
                if let Some(kernel) = sequence.as_mut() {
                    metrics.compressed_checks += 1;
                    let filtered_gap = {
                        #[cfg(feature = "experiment-phase-cost")]
                        let _filter = crate::bayesian::tree::phase_cost::Span::new(
                            crate::bayesian::tree::phase_cost::Phase::CompressedFilter,
                        );
                        kernel.gap(tree, &average)
                    };
                    if let Some(gap) = filtered_gap {
                        // Near the threshold, prefer the reference's floating-point
                        // result. This is a filter, not a published error bound.
                        let margin =
                            64. * f64::EPSILON * tree.scale * (1. + tree.information.len() as f64);
                        if gap > config.tolerance + margin {
                            metrics.compressed_rejections += 1;
                            continue;
                        }
                    }
                }
            }
            let assessment = {
                #[cfg(feature = "experiment-phase-cost")]
                let _certificate = crate::bayesian::tree::phase_cost::Span::new(
                    crate::bayesian::tree::phase_cost::Phase::Certificate,
                );
                tree.assess(&average)?
            };
            stats.assessments += 1;
            let converged = assessment.gap <= config.tolerance;
            if converged || t == config.iterations {
                return Ok(Run {
                    solution: Solution {
                        policy: average,
                        assessment,
                        iterations: t,
                        converged,
                    },
                    stats,
                });
            }
        }
    }
    unreachable!()
}
