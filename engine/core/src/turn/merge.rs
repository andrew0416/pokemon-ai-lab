//! Merging identical positions of a staged enumeration (Opus GG, GG-turn-throughput).
//!
//! [`super::enumerate_stages`] merges equal (state, remaining turn) pairs after every stage and
//! equal end positions at the end; the sampler does the same with its end positions. A
//! `HashMap<(State, P), _>` did this with SipHash over the whole state, and every growth of the
//! map hashed every stored state again: most of a Median enumeration went into hashing. A
//! [`Merger`] keeps the entries in first-reached order in a `Vec` and indexes them by one
//! 64-bit hash per key ([`KeyHasher`]), computed once: growing the index moves `(hash, entry)`
//! pairs only, and a new position's state is cloned once. Equal hashes are resolved by `Eq`, so
//! the merged result (entries, their order and the order in which probabilities add up) is the
//! same as the map's.

use std::hash::{Hash, Hasher};

use crate::state::State;

/// FxHash-style mixing of the derived `Hash` writes (one rotate, xor and multiply per word),
/// finished with the MurmurHash3 64-bit mixer so the low bits can index a table.
#[derive(Default)]
struct KeyHasher(u64);

impl KeyHasher {
    const SEED: u64 = 0x517c_c1b7_2722_0a95;

    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }
}

impl Hasher for KeyHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.add(u64::from_le_bytes(*chunk));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^ (h >> 33)
    }
}

/// Positions `(state, rest, probability)` merged by `(state, rest)` in first-reached order.
pub(super) struct Merger<const N: usize, Q> {
    entries: Vec<(State<N>, Q, f64)>,
    /// Open addressing with linear probing: `(hash, index into entries)`, [`Merger::EMPTY`] for
    /// a free slot. The length is a power of two and at least twice the entry count.
    table: Vec<(u64, u32)>,
}

impl<const N: usize, Q: Hash + Eq> Merger<N, Q> {
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

    /// Adds `probability` to the entry equal to `(state, rest)`, or appends one with a clone of
    /// `state` and `probability` if there is none.
    pub(super) fn add(&mut self, state: &State<N>, rest: Q, probability: f64) {
        let mut hasher = KeyHasher::default();
        state.hash(&mut hasher);
        rest.hash(&mut hasher);
        let hash = hasher.finish();
        let mask = self.table.len() - 1;
        let mut i = hash as usize & mask;
        loop {
            let (h, e) = self.table[i];
            if e == Self::EMPTY {
                break;
            }
            if h == hash {
                let entry = &mut self.entries[e as usize];
                if entry.0 == *state && entry.1 == rest {
                    entry.2 += probability;
                    return;
                }
            }
            i = (i + 1) & mask;
        }
        let index = u32::try_from(self.entries.len()).expect("fewer than 2^32 positions");
        self.table[i] = (hash, index);
        self.entries.push((state.clone(), rest, probability));
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

    /// The merged entries in first-reached order.
    pub(super) fn into_entries(self) -> Vec<(State<N>, Q, f64)> {
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
            merger.add(s, (k % 3 == 0) as u32, 0.25);
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
        let got: Vec<(u16, u32, f64)> = entries.iter().map(|(s, r, p)| (s.turn, *r, *p)).collect();
        assert_eq!(got, expected);
    }
}
