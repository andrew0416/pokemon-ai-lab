//! Zero-sum matrix games: the one-turn game between our choices (rows) and theirs (columns)
//! with the chance values as payoffs. Pure maximin ([`crate::solve`]) is exploitable and
//! usually reports every line as "the opponent can always answer it"; the mixed equilibrium
//! gives the value of the turn when neither side can be read, and a strategy (a probability
//! per choice) that holds it. Solved by regret matching plus (RM+) with strategy averaging,
//! which converges to a Nash equilibrium in two-player zero-sum games.

/// A row-player-maximizes payoff matrix, `rows × cols`, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<f32>,
}

impl Matrix {
    pub fn new(rows: usize, cols: usize, values: Vec<f32>) -> Matrix {
        assert_eq!(values.len(), rows * cols);
        Matrix { rows, cols, values }
    }

    pub fn at(&self, row: usize, col: usize) -> f32 {
        self.values[row * self.cols + col]
    }

    /// The pure maximin value and row: `max_r min_c`.
    pub fn maximin(&self) -> (usize, f32) {
        (0..self.rows)
            .map(|r| {
                let worst = (0..self.cols)
                    .map(|c| self.at(r, c))
                    .fold(f32::INFINITY, f32::min);
                (r, worst)
            })
            .fold(
                (0, f32::NEG_INFINITY),
                |best, x| {
                    if x.1 > best.1 {
                        x
                    } else {
                        best
                    }
                },
            )
    }
}

/// A mixed-strategy solution.
#[derive(Clone, Debug, PartialEq)]
pub struct Equilibrium {
    /// Row player's (our) strategy, one probability per row.
    pub rows: Vec<f32>,
    /// Column player's (their) strategy.
    pub cols: Vec<f32>,
    /// The game value under the two average strategies (row player's expected payoff).
    pub value: f32,
    /// How far from equilibrium the average strategies are: the sum of both players' best
    /// deviation gains (0 at an exact equilibrium).
    pub exploitability: f32,
    pub iterations: usize,
}

/// Solves `matrix` by RM+ (Tammelin 2014) with linear averaging, stopping after
/// `max_iterations` or once the exploitability falls under `tolerance`.
pub fn solve(matrix: &Matrix, max_iterations: usize, tolerance: f32) -> Equilibrium {
    let (n, m) = (matrix.rows, matrix.cols);
    assert!(n > 0 && m > 0, "an empty game");
    let mut row_regret = vec![0.0f64; n];
    let mut col_regret = vec![0.0f64; m];
    let mut row_sum = vec![0.0f64; n];
    let mut col_sum = vec![0.0f64; m];
    let mut iterations = 0;
    let mut result = None;
    for t in 1..=max_iterations.max(1) {
        iterations = t;
        let rows = strategy(&row_regret);
        let cols = strategy(&col_regret);
        // Expected payoffs of each pure action against the other player's current strategy.
        let mut row_util = vec![0.0f64; n];
        let mut col_util = vec![0.0f64; m];
        for r in 0..n {
            for c in 0..m {
                let v = f64::from(matrix.at(r, c));
                row_util[r] += cols[c] * v;
                col_util[c] += rows[r] * v;
            }
        }
        let row_value: f64 = (0..n).map(|r| rows[r] * row_util[r]).sum();
        let col_value: f64 = (0..m).map(|c| cols[c] * col_util[c]).sum();
        for r in 0..n {
            row_regret[r] = (row_regret[r] + row_util[r] - row_value).max(0.0);
        }
        for c in 0..m {
            // The column player minimizes.
            col_regret[c] = (col_regret[c] + col_value - col_util[c]).max(0.0);
        }
        let weight = t as f64;
        for r in 0..n {
            row_sum[r] += weight * rows[r];
        }
        for c in 0..m {
            col_sum[c] += weight * cols[c];
        }
        if t % 16 == 0 || t == max_iterations {
            let eq = evaluate(matrix, &normalized(&row_sum), &normalized(&col_sum), t);
            let done = eq.exploitability <= tolerance;
            result = Some(eq);
            if done {
                break;
            }
        }
    }
    result.unwrap_or_else(|| {
        evaluate(
            matrix,
            &normalized(&row_sum),
            &normalized(&col_sum),
            iterations,
        )
    })
}

fn strategy(regret: &[f64]) -> Vec<f64> {
    let total: f64 = regret.iter().sum();
    if total <= 0.0 {
        vec![1.0 / regret.len() as f64; regret.len()]
    } else {
        regret.iter().map(|r| r / total).collect()
    }
}

fn normalized(sum: &[f64]) -> Vec<f64> {
    let total: f64 = sum.iter().sum();
    if total <= 0.0 {
        vec![1.0 / sum.len() as f64; sum.len()]
    } else {
        sum.iter().map(|s| s / total).collect()
    }
}

#[allow(clippy::needless_range_loop)]
fn evaluate(matrix: &Matrix, rows: &[f64], cols: &[f64], iterations: usize) -> Equilibrium {
    let (n, m) = (matrix.rows, matrix.cols);
    let mut value = 0.0f64;
    let mut row_best = f64::NEG_INFINITY;
    let mut col_best = f64::INFINITY;
    for r in 0..n {
        let mut util = 0.0;
        for c in 0..m {
            util += cols[c] * f64::from(matrix.at(r, c));
        }
        value += rows[r] * util;
        row_best = row_best.max(util);
    }
    for c in 0..m {
        let mut util = 0.0;
        for r in 0..n {
            util += rows[r] * f64::from(matrix.at(r, c));
        }
        col_best = col_best.min(util);
    }
    Equilibrium {
        rows: rows.iter().map(|&p| p as f32).collect(),
        cols: cols.iter().map(|&p| p as f32).collect(),
        value: value as f32,
        exploitability: ((row_best - value) + (value - col_best)).max(0.0) as f32,
        iterations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_pennies_is_a_coin_flip() {
        let game = Matrix::new(2, 2, vec![1.0, -1.0, -1.0, 1.0]);
        let eq = solve(&game, 4000, 1e-3);
        assert!((eq.value).abs() < 0.02, "{eq:?}");
        assert!((eq.rows[0] - 0.5).abs() < 0.05, "{eq:?}");
        assert!((eq.cols[0] - 0.5).abs() < 0.05, "{eq:?}");
        assert!(eq.exploitability <= 1e-3 || eq.iterations == 4000);
    }

    /// A dominant row is played with probability 1 and the value is its minimum.
    #[test]
    fn dominant_strategy_is_pure() {
        let game = Matrix::new(2, 2, vec![3.0, 2.0, 1.0, 0.0]);
        let eq = solve(&game, 2000, 1e-4);
        assert!(eq.rows[0] > 0.99, "{eq:?}");
        assert!((eq.value - 2.0).abs() < 0.01, "{eq:?}");
        assert_eq!(game.maximin(), (0, 2.0));
    }

    /// Rock-paper-scissors with a payoff twist still has value 0 with the known equilibrium
    /// (each action 1/3) within tolerance.
    #[test]
    fn rock_paper_scissors() {
        let game = Matrix::new(3, 3, vec![0.0, -1.0, 1.0, 1.0, 0.0, -1.0, -1.0, 1.0, 0.0]);
        let eq = solve(&game, 6000, 1e-3);
        assert!(eq.value.abs() < 0.02, "{eq:?}");
        for p in eq.rows.iter().chain(&eq.cols) {
            assert!((p - 1.0 / 3.0).abs() < 0.06, "{eq:?}");
        }
    }
}
