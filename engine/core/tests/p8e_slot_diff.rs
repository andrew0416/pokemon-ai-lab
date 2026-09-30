#![cfg(feature = "experiment-slot-diff-observer")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use lab_engine::dex::{moves, species, MoveId};
use lab_engine::gimmick::DynamaxState;
use lab_engine::instruction::Instruction;
use lab_engine::state::{SideId, Slot, SlotRef, State, SwitchFlag};
use lab_engine::turn::slot_diff_observer::{self as observer, BaselineScope};
use lab_engine::volatile::{Volatile, VolatileState};

// Count allocations only in the calling test thread, with setup/assertions outside the window.
// This executable is separate from all existing tests and from timing builds.
struct Allocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn tracked<T>(work: impl FnOnce() -> T) -> (T, usize) {
    ALLOCS.with(|count| count.set(0));
    TRACK.with(|track| track.set(true));
    let value = work();
    TRACK.with(|track| track.set(false));
    (value, ALLOCS.with(Cell::get))
}

fn start<const N: usize>() -> State<N> {
    let mut state = State::<N>::default();
    for side in [SideId::One, SideId::Two] {
        for index in 0..N {
            let mon = &mut state.side_mut(side).party[index];
            mon.species = species::TYRANITAR;
            mon.hp = 100;
            mon.max_hp = 100;
            let slot = &mut state.side_mut(side).slots[index];
            slot.party_index = Some(index as u8);
            slot.boosts = [1, -1, 2, -2, 0, 3, -3];
            slot.last_move = moves::TACKLE;
            slot.last_move_target_loc = -2;
            slot.move_actions = 255;
            slot.history.hurt_this_turn = Some(47);
            slot.history.times_attacked = 19;
            slot.history.moves_used = 7;
            slot.history.newly_switched = false;
            slot.switch_flag = SwitchFlag::CopyVolatile;
            slot.substitute_hp = 23;
            slot.ability_order = 9;
            // >4 values force compact storage to spill; all hidden payload fields are nonzero.
            for (index, volatile) in Volatile::ALL.into_iter().take(7).enumerate() {
                slot.volatiles.set(
                    volatile,
                    VolatileState {
                        active: true,
                        duration: 2,
                        counter: (index + 4) as u16,
                        time: 3,
                        mv: moves::PROTECT,
                        hidden: 29,
                    },
                );
            }
        }
    }
    state
}

fn baseline<const N: usize>(from: &State<N>, to: &State<N>) -> Vec<Instruction> {
    let _scope = BaselineScope::new();
    observer::instructions(from, to)
}

fn check_roundtrip<const N: usize>(from: &State<N>, to: &State<N>, instructions: &[Instruction]) {
    let mut actual = from.clone();
    let mut hash = actual.position_hash();
    for instruction in instructions {
        hash = hash.wrapping_add(actual.apply_hashed(instruction));
        assert_eq!(hash, actual.position_hash());
    }
    assert_eq!(&actual, to, "full state, including hidden payloads");
    for instruction in instructions.iter().rev() {
        let before = actual.instruction_hash(instruction);
        actual.reverse_one(instruction);
        hash = hash.wrapping_add(actual.instruction_hash(instruction).wrapping_sub(before));
        assert_eq!(hash, actual.position_hash());
    }
    assert_eq!(&actual, from);
}

fn scalar_cases<const N: usize>() {
    let from = start::<N>();
    for side in [SideId::One, SideId::Two] {
        for slot in 0..N as u8 {
            let target = SlotRef { side, slot };
            for mask in 1u8..8 {
                for zero in [false, true] {
                    let mut to = from.clone();
                    let changed = to.slot_mut(target);
                    if mask & 1 != 0 {
                        changed.last_move = if zero { MoveId::NONE } else { moves::PROTECT };
                    }
                    if mask & 2 != 0 {
                        changed.last_move_target_loc = if zero { 0 } else { 2 };
                    }
                    if mask & 4 != 0 {
                        changed.move_actions = if zero { 0 } else { 1 };
                    }
                    let reference = baseline(&from, &to);
                    observer::reset();
                    let actual = observer::instructions(&from, &to);
                    assert_eq!(observer::counts().shortcut_slots, 1);
                    assert_eq!(observer::counts().rebuilt_slots, 0);
                    assert_eq!(actual.len(), mask.count_ones() as usize);
                    assert!(actual.iter().all(|instruction| matches!(
                        instruction,
                        Instruction::SetLastMove { .. }
                            | Instruction::SetLastMoveTargetLoc { .. }
                            | Instruction::SetMoveActions { .. }
                    )));
                    check_roundtrip(&from, &to, &actual);
                    check_roundtrip(&from, &to, &reference);
                }
            }
        }
    }
}

#[test]
fn scalar_diffs_preserve_full_states_and_incremental_hash_in_singles_and_doubles() {
    scalar_cases::<1>();
    scalar_cases::<2>();
}

