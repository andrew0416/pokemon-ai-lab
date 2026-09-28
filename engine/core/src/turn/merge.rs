//! Merging identical positions of a staged enumeration (Opus GG, GG-turn-throughput).
//!
//! [`super::enumerate_stages`] merges equal (state, remaining turn) pairs after every stage and
//! equal end positions at the end; the sampler does the same with its end positions. A
//! `HashMap<(State, P), _>` did this with SipHash over the whole state, and every growth of the
//! map hashed every stored state again: most of a Median enumeration went into hashing. A
//! [`Merger`] keeps the entries in first-reached order in a `Vec` and indexes them by one
//! 64-bit hash per key, computed once: growing the index moves `(hash, entry)` pairs only, and
//! a new position's state is cloned once. Equal hashes are resolved by `Eq`, so the merged
//! result (entries, their order and the order in which probabilities add up) is the same as the
//! map's.
//!
//! The key's hash is [`crate::hash::key_hash`] of the state's [`State::position_hash`], which
//! the caller keeps incrementally while a run applies its instructions (board P3a), and the
//! remaining turn: the state itself is not hashed here.

use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hash::key_hash;
use crate::state::State;

/// Whether [`Merger::add`] checks every incrementally kept position hash against
/// [`State::position_hash`] ([`super::verify_position_hashes`]).
static VERIFY: AtomicBool = AtomicBool::new(false);

/// Turns the check of [`Merger::add`] on or off for the whole process.
pub(super) fn set_verify(on: bool) {
    VERIFY.store(on, Ordering::Relaxed);
}

/// Positions `(state, rest, probability)` merged by `(state, rest)` in first-reached order,
/// each kept with its state's [`State::position_hash`].
pub(super) struct Merger<const N: usize, Q> {
    entries: Vec<(State<N>, Q, f64, u64)>,
    /// Open addressing with linear probing: `(hash, index into entries)`, [`Merger::EMPTY`] for
    /// a free slot. The length is a power of two and at least twice the entry count.
    table: Vec<(u64, u32)>,
}

impl<const N: usize, Q: Hash + Eq + Clone> Merger<N, Q> {
    const EMPTY: u32 = u32::MAX;
    const INITIAL: usize = 16;

    pub(super) fn new() -> Self {
        Merger {
            entries: Vec::new(),
            table: vec![(0, Self::EMPTY); Self::INITIAL],
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Adds `probability` to the entry equal to `(state, rest)`, or appends one with clones of
    /// `state` and `rest` and `probability` if there is none. `state_hash` must be
    /// `state.position_hash()` (kept incrementally by the caller).
    pub(super) fn add(&mut self, state: &State<N>, state_hash: u64, rest: &Q, probability: f64) {
        if VERIFY.load(Ordering::Relaxed) {
            assert_eq!(
                state_hash,
                state.position_hash(),
                "the incrementally kept position hash differs from the full one"
            );
        }
        let hash = key_hash(state_hash, rest);
        let mask = self.table.len() - 1;
        let mut i = hash as usize & mask;
        loop {
            let (h, e) = self.table[i];
            if e == Self::EMPTY {
                break;
            }
            if h == hash {
                let entry = &mut self.entries[e as usize];
                if entry.3 == state_hash && entry.0 == *state && entry.1 == *rest {
                    entry.2 += probability;
                    return;
                }
            }
            i = (i + 1) & mask;
        }
        let index = u32::try_from(self.entries.len()).expect("fewer than 2^32 positions");
        self.table[i] = (hash, index);
        self.entries
            .push((state.clone(), rest.clone(), probability, state_hash));
        if self.entries.len() * 2 > self.table.len() {
            self.grow();
        }
    }

    fn grow(&mut self) {
        let size = self.table.len() * 2;
        let mask = size - 1;
        let mut table = vec![(0, Self::EMPTY); size];
        for &(h, e) in &self.table {
            if e != Self::EMPTY {
                let mut i = h as usize & mask;
                while table[i].1 != Self::EMPTY {
                    i = (i + 1) & mask;
                }
                table[i] = (h, e);
            }
        }
        self.table = table;
    }

    /// The merged entries in first-reached order, each with its state's position hash.
    pub(super) fn into_entries(self) -> Vec<(State<N>, Q, f64, u64)> {
        self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equal keys merge (probabilities add in arrival order), different ones keep their
    /// first-reached order, across several index growths.
    #[test]
    fn merges_equal_keys_in_first_reached_order() {
        let mut merger: Merger<2, u32> = Merger::new();
        let base = State::<2>::default();
        let mut states = Vec::new();
        for k in 0..100u16 {
            let mut s = base.clone();
            s.turn = k % 40;
            states.push(s);
        }
        for (k, s) in states.iter().enumerate() {
            merger.add(s, s.position_hash(), &((k % 3 == 0) as u32), 0.25);
        }
        let entries = merger.into_entries();
        let mut expected: Vec<(u16, u32, f64)> = Vec::new();
        for (k, s) in states.iter().enumerate() {
            let rest = (k % 3 == 0) as u32;
            match expected
                .iter_mut()
                .find(|(t, r, _)| *t == s.turn && *r == rest)
            {
                Some(e) => e.2 += 0.25,
                None => expected.push((s.turn, rest, 0.25)),
            }
        }
        let got: Vec<(u16, u32, f64)> = entries
            .iter()
            .map(|(s, r, p, _)| (s.turn, *r, *p))
            .collect();
        assert_eq!(got, expected);
        assert!(entries.iter().all(|(s, _, _, h)| *h == s.position_hash()));
    }
}
