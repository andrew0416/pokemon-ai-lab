//! P10 opt-in storage. Every non-NONE value is registered, including inactive payloads.
//! The payload vector follows bitmap/index order, making storage history irrelevant to Eq.

use std::fmt;
use std::hash::{Hash, Hasher};

use smallvec::SmallVec;

use super::{Volatile, VolatileState, VOLATILE_COUNT};

// An unmeasured design choice, not an observed occupancy percentile.
const INLINE_ENTRIES: usize = 4;
const BITMAP_WORDS: usize = VOLATILE_COUNT.div_ceil(u64::BITS as usize);

/// All volatiles of one slot, stored by non-NONE index.
///
/// With `experiment-compact-volatiles`, this type is Clone but not Copy, and does not expose
/// the dense tuple field. The default-off type retains that existing API. Overflow is owned
/// by SmallVec; there is no maximum supported number of entries below VOLATILE_COUNT.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Volatiles {
    // A bit means value != NONE, not value.active. Unused high bits are always zero.
    present: [u64; BITMAP_WORDS],
    // One value per set bit, ascending by volatile index; no NONE values are stored.
    states: SmallVec<[VolatileState; INLINE_ENTRIES]>,
}

impl Volatiles {
    fn location(volatile: Volatile) -> (usize, u64) {
        let index = volatile as usize;
        (
            index / u64::BITS as usize,
            1u64 << (index % u64::BITS as usize),
        )
    }

    /// Number of registered entries preceding this bit.
    fn rank(&self, word: usize, bit: u64) -> usize {
        #[cfg(feature = "experiment-volatile-hash-update-observer")]
        super::hash_update_observer::rank();
        self.present[..word]
            .iter()
            .map(|bits| bits.count_ones() as usize)
            .sum::<usize>()
            + (self.present[word] & (bit - 1)).count_ones() as usize
    }

    pub fn get(&self, volatile: Volatile) -> VolatileState {
        #[cfg(feature = "experiment-volatile-hash-update-observer")]
        super::hash_update_observer::location();
        let (word, bit) = Self::location(volatile);
        if self.present[word] & bit == 0 {
            VolatileState::NONE
        } else {
            self.states[self.rank(word, bit)]
        }
    }

    pub fn set(&mut self, volatile: Volatile, state: VolatileState) {
        #[cfg(feature = "experiment-volatile-hash-update-observer")]
        super::hash_update_observer::location();
        let (word, bit) = Self::location(volatile);
        let registered = self.present[word] & bit != 0;
        if !registered && state == VolatileState::NONE {
            return;
        }
        let rank = self.rank(word, bit);
        if state == VolatileState::NONE {
            self.states.remove(rank);
            self.present[word] &= !bit;
        } else if registered {
            self.states[rank] = state;
        } else {
            self.states.insert(rank, state);
            self.present[word] |= bit;
        }
    }

    pub fn has(&self, volatile: Volatile) -> bool {
        self.get(volatile).active
    }

    /// Preserve the same registry updates as set, but find the position only once and
    /// return the whole previous payload, including inactive non-NONE values.
    #[cfg(feature = "experiment-volatile-hash-update")]
    pub(crate) fn replace(&mut self, volatile: Volatile, state: VolatileState) -> VolatileState {
        #[cfg(feature = "experiment-volatile-hash-update-observer")]
        super::hash_update_observer::location();
        let (word, bit) = Self::location(volatile);
        let registered = self.present[word] & bit != 0;
        if !registered && state == VolatileState::NONE {
            return VolatileState::NONE;
        }
        let rank = self.rank(word, bit);
        let old = if registered {
            self.states[rank]
        } else {
            VolatileState::NONE
        };
        if state == VolatileState::NONE {
            self.states.remove(rank);
            self.present[word] &= !bit;
        } else if registered {
            self.states[rank] = state;
        } else {
            self.states.insert(rank, state);
            self.present[word] |= bit;
        }
        old
    }

    pub fn is_empty(&self) -> bool {
        self.states.iter().all(|state| !state.active)
    }

