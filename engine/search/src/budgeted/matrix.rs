//! Indexed, demand-driven matrix solve for a changing continuation-value table.
//! The existing RM+ implementation is reused. Each backup restarts DO support discovery;
//! retaining an old restricted support after payoffs change would miss new best replies.

use crate::nash::{self, Equilibrium, Matrix};

#[derive(Default)]
pub(super) struct Counts {
    pub solves: u64,
    pub iterations: u64,
}

pub(super) struct Solution {
    pub equilibrium: Equilibrium,
    #[cfg(not(feature = "experiment-response-sweeps"))]
    pub row_values: Vec<f32>,
    #[cfg(not(feature = "experiment-response-sweeps"))]
    pub col_values: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn solve<E>(
    rows: usize,
    cols: usize,
    iterations: usize,
    tolerance: f32,
    lazy: bool,
    counts: &mut Counts,
    mut payoff: impl FnMut(usize) -> Result<f32, E>,
) -> Result<Solution, E> {
    let mut known = vec![None; rows * cols];
    let mut support_rows = vec![0];
    let mut support_cols = vec![0];
    let mut full = !lazy;
    for round in 0..=64 {
        if round == 64 || known.iter().flatten().count() * 5 > rows * cols * 3 {
            full = true;
        }
        if full {
            support_rows = (0..rows).collect();
            support_cols = (0..cols).collect();
        }
        // All deviations against the restricted strategy must be priced. Unobserved
        // off-support x off-support cells may remain unknown, never filled by a guess.
        for r in 0..rows {
            for c in 0..cols {
                if full || support_rows.contains(&r) || support_cols.contains(&c) {
                    let i = r * cols + c;
                    if known[i].is_none() {
                        known[i] = Some(payoff(i)?);
                    }
                }
            }
        }
        let mut values = Vec::with_capacity(support_rows.len() * support_cols.len());
        for &r in &support_rows {
            for &c in &support_cols {
                values.push(known[r * cols + c].unwrap());
            }
        }
        let eq = nash::solve(
            &Matrix::new(support_rows.len(), support_cols.len(), values),
            iterations,
            tolerance * 0.1,
        );
        counts.solves += 1;
        counts.iterations += eq.iterations as u64;
        let mut row_strategy = vec![0.0; rows];
        let mut col_strategy = vec![0.0; cols];
        for (&r, &p) in support_rows.iter().zip(&eq.rows) {
            row_strategy[r] = p;
        }
        for (&c, &p) in support_cols.iter().zip(&eq.cols) {
            col_strategy[c] = p;
        }
        let row_values: Vec<f32> = (0..rows)
            .map(|r| {
                support_cols
                    .iter()
                    .map(|&c| f64::from(col_strategy[c]) * f64::from(known[r * cols + c].unwrap()))
                    .sum::<f64>() as f32
            })
            .collect();
        let col_values: Vec<f32> = (0..cols)
            .map(|c| {
                support_rows
                    .iter()
                    .map(|&r| f64::from(row_strategy[r]) * f64::from(known[r * cols + c].unwrap()))
                    .sum::<f64>() as f32
            })
            .collect();
        let best_row =
            row_values
                .iter()
                .enumerate()
                .fold((0, f32::NEG_INFINITY), |best, (i, &v)| {
                    if v > best.1 {
                        (i, v)
                    } else {
                        best
                    }
                });
        let best_col = col_values
            .iter()
            .enumerate()
            .fold(
                (0, f32::INFINITY),
                |best, (i, &v)| if v < best.1 { (i, v) } else { best },
            );
        let gap = (best_row.1 - best_col.1).max(0.0);
        let new_row = !support_rows.contains(&best_row.0);
        let new_col = !support_cols.contains(&best_col.0);
        if full || gap <= tolerance || (!new_row && !new_col) {
            let value = row_strategy
                .iter()
                .zip(&row_values)
                .map(|(&p, &v)| f64::from(p) * f64::from(v))
                .sum::<f64>() as f32;
            return Ok(Solution {
                equilibrium: Equilibrium {
                    rows: row_strategy,
                    cols: col_strategy,
                    value,
                    exploitability: gap,
                    iterations: eq.iterations,
                },
                #[cfg(not(feature = "experiment-response-sweeps"))]
                row_values,
                #[cfg(not(feature = "experiment-response-sweeps"))]
                col_values,
            });
        }
        if new_row {
            support_rows.push(best_row.0);
        }
        if new_col {
            support_cols.push(best_col.0);
        }
    }
    unreachable!("last round uses the full matrix")
}
