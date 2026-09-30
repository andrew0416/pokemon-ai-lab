//! Private-state turn batch. There is no transferable validation token and no unchecked
//! entry point accepting a caller's state/ruleset. Enumeration can only use this snapshot.

use super::*;

type CheckedChoice<const N: usize> = Option<Result<JointAction<N>, TurnError>>;

/// Experimental internal bridge for search, not a replacement for the single-turn API.
/// Owns one parent snapshot and its ruleset for the whole batch. Callers can change their
/// original State without invalidating this batch: its outcomes still start at its snapshot.
/// Fields are private, and no method exposes mutable parent access or a normalized token.
/// No hashes participate in validation reuse. Duplicate indices are separate choices.
///
/// The saved parent and rules cannot be replaced from outside the engine:
/// ```compile_fail
/// use lab_engine::{state::State, turn::PreparedTurn};
/// fn replace_parent(batch: &mut PreparedTurn<2>, state: State<2>) { batch.state = state; }
/// ```
/// ```compile_fail
/// use lab_engine::{rules::Ruleset, turn::PreparedTurn};
/// fn replace_rules(batch: &mut PreparedTurn<2>) { batch.ruleset = Ruleset::NO_GIMMICKS; }
/// ```
pub struct PreparedTurn<const N: usize> {
    state: State<N>,
    ruleset: Ruleset,
    choices: [Vec<JointAction<N>>; 2],
    normalized: [Vec<CheckedChoice<N>>; 2],
    parent: Option<Result<(), TurnError>>,
    support: Option<Result<(), TurnError>>,
}

impl<const N: usize> PreparedTurn<N> {
    pub fn new(state: &State<N>, ruleset: Ruleset, choices: [Vec<JointAction<N>>; 2]) -> Self {
        let normalized = std::array::from_fn(|side| vec![None; choices[side].len()]);
        Self {
            state: state.clone(),
            ruleset,
            choices,
            normalized,
            parent: None,
            support: None,
        }
    }

    /// Search deliberately keeps Full and factored enumeration on their ordinary path.
    pub fn eligible(options: EnumerateOptions) -> bool {
        options.rolls != RollMode::Full && !frontier::factored_active()
    }

    /// Indices always refer to this batch's own choices (side one, then side two).
    /// Validate lazily in precisely `check_turn` order: parent, side one, side two, support.
    /// Errors stay cached but are returned only when their original pair reaches that step.
    pub fn enumerate(
        &mut self,
        indices: [usize; 2],
        options: EnumerateOptions,
    ) -> Result<Vec<Outcome>, TurnError> {
        // This internal indexed API has no single-turn equivalent for an out-of-range index.
        // Indexing before enumeration prevents partial execution on programmer error.
        let actions = [self.choices[0][indices[0]], self.choices[1][indices[1]]];
        if !Self::eligible(options) {
            return enumerate_turn_with(&mut self.state, self.ruleset, actions, options);
        }
        let normalized = self.checked_choices(indices, actions)?;
        enumerate_checked(&mut self.state, &normalized, options)
    }

    /// The same private parent and lazy validation, returning completed P9 endings.
    /// None declines Full/factored requests; the caller keeps its original P9 fallback.
    /// No State, ruleset, normalized action or validation token is accepted from outside.
    #[cfg(feature = "experiment-prepared-leaf")]
    pub fn try_enumerate_final_states(
        &mut self,
        indices: [usize; 2],
        options: EnumerateOptions,
    ) -> Result<Option<FinalStates<N>>, TurnError> {
        // Match the existing indexed bridge's programmer-error boundary.
        let actions = [self.choices[0][indices[0]], self.choices[1][indices[1]]];
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        leaf_observer::update(|c| c.requests += 1);
        if !Self::eligible(options) {
            #[cfg(feature = "experiment-prepared-leaf-observer")]
            leaf_observer::update(|c| c.declined += 1);
            return Ok(None);
        }
        let result = self
            .checked_choices(indices, actions)
            .and_then(|normalized| {
                final_states::enumerate_final_states_checked(&mut self.state, &normalized, options)
            });
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        leaf_observer::update(|c| {
            if result.is_ok() {
                c.batches += 1;
            } else {
                c.errors += 1;
            }
        });
        result.map(Some)
    }

    // Keep one validation body for both bridges, including lazy cached error order.
    fn checked_choices(
        &mut self,
        indices: [usize; 2],
        actions: [JointAction<N>; 2],
    ) -> Result<[JointAction<N>; 2], TurnError> {
        self.parent
            .get_or_insert_with(|| check_turn_parent(&self.state))
            .clone()?;
        let mut normalized = actions;
        for (side, i) in indices.into_iter().enumerate() {
            normalized[side] = self.normalized[side][i]
                .get_or_insert_with(|| {
                    check_side(
                        &self.state,
                        self.ruleset,
                        [SideId::One, SideId::Two][side],
                        &actions[side],
                    )
                })
                .clone()?;
        }
        self.support
            .get_or_insert_with(|| check_turn_support(&self.state))
            .clone()?;
        Ok(normalized)
    }
}

/// P15 work evidence only; absent from uninstrumented builds.
#[cfg(feature = "experiment-prepared-leaf-observer")]
pub mod leaf_observer {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub requests: u64,
        pub declined: u64,
        pub batches: u64,
        pub errors: u64,
    }
    thread_local! { static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        requests: 0, declined: 0, batches: 0, errors: 0,
    }) }; }
    pub fn reset() {
        COUNTS.set(Counts::default());
    }
    pub fn counts() -> Counts {
        COUNTS.get()
    }
    #[cfg(feature = "experiment-prepared-leaf")]
    pub(super) fn update(f: impl FnOnce(&mut Counts)) {
        let mut c = counts();
        f(&mut c);
        COUNTS.set(c);
    }
}

#[cfg(feature = "experiment-prepared-turn-observe")]
thread_local! {
    static COUNTS: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) };
}

#[cfg(feature = "experiment-prepared-turn-observe")]
pub(super) fn count(index: usize) {
    COUNTS.with(|cell| {
        let mut counts = cell.get();
        counts[index] += 1;
        cell.set(counts);
    });
}

/// Test observer only: [parent checks, side checks, support-state checks] on this thread.
#[cfg(feature = "experiment-prepared-turn-observe")]
pub fn validation_counts() -> [u64; 3] {
    COUNTS.with(std::cell::Cell::get)
}

#[cfg(feature = "experiment-prepared-turn-observe")]
pub fn reset_validation_counts() {
    COUNTS.with(|cell| cell.set([0; 3]));
}
