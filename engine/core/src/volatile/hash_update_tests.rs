//! Independent dense shadow for the experimental write-returning-old operation.
use std::hash::{Hash, Hasher};

use super::{Volatile, VolatileState, Volatiles, VOLATILE_COUNT};
use crate::dex::moves;

#[derive(Debug, Default, PartialEq, Eq)]
struct Trace(Vec<(&'static str, Vec<u8>)>);
macro_rules! record {
    ($name:ident, $ty:ty) => {
        fn $name(&mut self, value: $ty) {
            self.0
                .push((stringify!($name), value.to_ne_bytes().to_vec()));
        }
    };
}
impl Hasher for Trace {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.push(("write", bytes.to_vec()));
    }
    record!(write_u8, u8);
    record!(write_u16, u16);
    record!(write_u32, u32);
    record!(write_u64, u64);
    record!(write_u128, u128);
    record!(write_usize, usize);
    record!(write_i8, i8);
    record!(write_i16, i16);
    record!(write_i32, i32);
    record!(write_i64, i64);
    record!(write_i128, i128);
    record!(write_isize, isize);
}

// Deliberately exhaustive: a new payload field requires test input review too.
fn payload(seed: usize, active: bool) -> VolatileState {
    VolatileState {
        active,
        duration: (seed % 255 + 1) as u8,
        counter: (seed as u16).wrapping_mul(521).wrapping_add(1),
        time: (seed % 253 + 1) as u8,
        mv: moves::PROTECT,
        hidden: (seed % 251 + 1) as u8,
    }
}

fn check(actual: &Volatiles, dense: &[VolatileState; VOLATILE_COUNT]) {
    let mut expected_trace = Trace::default();
    let mut active = Vec::new();
    for (index, volatile) in Volatile::ALL.into_iter().enumerate() {
        assert_eq!(actual.get(volatile), dense[index]);
        assert_eq!(actual.has(volatile), dense[index].active);
        if dense[index].active {
            active.push((volatile, dense[index]));
        }
        if dense[index] != VolatileState::NONE {
            expected_trace.write_u8(index as u8);
            dense[index].hash(&mut expected_trace);
        }
    }
    expected_trace.write_u8(u8::MAX);
    let mut actual_trace = Trace::default();
    actual.hash(&mut actual_trace);
    assert_eq!(actual_trace, expected_trace, "exact typed Hash input");
    assert_eq!(actual.iter().collect::<Vec<_>>(), active);
    assert_eq!(actual.is_empty(), active.is_empty());
    let mut rebuilt = Volatiles::default();
    for index in (0..VOLATILE_COUNT).rev() {
        rebuilt.set(Volatile::ALL[index], dense[index]);
    }
    assert_eq!(*actual, rebuilt, "insertion order does not change Eq");
    assert_eq!(format!("{actual:#?}"), format!("{rebuilt:#?}"));
}

fn replace(
    actual: &mut Volatiles,
    dense: &mut [VolatileState; VOLATILE_COUNT],
    index: usize,
    next: VolatileState,
) {
    let old = dense[index];
    assert_eq!(actual.replace(Volatile::ALL[index], next), old);
    dense[index] = next;
    check(actual, dense);
}

#[test]
fn replace_returns_actual_old_for_every_cell_and_transition() {
    let mut actual = Volatiles::default();
    let mut dense = [VolatileState::NONE; VOLATILE_COUNT];
    for index in 0..VOLATILE_COUNT {
        for next in [
            VolatileState::NONE,
            payload(index, false),
            payload(index, true),
            payload(index, true),
            payload(index + 5, false),
            VolatileState::NONE,
            VolatileState::NONE,
        ] {
            replace(&mut actual, &mut dense, index, next);
        }
    }
}

#[test]
fn replace_preserves_inactive_fields_boundaries_spill_and_clone_ownership() {
    assert!(VOLATILE_COUNT > 64);
    let inactive = [
        VolatileState {
            duration: 1,
            ..VolatileState::NONE
        },
        VolatileState {
            counter: 65535,
            ..VolatileState::NONE
        },
        VolatileState {
            time: 255,
            ..VolatileState::NONE
        },
        VolatileState {
            mv: moves::FAKE_OUT,
            ..VolatileState::NONE
        },
        VolatileState {
            hidden: 255,
            ..VolatileState::NONE
        },
    ];
    for count in [0, 1, 4, 5, 12, VOLATILE_COUNT] {
        let mut actual = Volatiles::default();
        let mut dense = [VolatileState::NONE; VOLATILE_COUNT];
        for index in (0..VOLATILE_COUNT).rev().take(count) {
            replace(
                &mut actual,
                &mut dense,
                index,
                payload(index, index % 3 == 0),
            );
        }
        let captured = actual.clone();
        let captured_dense = dense;
        for index in [0, 63, 64, VOLATILE_COUNT - 1] {
            for next in inactive
                .into_iter()
                .chain([payload(9, true), VolatileState::NONE])
            {
                replace(&mut actual, &mut dense, index, next);
            }
        }
        check(&captured, &captured_dense);
        for index in 0..VOLATILE_COUNT {
            replace(&mut actual, &mut dense, index, VolatileState::NONE);
        }
        assert_eq!(actual, Volatiles::default());
    }
}