#[test]
fn shortcut_avoids_real_switch_and_compact_clone_allocations() {
    let from = start::<2>();
    let mut to = from.clone();
    to.sides[0].slots[0].move_actions = 0;
    // Initialize observer TLS outside the allocation measurement.
    observer::reset();
    let (reference, reference_allocs) = {
        let _scope = BaselineScope::new();
        tracked(|| observer::instructions(&from, &to))
    };
    observer::reset();
    let (actual, actual_allocs) = tracked(|| observer::instructions(&from, &to));
    assert_eq!(
        actual_allocs, 1,
        "only the existing output Vec allocation remains"
    );
    #[cfg(feature = "experiment-compact-volatiles")]
    assert_eq!(
        reference_allocs,
        actual_allocs + 2,
        "one Switch Box and one spilled compact clone"
    );
    #[cfg(not(feature = "experiment-compact-volatiles"))]
    assert_eq!(reference_allocs, actual_allocs + 1, "one Switch Box");
    assert_eq!(observer::counts().shortcut_slots, 1);
    assert!(!actual
        .iter()
        .any(|instruction| matches!(instruction, Instruction::Switch { .. })));
    check_roundtrip(&from, &to, &reference);
    check_roundtrip(&from, &to, &actual);
}

#[test]
fn every_other_slot_field_retains_the_exact_baseline_fallback() {
    let from = start::<2>();
    let target = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    let mutations: &[fn(&mut Slot)] = &[
        |slot| slot.party_index = Some(2),
        |slot| slot.party_index = None,
        |slot| slot.fainted_occupant = Some(2),
        |slot| slot.boosts[3] += 1,
        |slot| {
            slot.volatiles.set(
                Volatile::Protect,
                VolatileState {
                    active: true,
                    hidden: 41,
                    ..VolatileState::NONE
                },
            )
        },
        |slot| slot.history.hurt_this_turn = Some(18),
        |slot| slot.history.moves_used ^= 1,
        |slot| slot.switch_flag = SwitchFlag::Effect,
        |slot| slot.substitute_hp = 17,
        |slot| slot.ability_order = 7,
    ];
    for mutate in mutations {
        let mut to = from.clone();
        let slot = to.slot_mut(target);
        slot.move_actions = 1;
        mutate(slot);
        let reference = baseline(&from, &to);
        observer::reset();
        let actual = observer::instructions(&from, &to);
        assert_eq!(actual, reference);
        assert_eq!(observer::counts().shortcut_slots, 0);
        assert_eq!(observer::counts().rebuilt_slots, 1);
        check_roundtrip(&from, &to, &actual);
    }
    let mut to = from.clone();
    to.sides[0].slots.swap(0, 1); // Ally Switch changes occupants, including hidden slot data.
    assert_eq!(observer::instructions(&from, &to), baseline(&from, &to));
    check_roundtrip(&from, &to, &observer::instructions(&from, &to));
}

#[test]
fn empty_slots_unchanged_slots_and_inactive_payload_keep_existing_boundary() {
    let mut from = start::<1>();
    observer::reset();
    assert!(observer::instructions(&from, &from).is_empty());
    assert_eq!(observer::counts(), Default::default());
    from.sides[0].slots[0].party_index = None;
    let mut to = from.clone();
    to.sides[0].slots[0].move_actions = 1;
    assert_eq!(observer::instructions(&from, &to), baseline(&from, &to));
    check_roundtrip(&from, &to, &observer::instructions(&from, &to));

    from.sides[0].slots[0].party_index = Some(0);
    from.sides[0].slots[0].volatiles.set(
        Volatile::Protect,
        VolatileState {
            active: false,
            duration: 4,
            hidden: 99,
            ..VolatileState::NONE
        },
    );
    to = from.clone();
    to.sides[0].slots[0].move_actions = 1;
    let reference = baseline(&from, &to);
    observer::reset();
    let actual = observer::instructions(&from, &to);
    assert_eq!(actual, reference);
    assert_eq!(observer::counts().shortcut_slots, 0);
    let mut expected = to;
    expected.sides[0].slots[0]
        .volatiles
        .set(Volatile::Protect, VolatileState::NONE);
    // Document, do not silently repair, the baseline active-only reconstruction boundary.
    check_roundtrip(&from, &expected, &actual);
}

#[test]
fn dynamax_keeps_the_existing_unsupported_reconstruction_boundary() {
    let mut from = start::<1>();
    from.sides[0].slots[0].dynamax = DynamaxState::Dynamax { turns: 2 };
    let mut to = from.clone();
    to.sides[0].slots[0].move_actions = 1;
    let reference = std::panic::catch_unwind(|| baseline(&from, &to));
    let actual = std::panic::catch_unwind(|| observer::instructions(&from, &to));
    if cfg!(debug_assertions) {
        assert!(reference.is_err());
        assert!(actual.is_err());
    } else {
        assert_eq!(actual.unwrap(), reference.unwrap());
    }
}
