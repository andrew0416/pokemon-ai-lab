//! Transposition table of child equilibria (board S24a): the one-turn equilibrium value of a
//! position ([`crate::Solver::nash_value`]: the child valuation of `deep`, `deep-nash`,
//! `--child-nash` and the child dump), reused whenever the same position recurs — across
//! replies and outcomes of one analysis and across analyses by the same solver (a rollout's
//! decisions, several `lab-plan` modes on one position).
//!
//! The key is the position itself (`State` + the suspended turn, `State: Eq + Hash`, the
//! engine's outcome-merge key), never a hash alone: a 64-bit hash would hand one position
//! another's value on a collision (board B32). A cheaper key is board P3a/P3b (incremental
//! hash, merge key) and only needs [`PositionKey`] replaced here.
//!
//! The value stored for a position depends on the solver's configuration (evaluator, rolls,
//! chance, pruning, side); a table belongs to one [`crate::Solver`], so a caller that changes
//! `Solver::config` between analyses must not rely on it (`lab-plan` and `lab-rollout` never
//! do). The table stops inserting at its capacity (deterministically: what is kept depends
//! only on the insertion order) so a long rollout cannot exhaust memory.
//!
//! Depth 3 and beyond (board S24c): a child analysed by its own depth-2 (or deeper) mixed
//! analysis is stored in a [`DeepTable`], keyed by the position and the levels (beam and
//! outcome cap per level) it was analysed with.
//!
//! Measured on the search bench (runs/search-bench-20260927): children recur rarely at depth 2
//! (sand-owen deep-nash 4 hits in 45 lookups, psy-cona 0 in 53: the moves used differ between
//! beam pairs, so PP alone tells their children apart), more often in small positions (the
//! `ability-change-fails` oracle scenario 19 in 27).

use std::collections::HashMap;
use std::hash::Hash;

use lab_engine::state::State;
use lab_engine::turn::Suspension;

/// A position: the state and what its turn still waits for.
pub type PositionKey<const N: usize> = (State<N>, Option<Suspension>);

/// A position with the deep levels (`(beam, outcome cap)` per level, outermost first) its
/// value was computed with ([`crate::solve::DeepLevel`]).
pub type DeepKey<const N: usize> = (Vec<(usize, Option<usize>)>, PositionKey<N>);

/// The table of one-turn equilibrium values.
pub type TranspositionTable<const N: usize> = Table<PositionKey<N>>;

/// The table of deep (depth 2 and more) child values (board S24c).
pub type DeepTable<const N: usize> = Table<DeepKey<N>>;

/// Default capacity: about 200 000 positions (a `State<2>` with its map overhead is a few
/// hundred bytes to a couple of kilobytes).
pub const DEFAULT_CAPACITY: usize = 200_000;

pub struct Table<K> {
    entries: HashMap<K, f32>,
    capacity: usize,
    enabled: bool,
}

impl<K: Eq + Hash> Table<K> {
    pub fn new(enabled: bool, capacity: usize) -> Self {
        Table {
            entries: HashMap::new(),
            capacity,
            enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The stored value of `key` (never when disabled).
    pub fn get(&self, key: &K) -> Option<f32> {
        if !self.enabled {
            return None;
        }
        self.entries.get(key).copied()
    }

    /// Stores `value` for `key` unless disabled or full.
    pub fn insert(&mut self, key: K, value: f32) {
        if self.enabled && self.entries.len() < self.capacity {
            self.entries.insert(key, value);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
