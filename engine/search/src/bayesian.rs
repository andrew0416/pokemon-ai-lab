//! One-sided private-information, zero-sum Bayesian matrix games.
//!
//! Our policy is SHARED across hidden worlds. The informed opponent has one policy per
//! world and may have different legal menus. Alternating linear CFR is applied to this
//! two-stage extensive form (chance chooses the type, then simultaneous actions).
//! This is not a multi-turn public-belief-state search. In particular, worldwise
//! perfect-information continuation policies must not be advertised as safe leaf values.

pub mod engine;
#[cfg(feature = "experiment-public-belief")]
pub mod tree;

use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Payoffs are from our perspective, row-major over the game's shared row menu.
#[derive(Clone, Debug)]
pub struct World {
    pub id: String,
    pub weight: f64,
    pub columns: Vec<String>,
    pub payoffs: Vec<f64>,
}

/// Validated immutable game. Zero-prior worlds retain their IDs and menu positions.
#[derive(Clone, Debug)]
pub struct Game {
    rows: Vec<String>,
    worlds: Vec<World>,
}

fn labels<'a>(items: impl Iterator<Item = &'a str>, what: &str) -> Result<(), Error> {
    let mut seen = HashSet::new();
    for id in items {
        if id.is_empty() || !seen.insert(id) {
            return Err(Error(format!("empty or duplicate {what}: {id}")));
        }
    }
    if seen.is_empty() {
        return Err(Error(format!("empty {what}")));
    }
    Ok(())
}

fn normalize(weights: &mut [f64]) -> Result<(), Error> {
    if weights.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(Error("weights must be finite and nonnegative".into()));
    }
    let largest = weights.iter().copied().fold(0.0, f64::max);
    if largest == 0.0 {
        return Err(Error("belief has zero total weight".into()));
    }
    let sum: f64 = weights.iter().map(|v| v / largest).sum();
    for v in weights {
        let next = (*v / largest) / sum;
        if *v > 0.0 && next == 0.0 {
            return Err(Error(
                "positive belief underflow; world was not dropped".into(),
            ));
        }
        *v = next;
    }
    Ok(())
}

impl Game {
    pub fn new(rows: Vec<String>, mut worlds: Vec<World>) -> Result<Self, Error> {
        labels(rows.iter().map(String::as_str), "row ID")?;
        labels(worlds.iter().map(|w| w.id.as_str()), "world ID")?;
        for w in &worlds {
            labels(w.columns.iter().map(String::as_str), "column ID")?;
            if rows.len().checked_mul(w.columns.len()) != Some(w.payoffs.len()) {
                return Err(Error(format!(
                    "world {}: payoff dimensions do not match menus",
                    w.id
                )));
            }
            if w.payoffs
                .iter()
                .any(|v| !v.is_finite() || v.abs() > f64::MAX / 8.0)
            {
                return Err(Error(format!(
                    "world {}: nonfinite or unrepresentably large payoff",
                    w.id
                )));
            }
        }
        let mut weights: Vec<_> = worlds.iter().map(|w| w.weight).collect();
        normalize(&mut weights)?;
        for (w, p) in worlds.iter_mut().zip(weights) {
            w.weight = p;
        }
        Ok(Self { rows, worlds })
    }
    pub fn rows(&self) -> &[String] {
        &self.rows
    }
    pub fn worlds(&self) -> &[World] {
        &self.worlds
    }

