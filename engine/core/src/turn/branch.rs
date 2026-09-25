//! Exhaustive enumeration of a turn's random decisions by replay.
//!
//! The turn code is written straight-line, like Showdown's, and asks [`Chooser`] whenever
//! something is random. One *run* executes the whole turn once, following a recorded prefix
//! of choices and taking the first option at every new decision point. After a run the
//! odometer advances to the next unexplored choice sequence, and the turn runs again from
//! the start. This is what `oracle/enumerate.cjs` does to Showdown, but over decisions the
//! engine models directly (a damage roll is one decision over the distinct damage values,
//! not a draw from Showdown's PRNG).
//!
//! Runs must be deterministic given the prefix, so the turn code may not branch on anything
//! but the state and earlier choices.

#[derive(Debug, Default)]
pub(crate) struct Chooser {
    /// Choices to replay at the start of the next run.
    prefix: Vec<u8>,
    /// `(choice, option count)` of every decision in the current run.
    trace: Vec<(u8, u8)>,
    probability: f64,
    /// Sampling mode (xorshift64* state): every decision is drawn at random instead of
    /// enumerated, and [`Chooser::advance`] ends after one run.
    random: Option<u64>,
}

impl Chooser {
    pub fn new() -> Chooser {
        Chooser {
            prefix: Vec::new(),
            trace: Vec::new(),
            probability: 1.0,
            random: None,
        }
    }

    /// A chooser that samples one path per run (Monte Carlo) from a nonzero seed.
    pub fn sampler(seed: u64) -> Chooser {
        Chooser {
            random: Some(seed.max(1)),
            ..Chooser::new()
        }
    }

    /// Uniform in [0, 1).
    fn draw(&mut self) -> f64 {
        let x = self.random.as_mut().expect("sampling mode");
        *x ^= *x >> 12;
        *x ^= *x << 25;
        *x ^= *x >> 27;
        let bits = x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        bits as f64 / (1u64 << 53) as f64
    }

    pub fn begin_run(&mut self) {
        self.trace.clear();
        self.probability = 1.0;
    }

    /// Probability of the current run's choices so far.
    pub fn probability(&self) -> f64 {
        self.probability
    }

    /// Moves to the next unexplored choice sequence. `false` once every run has been made.
    pub fn advance(&mut self) -> bool {
        if self.random.is_some() {
            return false;
        }
        while let Some((choice, count)) = self.trace.pop() {
            if choice + 1 < count {
                self.prefix.clear();
                self.prefix.extend(self.trace.iter().map(|&(c, _)| c));
                self.prefix.push(choice + 1);
                return true;
            }
        }
        false
    }

    fn decide(&mut self, count: usize) -> usize {
        debug_assert!((2..=u8::MAX as usize).contains(&count));
        let depth = self.trace.len();
        let choice = self.prefix.get(depth).copied().unwrap_or(0);
        assert!(
            (choice as usize) < count,
            "turn replay diverged at decision {depth}"
        );
        self.trace.push((choice, count as u8));
        choice as usize
    }

    /// One of `weights.len()` options; weights must be positive and sum to 1.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        if weights.len() <= 1 {
            return 0;
        }
        if self.random.is_some() {
            let mut u = self.draw();
            for (i, &w) in weights.iter().enumerate() {
                if u < w {
                    return i;
                }
                u -= w;
            }
            return weights.len() - 1;
        }
        let choice = self.decide(weights.len());
        self.probability *= weights[choice];
        choice
    }

    /// Uniform over `count` options.
    pub fn uniform(&mut self, count: usize) -> usize {
        if count <= 1 {
            return 0;
        }
        if self.random.is_some() {
            return ((self.draw() * count as f64) as usize).min(count - 1);
        }
        let choice = self.decide(count);
        self.probability /= count as f64;
        choice
    }

    /// Showdown `randomChance(numerator, denominator)`.
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        if numerator >= denominator {
            return true;
        }
        if numerator == 0 {
            return false;
        }
        let p = f64::from(numerator) / f64::from(denominator);
        self.weighted(&[p, 1.0 - p]) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_every_sequence_once_with_its_probability() {
        let mut chooser = Chooser::new();
        let mut runs = Vec::new();
        loop {
            chooser.begin_run();
            let a = chooser.chance(1, 4);
            // The second decision only exists on one branch.
            let b = if a { chooser.uniform(3) } else { 9 };
            runs.push((a, b, chooser.probability()));
            if !chooser.advance() {
                break;
            }
        }
        assert_eq!(
            runs,
            vec![
                (true, 0, 0.25 / 3.0),
                (true, 1, 0.25 / 3.0),
                (true, 2, 0.25 / 3.0),
                (false, 9, 0.75),
            ]
        );
    }
}
