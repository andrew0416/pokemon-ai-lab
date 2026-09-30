use super::*;
use lab_engine::{
    state::SideId,
    volatile::{Volatile, VolatileState},
};
use std::hash::{Hash, Hasher};
#[path = "../tests/p13_support.rs"]
mod support;

#[derive(Default, PartialEq, Debug)]
struct Writes(Vec<(&'static str, Vec<u8>)>);
macro_rules! typed_write {
    ($name:ident, $ty:ty) => {
        fn $name(&mut self, value: $ty) {
            self.0.push((stringify!($ty), value.to_ne_bytes().to_vec()));
        }
    };
}
impl Hasher for Writes {
    typed_write!(write_u8, u8);
    typed_write!(write_u16, u16);
    typed_write!(write_u32, u32);
    typed_write!(write_u64, u64);
    typed_write!(write_u128, u128);
    typed_write!(write_usize, usize);
    typed_write!(write_i8, i8);
    typed_write!(write_i16, i16);
    typed_write!(write_i32, i32);
    typed_write!(write_i64, i64);
    typed_write!(write_i128, i128);
    typed_write!(write_isize, isize);
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.push(("bytes", bytes.to_vec()));
    }
}

#[test]
fn borrowed_option_hash_input_and_collision_eq_match_owned_keys() {
    let (state, suspension) = support::suspended();
    for rest in [
        None,
        Some(suspension),
        Some(support::suspended_named("uturn-pause").1),
    ] {
        let mut owned = Writes::default();
        let mut borrowed = Writes::default();
        rest.hash(&mut owned);
        rest.as_ref().hash(&mut borrowed);
        assert_eq!(owned, borrowed);
        let key = (state.clone(), rest.clone());
        assert_eq!(
            position_index(&state, rest.as_ref()),
            key_hash(state.position_hash(), &rest)
        );
        assert_eq!(key.index(), position_index(&state, rest.as_ref()));
        let mut table = TranspositionTable::new(true, 2);
        // Deliberately bypass indexing to force all distinct keys into one bucket.
        table.entries.insert(42, key, 1.0);
        let mut altered = state.clone();
        altered.side_mut(SideId::One).history.ate_berry ^= 1;
        table
            .entries
            .insert(42, (altered.clone(), rest.clone()), 2.0);
        assert_eq!(table.get_borrowed_at(42, &state, rest.as_ref()), Some(1.0));
        assert_eq!(
            table.get_borrowed_at(42, &altered, rest.as_ref()),
            Some(2.0)
        );
        altered.sides[0].slots[0].volatiles.set(
            Volatile::ALL[0],
            VolatileState {
                hidden: 237,
                ..VolatileState::NONE
            },
        );
        assert_eq!(table.get_borrowed_at(42, &altered, rest.as_ref()), None);
        if rest.is_some() {
            assert_eq!(table.get_borrowed_at(42, &state, None), None);
        }
    }
}

#[test]
fn capacity_full_update_order_disabled_and_clear_are_unchanged() {
    let first = (State::<1>::default(), None);
    let mut second = first.clone();
    second.0.turn = 1;
    for (enabled, capacity) in [(false, 2), (true, 0), (true, 1), (true, 2)] {
        let mut table = TranspositionTable::new(enabled, capacity);
        table.insert(first.clone(), 1.0);
        table.insert(second.clone(), 2.0);
        table.insert(first.clone(), 3.0); // Even an update is blocked when full.
        let expected = if enabled && capacity > 0 {
            Some(1.0)
        } else {
            None
        };
        assert_eq!(table.get(&first), expected);
        assert_eq!(
            table.get_borrowed_at(first.index(), &first.0, None),
            expected
        );
        assert_eq!(
            table.get(&second),
            if enabled && capacity == 2 {
                Some(2.0)
            } else {
                None
            }
        );
        table.clear();
        assert!(table.is_empty());
        assert_eq!(table.get_borrowed_at(first.index(), &first.0, None), None);
    }
}

#[test]
fn collision_bucket_distinguishes_two_some_suspensions() {
    let (state, first) = support::suspended();
    let (_, second) = support::suspended_named("uturn-pause");
    assert_ne!(first, second);
    let mut table = TranspositionTable::new(true, 2);
    table
        .entries
        .insert(19, (state.clone(), Some(first.clone())), 11.0);
    table
        .entries
        .insert(19, (state.clone(), Some(second.clone())), 22.0);
    assert_eq!(table.get_borrowed_at(19, &state, Some(&first)), Some(11.0));
    assert_eq!(table.get_borrowed_at(19, &state, Some(&second)), Some(22.0));
    assert_eq!(table.get_borrowed_at(19, &state, None), None);
}