    /// Bayes update using P(observation | world). Every world ID is required exactly once.
    /// These likelihoods must include the informed player's behavior where applicable;
    /// they are not automatically derived from battle state or an action's mere legality.
    /// A new game starts a fresh CFR solve; regrets are never carried across changed beliefs.
    pub fn conditioned(&self, likelihoods: &[(String, f64)]) -> Result<Self, Error> {
        labels(
            likelihoods.iter().map(|v| v.0.as_str()),
            "likelihood world ID",
        )?;
        if likelihoods.len() != self.worlds.len()
            || likelihoods.iter().any(|(id, p)| {
                !p.is_finite()
                    || !(0.0..=1.0).contains(p)
                    || !self.worlds.iter().any(|w| &w.id == id)
            })
        {
            return Err(Error(
                "likelihoods must cover each world with a probability in [0,1]".into(),
            ));
        }
        let logs: Vec<_> = self
            .worlds
            .iter()
            .map(|w| {
                let p = likelihoods.iter().find(|v| v.0 == w.id).unwrap().1;
                if w.weight == 0.0 || p == 0.0 {
                    f64::NEG_INFINITY
                } else {
                    w.weight.ln() + p.ln()
                }
            })
            .collect();
        let max = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !max.is_finite() {
            return Err(Error(
                "observation contradicts every positive-prior world".into(),
            ));
        }
        let mut worlds = self.worlds.clone();
        for (w, log) in worlds.iter_mut().zip(logs) {
            w.weight = (log - max).exp();
            if log.is_finite() && w.weight == 0.0 {
                return Err(Error(
                    "positive posterior underflow; world was not dropped".into(),
                ));
            }
        }
        Self::new(self.rows.clone(), worlds)
    }

    /// Exact best responses to the supplied policies on THESE FIXED MATRICES.
    /// `lower <= equilibrium value <= upper`; gap is the sum of deviation gains.
    pub fn assess(&self, ours: &[f64], theirs: &[Vec<f64>]) -> Result<Assessment, Error> {
        probability(ours, self.rows.len())?;
        if theirs.len() != self.worlds.len() {
            return Err(Error("opponent policy world count mismatch".into()));
        }
        let mut row_values = vec![0.0; self.rows.len()];
        let mut lower = 0.0;
        let mut value = 0.0;
        let mut world_values = Vec::with_capacity(theirs.len());
        for (w, q) in self.worlds.iter().zip(theirs) {
            probability(q, w.columns.len())?;
            let mut col_values = vec![0.0; q.len()];
            let mut world_value = 0.0;
            for (r, row) in w.payoffs.chunks_exact(q.len()).enumerate() {
                let mut v = 0.0;
                for (c, &a) in row.iter().enumerate() {
                    v += q[c] * a;
                    col_values[c] += ours[r] * a;
                }
                row_values[r] += w.weight * v;
                world_value += ours[r] * v;
            }
            lower += w.weight * col_values.into_iter().fold(f64::INFINITY, f64::min);
            value += w.weight * world_value;
            world_values.push(world_value);
        }
        let upper = row_values.into_iter().fold(f64::NEG_INFINITY, f64::max);
        Ok(Assessment {
            value,
            lower,
            upper,
            gap: (upper - lower).max(0.0),
            world_values,
        })
    }
}