    /// Active volatiles with their state, in Volatile::ALL order.
    pub fn iter(&self) -> impl Iterator<Item = (Volatile, VolatileState)> + '_ {
        self.non_none()
            .filter_map(|(index, state)| state.active.then_some((Volatile::ALL[index], *state)))
    }

    /// Logical dense cells that differ from NONE, in index order (inactive payloads included).
    /// Only internal hash consumers need this; public iteration keeps its active-only meaning.
    pub(crate) fn non_none(&self) -> impl Iterator<Item = (usize, &VolatileState)> + '_ {
        self.present
            .iter()
            .enumerate()
            .flat_map(|(word, &bits)| SetBits {
                base: word * u64::BITS as usize,
                bits,
            })
            .zip(self.states.iter())
    }
}

struct SetBits {
    base: usize,
    bits: u64,
}

impl Iterator for SetBits {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            return None;
        }
        let index = self.base + self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        Some(index)
    }
}

impl Hash for Volatiles {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Exactly the dense implementation's sequence of Hasher calls, not SmallVec's Hash.
        for (index, value) in self.non_none() {
            state.write_u8(index as u8);
            value.hash(state);
        }
        state.write_u8(u8::MAX);
    }
}

impl fmt::Debug for Volatiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keep both normal and alternate Debug compatible with the dense tuple type.
        let dense: [VolatileState; VOLATILE_COUNT] =
            std::array::from_fn(|index| self.get(Volatile::ALL[index]));
        f.debug_tuple("Volatiles").field(&dense).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::moves;

    #[cfg(feature = "experiment-volatile-hash-update")]
    #[test]
    fn replace_keeps_set_registry_capacity_and_spill_representation() {
        let mut candidate = Volatiles::default();
        let mut baseline = Volatiles::default();
        for count in [0, 1, 4, 5, 12, VOLATILE_COUNT, 4, 0] {
            for index in (0..VOLATILE_COUNT).rev() {
                let next = if index < count {
                    payload(index + count, index % 3 == 0)
                } else {
                    VolatileState::NONE
                };
                let expected_old = baseline.get(Volatile::ALL[index]);
                assert_eq!(candidate.replace(Volatile::ALL[index], next), expected_old);
                baseline.set(Volatile::ALL[index], next);
                assert_eq!(candidate.present, baseline.present);
                assert_eq!(candidate.states, baseline.states);
                assert_eq!(candidate.states.capacity(), baseline.states.capacity());
                assert_eq!(candidate.states.spilled(), baseline.states.spilled());
            }
        }
    }

    // The old storage and hash contract, independent of the compact bookkeeping.
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Dense([VolatileState; VOLATILE_COUNT]);

    impl Default for Dense {
        fn default() -> Self {
            Self([VolatileState::NONE; VOLATILE_COUNT])
        }
    }

    impl Hash for Dense {
        fn hash<H: Hasher>(&self, state: &mut H) {
            for (index, value) in self.0.iter().enumerate() {
                if *value != VolatileState::NONE {
                    state.write_u8(index as u8);
                    value.hash(state);
                }
            }
            state.write_u8(u8::MAX);
        }
    }

    impl fmt::Debug for Dense {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("Volatiles").field(&self.0).finish()
        }
    }

    #[derive(Default, Debug, PartialEq, Eq)]
    struct TraceHasher(Vec<(&'static str, Vec<u8>)>);

    macro_rules! record_write {
        ($method:ident, $ty:ty) => {
            fn $method(&mut self, value: $ty) {
                self.0
                    .push((stringify!($method), value.to_ne_bytes().to_vec()));
            }
        };
    }

    impl Hasher for TraceHasher {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, bytes: &[u8]) {
            self.0.push(("write", bytes.to_vec()));
        }

        record_write!(write_u8, u8);
        record_write!(write_u16, u16);
        record_write!(write_u32, u32);
        record_write!(write_u64, u64);
        record_write!(write_u128, u128);
        record_write!(write_usize, usize);
        record_write!(write_i8, i8);
        record_write!(write_i16, i16);
        record_write!(write_i32, i32);
        record_write!(write_i64, i64);
        record_write!(write_i128, i128);
        record_write!(write_isize, isize);
    }

    fn hash_trace(value: &impl Hash) -> TraceHasher {
        let mut trace = TraceHasher::default();
        value.hash(&mut trace);
        trace
    }

    fn payload(index: usize, active: bool) -> VolatileState {
        VolatileState {
            active,
            duration: (index % 5) as u8,
            counter: (index as u16).wrapping_mul(521).wrapping_add(1),
            time: (index % 7) as u8,
            mv: if index % 2 == 0 {
                moves::PROTECT
            } else {
                moves::FAKE_OUT
            },
            hidden: (index % 251) as u8,
        }
    }

    fn set_both(
        compact: &mut Volatiles,
        dense: &mut Dense,
        volatile: Volatile,
        state: VolatileState,
    ) {
        compact.set(volatile, state);
        dense.0[volatile as usize] = state;
    }

    fn assert_dense(compact: &Volatiles, dense: &Dense) {
        for volatile in Volatile::ALL {
            assert_eq!(compact.get(volatile), dense.0[volatile as usize]);
            assert_eq!(compact.has(volatile), dense.0[volatile as usize].active);
        }
        let active: Vec<_> = Volatile::ALL
            .into_iter()
            .filter_map(|volatile| {
                let state = dense.0[volatile as usize];
                state.active.then_some((volatile, state))
            })
            .collect();
        assert_eq!(compact.iter().collect::<Vec<_>>(), active);
        assert_eq!(compact.is_empty(), active.is_empty());
        assert_eq!(hash_trace(compact), hash_trace(dense));
        let non_none: Vec<_> = dense
            .0
            .iter()
            .enumerate()
            .filter(|(_, state)| **state != VolatileState::NONE)
            .collect();
        assert_eq!(compact.non_none().collect::<Vec<_>>(), non_none);
    }

    fn assert_debug(compact: &Volatiles, dense: &Dense) {
        assert_eq!(format!("{compact:?}"), format!("{dense:?}"));
        assert_eq!(format!("{compact:#?}"), format!("{dense:#?}"));
    }

    #[test]
    fn registry_indices_and_bitmap_boundaries_match_dense() {
        for (index, volatile) in Volatile::ALL.into_iter().enumerate() {
            assert_eq!(volatile as usize, index);
        }
        let mut compact = Volatiles::default();
        let mut dense = Dense::default();
        assert_dense(&compact, &dense);
        assert_debug(&compact, &dense);
        assert!(std::mem::size_of::<Volatiles>() < std::mem::size_of::<Dense>());
        for index in [VOLATILE_COUNT - 1, 64, 0, 63] {
            set_both(
                &mut compact,
                &mut dense,
                Volatile::ALL[index],
                payload(index, index != 64),
            );
            assert_dense(&compact, &dense);
        }
        for index in [63, 0, VOLATILE_COUNT - 1, 64] {
            set_both(
                &mut compact,
                &mut dense,
                Volatile::ALL[index],
                VolatileState::NONE,
            );
            assert_dense(&compact, &dense);
        }
        assert_eq!(compact, Volatiles::default());
    }

    #[test]
    fn every_inactive_non_none_field_is_preserved() {
        let values = [
            VolatileState {
                duration: 1,
                ..VolatileState::NONE
            },
            VolatileState {
                counter: u16::MAX,
                ..VolatileState::NONE
            },
            VolatileState {
                time: 1,
                ..VolatileState::NONE
            },
            VolatileState {
                mv: moves::PROTECT,
                ..VolatileState::NONE
            },
            VolatileState {
                hidden: 1,
                ..VolatileState::NONE
            },
        ];
        for volatile in Volatile::ALL {
            let mut compact = Volatiles::default();
            let mut dense = Dense::default();
            for state in values {
                set_both(&mut compact, &mut dense, volatile, state);
                assert_dense(&compact, &dense);
                assert!(compact.is_empty());
                assert!(compact.iter().next().is_none());
                assert!(!compact.has(volatile));
                assert_ne!(compact, Volatiles::default());
                assert_ne!(hash_trace(&compact), hash_trace(&Volatiles::default()));
            }
            set_both(&mut compact, &mut dense, volatile, VolatileState::NONE);
            assert_dense(&compact, &dense);
            assert_eq!(compact, Volatiles::default());
        }
    }

    #[test]
    fn overflow_delete_reinsert_and_storage_history_do_not_change_identity() {
        let mut compact = Volatiles::default();
        let mut dense = Dense::default();
        for index in 0..=INLINE_ENTRIES {
            set_both(
                &mut compact,
                &mut dense,
                Volatile::ALL[index],
                payload(index, true),
            );
            assert_eq!(compact.states.spilled(), index >= INLINE_ENTRIES);
            assert_dense(&compact, &dense);
        }
        for index in (INLINE_ENTRIES + 1..VOLATILE_COUNT).rev() {
            set_both(
                &mut compact,
                &mut dense,
                Volatile::ALL[index],
                payload(index, index % 2 == 0),
            );
        }
        assert_eq!(compact.states.len(), VOLATILE_COUNT);
        assert_dense(&compact, &dense);
        assert_debug(&compact, &dense);
        let mut reverse_order = Volatiles::default();
        for index in (0..VOLATILE_COUNT).rev() {
            reverse_order.set(Volatile::ALL[index], dense.0[index]);
        }
        assert_eq!(compact, reverse_order);
        assert_eq!(hash_trace(&compact), hash_trace(&reverse_order));
        // Delete every registered entry in a different order, then reuse the spilled buffer.
        for index in (0..VOLATILE_COUNT)
            .step_by(2)
            .chain((1..VOLATILE_COUNT).step_by(2))
        {
            set_both(
                &mut compact,
                &mut dense,
                Volatile::ALL[index],
                VolatileState::NONE,
            );
            assert_dense(&compact, &dense);
        }
        assert!(compact.states.spilled());
        assert_eq!(compact, Volatiles::default());
        assert_debug(&compact, &dense);
        let mut inline = Volatiles::default();
        for index in (0..INLINE_ENTRIES).rev() {
            let state = payload(index, index != 1);
            set_both(&mut compact, &mut dense, Volatile::ALL[index], state);
            inline.set(Volatile::ALL[index], state);
        }
        assert!(compact.states.spilled());
        assert!(!inline.states.spilled());
        assert_eq!(compact, inline);
        assert_eq!(hash_trace(&compact), hash_trace(&inline));
        assert_dense(&compact, &dense);
        assert_debug(&compact, &dense);
        assert_eq!(format!("{compact:#?}"), format!("{inline:#?}"));
    }

    #[test]
    fn clones_and_clone_from_own_their_inline_and_overflow_values() {
        for source_len in [0, INLINE_ENTRIES, INLINE_ENTRIES + 1, VOLATILE_COUNT] {
            let mut source = Volatiles::default();
            for index in 0..source_len {
                source.set(Volatile::ALL[index], payload(index, true));
            }
            let expected = Dense(std::array::from_fn(|index| {
                if index < source_len {
                    payload(index, true)
                } else {
                    VolatileState::NONE
                }
            }));
            let original = source.clone();
            let mut cloned = source.clone();
            cloned.set(Volatile::ALL[VOLATILE_COUNT - 1], payload(7, false));
            assert_eq!(source, original);
            assert_dense(&source, &expected);
            assert_ne!(cloned, source);
            for target_len in [0, INLINE_ENTRIES, VOLATILE_COUNT] {
                let mut target = Volatiles::default();
                for index in 0..target_len {
                    target.set(Volatile::ALL[index], payload(index, false));
                }
                target.clone_from(&source);
                assert_eq!(target, source);
                assert_dense(&target, &expected);
                assert_eq!(hash_trace(&target), hash_trace(&source));
                let saved = target.clone();
                let mut changed_source = source.clone();
                changed_source.set(Volatile::ALL[0], payload(19, false));
                assert_eq!(target, saved);
                assert_dense(&target, &expected);
                assert_ne!(changed_source, target);
                target.set(Volatile::ALL[0], payload(23, false));
                assert_eq!(source, original);
                assert_dense(&source, &expected);
                assert_ne!(target, source);
            }
        }
    }

    #[test]
    fn mixed_set_clear_and_replace_sequence_matches_dense_model() {
        let mut compact = Volatiles::default();
        let mut dense = Dense::default();
        let mut rng = 0x58d0_6e9b_8af2_0c13u64;
        for step in 0..2048usize {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let index = (rng as usize) % VOLATILE_COUNT;
            let state = if step % 4 == 0 {
                VolatileState::NONE
            } else {
                payload((rng >> 32) as usize, step % 3 != 0)
            };
            set_both(&mut compact, &mut dense, Volatile::ALL[index], state);
            assert_dense(&compact, &dense);
            if step % 127 == 0 {
                let mut reordered = Volatiles::default();
                for index in (0..VOLATILE_COUNT).rev() {
                    reordered.set(Volatile::ALL[index], dense.0[index]);
                }
                assert_eq!(compact, reordered);
                assert_debug(&compact, &dense);
            }
        }
    }
}
