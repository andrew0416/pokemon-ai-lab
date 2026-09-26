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

use crate::state::SideId;

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
    /// Which damage rolls the enumeration branches on (F18).
    roll_mode: RollMode,
}

/// How [`Chooser::roll`] treats the 16 damage rolls (WORKPLAN F18). Everything else (critical
/// hits, accuracy, secondary effects, Speed ties) stays exact in every mode. Sampling ignores
/// the mode and draws from all 16.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RollMode {
    /// All 16 rolls (exact distribution).
    #[default]
    Full,
    /// The minimum and the maximum roll, 1/2 each: the oracle's `--mode extremes`. The
    /// support of the distribution is right at the ends (a worst-case search sees the true
    /// worst roll as long as the value is monotone in damage); probabilities are approximate.
    Extremes,
    /// Rolls 85, 90, 95 and 100 (indices 0, 5, 10, 15), 1/4 each: the exact mean multiplier
    /// 92.5 with four support points; probabilities are approximate.
    Quartiles,
    /// One roll, 92% (index 7): no roll branching at all; a "typical" line.
    Median,
    /// One roll: the minimum for every attack by `side` and the maximum for every attack
    /// against it. The user's "최저난수 보장" criterion: a line that works here works under
    /// every roll (accuracy, critical hits and secondary effects stay probabilistic).
    Pessimistic(SideId),
}

impl RollMode {
    /// The roll indices (into the ascending 16-roll table) the mode branches on for an
    /// attack by `attacker`.
    pub fn indices(self, attacker: SideId) -> &'static [usize] {
        match self {
            RollMode::Full => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            RollMode::Extremes => &[0, 15],
            RollMode::Quartiles => &[0, 5, 10, 15],
            RollMode::Median => &[7],
            RollMode::Pessimistic(side) if side == attacker => &[0],
            RollMode::Pessimistic(_) => &[15],
        }
    }

    /// Whether the mode enumerates the exact distribution.
    pub fn is_exact(self) -> bool {
        self == RollMode::Full
    }
}

impl Chooser {
    pub fn new() -> Chooser {
        Chooser {
            prefix: Vec::new(),
            trace: Vec::new(),
            probability: 1.0,
            random: None,
            roll_mode: RollMode::Full,
        }
    }

    /// An enumerating chooser that branches on the damage rolls `mode` selects.
    pub fn with_rolls(mode: RollMode) -> Chooser {
        Chooser {
            roll_mode: mode,
            ..Chooser::new()
        }
    }

    /// One damage roll out of `rolls` (ascending, 85% first) for an attack by `attacker`: in
    /// enumeration one branch per distinct value among the rolls the mode selects, weighted
    /// by multiplicity; in sampling one of the 16 uniformly.
    pub fn roll(
        &mut self,
        rolls: &[u16; crate::damage::DAMAGE_ROLL_COUNT],
        attacker: SideId,
    ) -> u16 {
        let indices = if self.random.is_some() {
            RollMode::Full.indices(attacker)
        } else {
            self.roll_mode.indices(attacker)
        };
        let mut values: Vec<(u16, u32)> = Vec::with_capacity(indices.len());
        for &i in indices {
            let r = rolls[i];
            match values.iter_mut().find(|(v, _)| *v == r) {
                Some((_, count)) => *count += 1,
                None => values.push((r, 1)),
            }
        }
        if values.len() == 1 {
            return values[0].0;
        }
        let total = indices.len() as f64;
        let weights: Vec<f64> = values.iter().map(|&(_, c)| f64::from(c) / total).collect();
        values[self.weighted(&weights)].0
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

    /// Every roll mode branches once per distinct value it selects, with the right weights,
    /// and sampling ignores the mode.
    #[test]
    fn roll_modes_branch_on_their_indices() {
        let rolls: [u16; 16] = std::array::from_fn(|i| 85 + i as u16);
        for (mode, expected) in [
            (
                RollMode::Full,
                (0..16).map(|i| 85 + i).collect::<Vec<u16>>(),
            ),
            (RollMode::Extremes, vec![85, 100]),
            (RollMode::Quartiles, vec![85, 90, 95, 100]),
            (RollMode::Median, vec![92]),
            (RollMode::Pessimistic(SideId::One), vec![85]),
            (RollMode::Pessimistic(SideId::Two), vec![100]),
        ] {
            let mut chooser = Chooser::with_rolls(mode);
            let mut seen = Vec::new();
            let mut total = 0.0;
            loop {
                chooser.begin_run();
                seen.push(chooser.roll(&rolls, SideId::One));
                total += chooser.probability();
                if !chooser.advance() {
                    break;
                }
            }
            assert_eq!(seen, expected, "{mode:?}");
            assert!((total - 1.0).abs() < 1e-12, "{mode:?}: {total}");
        }
        // Equal rolls merge (weights add up).
        let flat = [50u16; 16];
        let mut chooser = Chooser::with_rolls(RollMode::Extremes);
        chooser.begin_run();
        assert_eq!(chooser.roll(&flat, SideId::One), 50);
        assert!(!chooser.advance());
        // Pessimistic: the other side's attacks take the maximum roll.
        let mut chooser = Chooser::with_rolls(RollMode::Pessimistic(SideId::One));
        chooser.begin_run();
        assert_eq!(chooser.roll(&rolls, SideId::Two), 100);
        assert!(!chooser.advance());
        // A sampler draws from all 16 whatever the mode.
        let mut sampler = Chooser::sampler(7);
        sampler.roll_mode = RollMode::Extremes;
        let mut values = std::collections::HashSet::new();
        for _ in 0..500 {
            sampler.begin_run();
            values.insert(sampler.roll(&rolls, SideId::One));
        }
        assert!(values.len() > 2, "{values:?}");
    }
}