fn probability(policy: &[f64], len: usize) -> Result<(), Error> {
    if policy.len() != len
        || policy
            .iter()
            .any(|p| !p.is_finite() || *p < 0.0 || *p > 1.0)
        || (policy.iter().sum::<f64>() - 1.0).abs() > 1e-10
    {
        return Err(Error("policy must match its menu and sum to one".into()));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub iterations: usize,
    pub tolerance: f64,
    pub check_every: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            iterations: 20_000,
            tolerance: 0.01,
            check_every: 32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Assessment {
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
    pub gap: f64,
    pub world_values: Vec<f64>,
}
#[derive(Clone, Debug)]
pub struct Solution {
    pub ours: Vec<f64>,
    pub theirs: Vec<Vec<f64>>,
    pub assessment: Assessment,
    pub iterations: usize,
    pub converged: bool,
}

fn strategy(regret: &[f64], out: &mut [f64]) {
    let sum: f64 = regret.iter().map(|v| v.max(0.0)).sum();
    let uniform = 1.0 / out.len() as f64;
    for (p, &r) in out.iter_mut().zip(regret) {
        *p = if sum > 0.0 { r.max(0.0) / sum } else { uniform };
    }
}

/// Alternating linear CFR: update our regrets, then each informed opponent's regrets
/// against the updated row policy. Average each policy after its own update with weight t.
/// Regret increments also have weight t (ordinary signed CFR regrets, NOT RM+ clipping).
/// Positive per-type chance reach is divided out of opponent regrets: a fixed positive
/// scalar cannot change regret matching. Zero-prior types retain a uniform policy.
pub fn solve(game: &Game, config: Config) -> Result<Solution, Error> {
    if config.iterations == 0
        || config.iterations > 10_000_000
        || config.check_every == 0
        || !config.tolerance.is_finite()
        || config.tolerance < 0.0
    {
        return Err(Error("need 1..=10000000 iterations, positive check interval and finite nonnegative tolerance".into()));
    }
    // Common positive scale preserves the game, bounds arithmetic, and retains signs.
    let scale = game
        .worlds
        .iter()
        .flat_map(|w| &w.payoffs)
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        .max(f64::MIN_POSITIVE);
    let payoffs: Vec<Vec<f64>> = game
        .worlds
        .iter()
        .map(|w| w.payoffs.iter().map(|v| v / scale).collect())
        .collect();
    let n = game.rows.len();
    let mut p = vec![1.0 / n as f64; n];
    let mut q: Vec<Vec<f64>> = game
        .worlds
        .iter()
        .map(|w| vec![1.0 / w.columns.len() as f64; w.columns.len()])
        .collect();
    let mut row_regret = vec![0.0; n];
    let mut col_regret: Vec<Vec<f64>> = q.iter().map(|q| vec![0.0; q.len()]).collect();
    let mut p_avg = vec![0.0; n];
    let mut q_avg = col_regret.clone();
    let mut row_util = vec![0.0; n];
    let mut col_util = col_regret.clone();
    for t in 1..=config.iterations {
        // t/max_iterations is the same linear weighting up to a common constant.
        let weight = t as f64 / config.iterations as f64;
        row_util.fill(0.0);
        for ((w, a), q) in game.worlds.iter().zip(&payoffs).zip(&q) {
            for (u, row) in row_util.iter_mut().zip(a.chunks_exact(q.len())) {
                *u += w.weight * row.iter().zip(q).map(|(a, q)| a * q).sum::<f64>();
            }
        }
        let v: f64 = p.iter().zip(&row_util).map(|(p, u)| p * u).sum();
        for (r, u) in row_regret.iter_mut().zip(&row_util) {
            *r += weight * (u - v);
        }
        strategy(&row_regret, &mut p);
        for i in 0..q.len() {
            if game.worlds[i].weight == 0.0 {
                continue;
            }
            let util = &mut col_util[i];
            util.fill(0.0);
            for (&p, row) in p.iter().zip(payoffs[i].chunks_exact(q[i].len())) {
                for (u, &a) in util.iter_mut().zip(row) {
                    *u += p * a;
                }
            }
            let v: f64 = q[i].iter().zip(util.iter()).map(|(q, u)| q * u).sum();
            for (r, &u) in col_regret[i].iter_mut().zip(util.iter()) {
                *r += weight * (v - u);
            }
            strategy(&col_regret[i], &mut q[i]);
        }
        let alpha = 2.0 / (t as f64 + 1.0);
        for (avg, &p) in p_avg.iter_mut().zip(&p) {
            *avg += alpha * (p - *avg);
        }
        for (avg, q) in q_avg.iter_mut().zip(&q) {
            for (avg, &q) in avg.iter_mut().zip(q) {
                *avg += alpha * (q - *avg);
            }
        }
        if t % config.check_every == 0 || t == config.iterations {
            let assessment = game.assess(&p_avg, &q_avg)?;
            let converged = assessment.gap <= config.tolerance;
            if converged || t == config.iterations {
                return Ok(Solution {
                    ours: p_avg,
                    theirs: q_avg,
                    assessment,
                    iterations: t,
                    converged,
                });
            }
        }
    }
    unreachable!("positive iteration budget returns at its final iteration")
}

#[cfg(test)]
mod tests;
