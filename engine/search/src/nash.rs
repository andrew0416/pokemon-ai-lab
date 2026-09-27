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
    // The payoffs widened once (the loop below reads them 2 × iterations times); every sum
    // runs in the same order as a cell-by-cell pass, so the result is bit-identical to it.
    let values: Vec<f64> = matrix.values.iter().map(|&v| f64::from(v)).collect();
    let mut row_regret = vec![0.0f64; n];
    let mut col_regret = vec![0.0f64; m];
    let mut row_sum = vec![0.0f64; n];
    let mut col_sum = vec![0.0f64; m];
    let mut rows = vec![0.0f64; n];
    let mut cols = vec![0.0f64; m];
    let mut row_util = vec![0.0f64; n];
    let mut col_util = vec![0.0f64; m];
    let mut iterations = 0;
    let mut result = None;
    for t in 1..=max_iterations.max(1) {
        iterations = t;
        strategy_into(&row_regret, &mut rows);
        strategy_into(&col_regret, &mut cols);
        // Expected payoffs of each pure action against the other player's current strategy.
        col_util.fill(0.0);
        for ((util, &p), row) in row_util.iter_mut().zip(&rows).zip(values.chunks_exact(m)) {
            let mut acc = 0.0f64;
            for ((cu, &q), &v) in col_util.iter_mut().zip(&cols).zip(row) {
                acc += q * v;
                *cu += p * v;
            }
            *util = acc;
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

/// The rows and columns that survive iterated weak dominance (board S24d): a row is removed
/// when another surviving row is at least as good in every surviving column (for an exact
/// duplicate, the later one goes), a column when another is at most as bad in every surviving
/// row, until nothing changes. Removing weakly dominated strategies keeps the value of a
/// zero-sum game, and every equilibrium of the reduced game is one of the full game.
pub fn undominated(matrix: &Matrix) -> (Vec<usize>, Vec<usize>) {
    let (n, m) = (matrix.rows, matrix.cols);
    let v = &matrix.values;
    let mut rows: Vec<usize> = (0..n).collect();
    let mut cols: Vec<usize> = (0..m).collect();
    loop {
        let before = (rows.len(), cols.len());
        // Rows: r goes if some other surviving q has v[q][c] >= v[r][c] for every column
        // (q before r when they are equal everywhere, so one of a duplicate pair stays).
        let mut keep = Vec::with_capacity(rows.len());
        for (i, &r) in rows.iter().enumerate() {
            let dominated = rows.iter().enumerate().any(|(k, &q)| {
                q != r
                    && cols.iter().all(|&c| v[q * m + c] >= v[r * m + c])
                    && (k < i || cols.iter().any(|&c| v[q * m + c] > v[r * m + c]))
            });
            if !dominated {
                keep.push(r);
            }
        }
        rows = keep;
        let mut keep = Vec::with_capacity(cols.len());
        for (i, &c) in cols.iter().enumerate() {
            let dominated = cols.iter().enumerate().any(|(k, &q)| {
                q != c
                    && rows.iter().all(|&r| v[r * m + q] <= v[r * m + c])
                    && (k < i || rows.iter().any(|&r| v[r * m + q] < v[r * m + c]))
            });
            if !dominated {
                keep.push(c);
            }
        }
        cols = keep;
        if (rows.len(), cols.len()) == before {
            return (rows, cols);
        }
    }
}

/// [`solve`] on the game reduced by [`undominated`], its strategies extended with zeros and
/// the value and exploitability measured on the full matrix. The value agrees with [`solve`]'s
/// within both solutions' exploitability; the reduced game is usually several times smaller,
/// so each iteration is cheaper and fewer are needed.
pub fn solve_reduced(matrix: &Matrix, max_iterations: usize, tolerance: f32) -> Equilibrium {
    let (rows, cols) = undominated(matrix);
    if rows.len() == matrix.rows && cols.len() == matrix.cols {
        return solve(matrix, max_iterations, tolerance);
    }
    let mut values = Vec::with_capacity(rows.len() * cols.len());
    for &r in &rows {
        for &c in &cols {
            values.push(matrix.at(r, c));
        }
    }
    let reduced = solve(
        &Matrix::new(rows.len(), cols.len(), values),
        max_iterations,
        tolerance,
    );
    let mut row_p = vec![0.0f64; matrix.rows];
    for (&r, &p) in rows.iter().zip(&reduced.rows) {
        row_p[r] = f64::from(p);
    }
    let mut col_p = vec![0.0f64; matrix.cols];
    for (&c, &p) in cols.iter().zip(&reduced.cols) {
        col_p[c] = f64::from(p);
    }
    evaluate(matrix, &row_p, &col_p, reduced.iterations)
}

/// Regret matching: the positive regrets normalised (uniform when none is positive).
fn strategy_into(regret: &[f64], out: &mut [f64]) {
    let total: f64 = regret.iter().sum();
    if total <= 0.0 {
        out.fill(1.0 / regret.len() as f64);
    } else {
        for (o, r) in out.iter_mut().zip(regret) {
            *o = r / total;
        }
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

fn evaluate(matrix: &Matrix, rows: &[f64], cols: &[f64], iterations: usize) -> Equilibrium {
    let m = matrix.cols;
    let mut value = 0.0f64;
    let mut row_best = f64::NEG_INFINITY;
    let mut col_best = f64::INFINITY;
    // Column utilities accumulate row by row: each column's sum still runs over the rows in
    // order, as a column-by-column pass would.
    let mut col_util = vec![0.0f64; m];
    for (&p, row) in rows.iter().zip(matrix.values.chunks_exact(m)) {
        let mut util = 0.0;
        for ((cu, &q), &v) in col_util.iter_mut().zip(cols).zip(row) {
            let v = f64::from(v);
            util += q * v;
            *cu += p * v;
        }
        value += p * util;
        row_best = row_best.max(util);
    }
    for util in col_util {
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

    /// The cell-by-cell RM+ loop the solver had before it read rows as slices (lab-search
    /// 9ff1081); `solve` must reproduce it bit for bit.
    #[allow(clippy::needless_range_loop)]
    fn solve_reference(matrix: &Matrix, max_iterations: usize, tolerance: f32) -> Equilibrium {
        fn strategy(regret: &[f64]) -> Vec<f64> {
            let total: f64 = regret.iter().sum();
            if total <= 0.0 {
                vec![1.0 / regret.len() as f64; regret.len()]
            } else {
                regret.iter().map(|r| r / total).collect()
            }
        }
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
        let (n, m) = (matrix.rows, matrix.cols);
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

    #[test]
    fn solve_is_bit_identical_to_the_cell_loop() {
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            ((x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32) * 400.0
                - 200.0
        };
        for (n, m, iterations) in [(1, 1, 50), (3, 7, 3000), (17, 11, 5000), (40, 33, 2000)] {
            let matrix = Matrix::new(n, m, (0..n * m).map(|_| next()).collect());
            let a = solve(&matrix, iterations, 0.01);
            let b = solve_reference(&matrix, iterations, 0.01);
            assert_eq!(a, b, "{n}x{m}");
        }
    }

    #[test]
    fn dominance_removes_dominated_and_duplicate_strategies() {
        // Row 1 is dominated by row 0, row 2 duplicates row 0; column 2 is worse for the
        // column player than column 0 everywhere.
        let game = Matrix::new(
            3,
            3,
            vec![
                3.0, -1.0, 5.0, //
                2.0, -1.0, 4.0, //
                3.0, -1.0, 5.0,
            ],
        );
        let (rows, cols) = undominated(&game);
        assert_eq!(rows, vec![0]);
        assert_eq!(cols, vec![1]);
        let eq = solve_reduced(&game, 1000, 1e-4);
        assert_eq!(eq.value, -1.0);
        assert_eq!(eq.rows, vec![1.0, 0.0, 0.0]);
        // Matching pennies has nothing to remove.
        let pennies = Matrix::new(2, 2, vec![1.0, -1.0, -1.0, 1.0]);
        assert_eq!(undominated(&pennies), (vec![0, 1], vec![0, 1]));
    }

    /// The reduced game's value is the full game's, within the two solutions' exploitability,
    /// on random games padded with dominated and duplicate rows and columns.
    #[test]
    fn reduced_solve_keeps_the_value() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            ((x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32) * 200.0
                - 100.0
        };
        for (n, m) in [(4, 3), (12, 9), (30, 25)] {
            let mut values: Vec<f32> = (0..n * m).map(|_| next()).collect();
            // Dominated copies of the first row and column, and an exact duplicate row.
            let mut rows = n;
            let first: Vec<f32> = values[..m].to_vec();
            values.extend(first.iter().map(|v| v - 3.0));
            values.extend(first.iter());
            rows += 2;
            let mut full = Vec::with_capacity(rows * (m + 1));
            for r in 0..rows {
                full.extend(&values[r * m..(r + 1) * m]);
                full.push(values[r * m] + 7.0);
            }
            let game = Matrix::new(rows, m + 1, full);
            let (kept_rows, kept_cols) = undominated(&game);
            assert!(kept_rows.len() <= n && kept_cols.len() <= m, "{n}x{m}");
            let a = solve(&game, 20_000, 0.001);
            let b = solve_reduced(&game, 20_000, 0.001);
            assert!(
                (a.value - b.value).abs() <= a.exploitability + b.exploitability + 1e-3,
                "{n}x{m}: {a:?} vs {b:?}"
            );
        }
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
