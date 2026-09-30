//! P9: completed flat endings, borrowed once without their instruction round trip.
use std::ops::ControlFlow;

use super::{
    check_turn, enumerate_stages, frontier, initial_queue, run_stage, Endings, EnumerateOptions,
    JointAction, Pending, Ruleset, State, Suspension, TurnError,
};

/// An opaque, fully enumerated and merged batch in first-reached order. It contains no
/// evaluator or search policy. Only this module can construct one, after enumeration succeeds.
pub struct FinalStates<const N: usize> {
    endings: Endings<N, Pending>,
}

impl<const N: usize> FinalStates<N> {
    /// Consume the batch while borrowing its states in place. Moving each pending queue
    /// into its suspension wrapper copies neither State nor queue. Visitors may stop early;
    /// all unvisited endings are dropped. The caller cannot mutate or retain these borrows.
    pub fn visit<B>(
        mut self,
        mut visitor: impl FnMut(&State<N>, f64, Option<&Suspension>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        for (state, pending, probability, _) in self.endings.iter_mut().flatten() {
            let suspension = pending.take().map(Suspension);
            #[cfg(feature = "experiment-leaf-ending-observer")]
            observer::visited();
            visitor(state, *probability, suspension.as_ref())?;
        }
        ControlFlow::Continue(())
    }
}

/// Experimental flat-turn entry point. `None` requests the existing Outcome fallback when
/// factored enumeration is active (scope or environment). Validation, stage replay, merging,
/// probabilities and input restoration are exactly the existing turn pipeline. An error
/// returns no batch: even if an earlier branch finished, no visitor has run yet.
pub fn try_enumerate_turn_final_states<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
    options: EnumerateOptions,
) -> Result<Option<FinalStates<N>>, TurnError> {
    if frontier::factored_active() {
        return Ok(None);
    }
    let choices = check_turn(state, ruleset, &choices)?;
    enumerate_final_states_checked(state, &choices, options).map(Some)
}

// Both callers are inside turn: the public P9 entry point checked this state above,
// and PreparedTurn checked its own unreplaceable snapshot. No transferable token/API.
pub(super) fn enumerate_final_states_checked<const N: usize>(
    state: &mut State<N>,
    choices: &[JointAction<N>; 2],
    options: EnumerateOptions,
) -> Result<FinalStates<N>, TurnError> {
    let start = Pending::new(initial_queue(state, choices));
    let endings = enumerate_stages(state, start, options, run_stage)?;
    #[cfg(feature = "experiment-leaf-ending-observer")]
    observer::batch();
    Ok(FinalStates { endings })
}

/// Optional work counters for logic checks, never a timing or allocation benchmark.
#[cfg(feature = "experiment-leaf-ending-observer")]
pub mod observer {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub materialized_outcomes: usize,
        pub emitted_instructions: usize,
        pub batches: usize,
        pub visits: usize,
    }
    thread_local! { static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        materialized_outcomes: 0, emitted_instructions: 0, batches: 0, visits: 0,
    }) }; }
    pub fn reset() {
        COUNTS.set(Counts::default());
    }
    pub fn counts() -> Counts {
        COUNTS.get()
    }
    pub(in crate::turn) fn materialized(instructions: usize) {
        let mut c = counts();
        c.materialized_outcomes += 1;
        c.emitted_instructions += instructions;
        COUNTS.set(c);
    }
    pub(super) fn batch() {
        let mut c = counts();
        c.batches += 1;
        COUNTS.set(c);
    }
    pub(super) fn visited() {
        let mut c = counts();
        c.visits += 1;
        COUNTS.set(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{FactoredScope, StageEnd};

    #[test]
    fn p9_late_enumeration_error_exposes_no_partial_batch() {
        let _flat = FactoredScope::new(false);
        let mut state = State::<2>::default();
        let original = state.clone();
        let mut completed_branch = false;
        let result = enumerate_stages(
            &mut state,
            Pending::new(Vec::new()),
            EnumerateOptions::default(),
            |battle, _| {
                if battle.rng.uniform(2) == 0 {
                    completed_branch = true;
                    Ok(StageEnd::Finished)
                } else {
                    Err(TurnError::Unsupported("deliberate later branch".into()))
                }
            },
        );
        let mut visited = 0;
        let result = result.map(|endings| {
            FinalStates { endings }.visit::<()>(|_, _, _| {
                visited += 1;
                ControlFlow::Continue(())
            })
        });
        assert!(completed_branch);
        assert_eq!(
            result,
            Err(TurnError::Unsupported("deliberate later branch".into()))
        );
        assert_eq!(visited, 0);
        assert_eq!(state, original);
    }

    #[test]
    fn p9_visit_starts_after_final_probability_merge() {
        let _flat = FactoredScope::new(false);
        let mut state = State::<2>::default();
        let original = state.clone();
        let mut replayed = 0;
        let endings = enumerate_stages(
            &mut state,
            Pending::new(Vec::new()),
            EnumerateOptions::default(),
            |battle, _| {
                let _ = battle.rng.uniform(2);
                replayed += 1;
                Ok(StageEnd::Finished)
            },
        )
        .unwrap();
        let mut visits = 0;
        let result: ControlFlow<()> = FinalStates { endings }.visit(|end, p, suspension| {
            assert_eq!(replayed, 2);
            assert_eq!(end, &original);
            assert_eq!(p.to_bits(), 1.0f64.to_bits());
            assert!(suspension.is_none());
            visits += 1;
            ControlFlow::Continue(())
        });
        assert!(result.is_continue());
        assert_eq!(visits, 1);
        assert_eq!(state, original);
    }
}
