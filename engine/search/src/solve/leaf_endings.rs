//! Default-off P9 dispatch: only flat turn edges into Next::Depth(0).
use super::*;
use lab_engine::turn::try_enumerate_turn_final_states;
use std::ops::ControlFlow;

impl<const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'_, N, E> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_leaf_endings(
        &mut self,
        state: &mut State<N>,
        decision: Decision,
        pair: [Choice<N>; 2],
        next: Next<'_, N>,
        alpha: f32,
        beta: f32,
        started: Instant,
    ) -> Option<Result<f32, SearchError>> {
        #[cfg(test)]
        if !tests::enabled() {
            return None;
        }
        if !matches!(next, Next::Depth(0)) || decision != Decision::Turn {
            return None;
        }
        let [Choice::Turn(a), Choice::Turn(b)] = pair else {
            return None;
        };
        let endings = try_enumerate_turn_final_states(
            state,
            self.config.ruleset,
            [a, b],
            self.config.enumerate_options(),
        );
        let endings = match endings {
            Ok(None) => return None,
            result => {
                self.stats.enumerate_seconds += started.elapsed().as_secs_f64();
                match result {
                    Ok(Some(endings)) => endings,
                    Err(TurnError::Unsupported(why)) => {
                        self.note_unsupported(why);
                        return Some(Ok(f32::NAN));
                    }
                    Err(error) => return Some(Err(error.into())),
                    Ok(None) => unreachable!(),
                }
            }
        };
        // The batch is complete before the first evaluation. These reductions deliberately
        // retain chance()'s operation order and cutoff/error timing; the core knows no policy.
        let value = match self.config.chance {
            Chance::Worst => {
                let mut worst = f32::INFINITY;
                match endings.visit(|state, _, suspension| {
                    let v = match self.depth_zero_value(state, suspension) {
                        Ok(v) => v,
                        Err(e) => return ControlFlow::Break(Err(e)),
                    };
                    if v.is_nan() {
                        return ControlFlow::Break(Ok(f32::NAN));
                    }
                    worst = worst.min(v);
                    if worst <= alpha {
                        return ControlFlow::Break(Ok(worst));
                    }
                    ControlFlow::Continue(())
                }) {
                    ControlFlow::Break(result) => result,
                    ControlFlow::Continue(()) => Ok(worst),
                }
            }
            Chance::Expect => {
                let mut sum = 0.0f64;
                let mut remaining = 1.0f64;
                match endings.visit(|state, p, suspension| {
                    let rest = (remaining - p).max(0.0);
                    // Keep Star1's arithmetic even though depth-zero leaves do not read
                    // their window. No replacement/resume is introduced at the horizon.
                    let _lo = ((alpha as f64 - (sum + rest * BOUND as f64)) / p).max(-BOUND as f64);
                    let _hi = ((beta as f64 - (sum - rest * BOUND as f64)) / p).min(BOUND as f64);
                    let v = match self.depth_zero_value(state, suspension) {
                        Ok(v) => v,
                        Err(e) => return ControlFlow::Break(Err(e)),
                    };
                    if v.is_nan() {
                        return ControlFlow::Break(Ok(f32::NAN));
                    }
                    sum += p * v as f64;
                    remaining = rest;
                    if sum - remaining * BOUND as f64 >= beta as f64 {
                        return ControlFlow::Break(Ok((sum - remaining * BOUND as f64) as f32));
                    }
                    if sum + remaining * BOUND as f64 <= alpha as f64 {
                        return ControlFlow::Break(Ok((sum + remaining * BOUND as f64) as f32));
                    }
                    ControlFlow::Continue(())
                }) {
                    ControlFlow::Break(result) => result,
                    ControlFlow::Continue(()) => Ok(sum as f32),
                }
            }
        };
        Some(value)
    }
}

#[cfg(test)]
pub(super) mod tests;
