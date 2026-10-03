//! Opt-in paper-derived solvers. All certificates use the original information tree.
//! The rule, check schedule, policy reuse and matrix kernel are independent settings.
use super::*;
mod compact;
#[cfg(feature = "experiment-growth-pipeline")]
pub(crate) mod pipeline;
pub mod reuse;
mod sequence;
#[cfg(test)]
mod tests;
mod warm;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rule {
    #[default]
    Lcfr,
    Cfr,
    CfrSimultaneous,
    Dcfr,
    PcfrPlus,
    SapcfrPlus,
    HsDcfr15,
    HsDcfr30,
    HsPcfr15,
    HsPcfr30,
}
impl Rule {
    pub fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "lcfr" => Ok(Self::Lcfr),
            "cfr" => Ok(Self::Cfr),
            "cfr-simultaneous" => Ok(Self::CfrSimultaneous),
            "dcfr" => Ok(Self::Dcfr),
            "pcfr+" => Ok(Self::PcfrPlus),
            "sapcfr+" => Ok(Self::SapcfrPlus),
            "hs-dcfr-15" => Ok(Self::HsDcfr15),
            "hs-dcfr-30" => Ok(Self::HsDcfr30),
            "hs-pcfr-15" => Ok(Self::HsPcfr15),
            "hs-pcfr-30" => Ok(Self::HsPcfr30),
            _ => Err(Error("unknown CFR rule".into())),
        }
    }
    fn predictive(self) -> bool {
        matches!(
            self,
            Self::PcfrPlus | Self::SapcfrPlus | Self::HsPcfr15 | Self::HsPcfr30
        )
    }
    // At the beginning of iteration t+1, discount contributions accumulated through t.
    // HS progress t/n uses the fixed configured horizon, never an estimated stop time.
    fn discounts(self, t: usize, n: usize) -> (f64, f64, f64) {
        let time = t as f64;
        let progress = time / n as f64;
        let (alpha, beta, gamma) = match self {
            Self::Lcfr | Self::Cfr | Self::CfrSimultaneous => return (1., 1., 1.),
            Self::Dcfr => (1.5, 0., 2.),
            Self::HsDcfr15 => (1. + 3. * progress, -1. - 2. * progress, 15. - 5. * progress),
            Self::HsDcfr30 => (1. + 3. * progress, -1. - 2. * progress, 30. - 5. * progress),
            Self::HsPcfr15 => (0., 0., 15. - 5. * progress),
            Self::HsPcfr30 => (0., 0., 30. - 5. * progress),
            Self::PcfrPlus | Self::SapcfrPlus => (0., 0., 2.),
        };
        let average = (time / (time + 1.)).powf(gamma);
        if self.predictive() {
            return (1., 1., average);
        }
        if t == 0 {
            return (0., 0., average);
        }
        let p = time.powf(alpha);
        let q = time.powf(beta);
        (p / (p + 1.), q / (q + 1.), average)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Checks {
    #[default]
    Periodic,
    Geometric,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub rule: Rule,
    pub checks: Checks,
    pub compact: bool,
    pub sequence: bool,
    /// Check a previous average strategy in the NEW game; reset on failure.
    pub reuse_policy: bool,
    /// Brown/Sandholm substitute-regret initialization, only for simultaneous CFR.
    /// Zero disables it. This is virtual history, never counted as executed iterations.
    pub warm_iterations: usize,
}
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub assessments: usize,
    pub compact: bool,
    pub matrix_entries: usize,
    pub sequence: bool,
    pub sequence_entries: usize,
    pub scalar_reach_fallbacks: usize,
    pub mapped_information: usize,
    pub reuse_attempted: bool,
    pub reused: bool,
    pub warm_attempted: bool,
    pub warm_applied: bool,
    pub virtual_iterations: usize,
    pub substitute_root_sum: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Run {
    pub solution: Solution,
    pub stats: Stats,
}

pub fn solve(tree: &Tree, config: Config, settings: Settings) -> Result<Run, Error> {
    solve_from(tree, config, settings, None)
}

pub fn solve_from(
    tree: &Tree,
    config: Config,
    settings: Settings,
    previous: Option<&reuse::Snapshot>,
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
    let mut sequence = if matrix.is_none() && settings.sequence {
        sequence::Kernel::new(tree)
    } else {
        None
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
