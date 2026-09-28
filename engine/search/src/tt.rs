//! Transposition table of child equilibria (board S24a): the one-turn equilibrium value of a
//! position ([`crate::Solver::nash_value`]: the child valuation of `deep`, `deep-nash`,
//! `--child-nash` and the child dump), reused whenever the same position recurs — across
//! replies and outcomes of one analysis and across analyses by the same solver (a rollout's
//! decisions, several `lab-plan` modes on one position).
//!
//! The key is the position itself (`State` + the suspended turn, the engine's outcome-merge
//! key), never a hash alone: a 64-bit hash would hand one position another's value on a
//! collision (board B32). The table indexes it by the engine's position hash
//! (`lab_engine::hash::key_hash` of `State::position_hash` and the suspension, board P3b) and
//! confirms by `Eq` (`lab_engine::hash::PositionMap`), instead of a SipHash of the whole state
//! for every lookup and every growth of the map.
//!
//! The value stored for a position depends on the solver's configuration (evaluator, rolls,
//! chance, pruning, side); a table belongs to one [`crate::Solver`], so a caller that changes
//! `Solver::config` between analyses must not rely on it (`lab-plan` and `lab-rollout` never
//! do). The table stops inserting at its capacity (deterministically: what is kept depends
//! only on the insertion order) so a long rollout cannot exhaust memory.
//!
//! Measured on the search bench (runs/search-bench-20260927): children recur rarely at depth 2
//! (sand-owen deep-nash 4 hits in 45 lookups, psy-cona 0 in 53: the moves used differ between
//! beam pairs, so PP alone tells their children apart), more often in small positions (the
//! `ability-change-fails` oracle scenario 19 in 27).

use lab_engine::hash::{key_hash, PositionMap};
use lab_engine::state::State;
use lab_engine::turn::Suspension;

/// A position: the state and what its turn still waits for.
pub type PositionKey<const N: usize> = (State<N>, Option<Suspension>);

/// Default capacity: about 200 000 positions (a `State<2>` with its map overhead is a few
/// hundred bytes to a couple of kilobytes).
pub const DEFAULT_CAPACITY: usize = 200_000;

pub struct TranspositionTable<const N: usize> {
    entries: PositionMap<PositionKey<N>, f32>,
    capacity: usize,
    enabled: bool,
}

impl<const N: usize> TranspositionTable<N> {
    pub fn new(enabled: bool, capacity: usize) -> Self {
        TranspositionTable {
            entries: PositionMap::new(),
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
    pub fn get(&self, key: &PositionKey<N>) -> Option<f32> {
        if !self.enabled {
            return None;
        }
        self.entries.get(Self::hash(key), key).copied()
    }

    /// Stores `value` for `key` unless disabled or full.
    pub fn insert(&mut self, key: PositionKey<N>, value: f32) {
        if self.enabled && self.entries.len() < self.capacity {
            self.entries.insert(Self::hash(&key), key, value);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The index of `key`: the engine's position hash with the suspended turn.
    fn hash(key: &PositionKey<N>) -> u64 {
        key_hash(key.0.position_hash(), &key.1)
    }
}
