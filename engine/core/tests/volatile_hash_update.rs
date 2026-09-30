//! Runs in OFF and ON builds, dense and compact. Reference is the original public
//! before/apply/after composition, with no candidate policy switch.
use lab_engine::dex::{moves, MoveId};
use lab_engine::instruction::Instruction;
use lab_engine::state::{SideId, SlotRef, State};
use lab_engine::volatile::{Volatile, VolatileState, VOLATILE_COUNT};

fn payload(seed: u16, active: bool) -> VolatileState {
    VolatileState {
        active,
        duration: 7,
        counter: seed,
        time: 3,
        mv: moves::PROTECT,
        hidden: 19,
    }
}

fn start<const N: usize>() -> State<N> {
    let mut state = State::default();
    for side in [SideId::One, SideId::Two] {
        for index in 0..N {
            state
                .slot_mut(SlotRef {
                    side,
                    slot: index as u8,
                })
                .party_index = Some(index as u8);
        }
    }
    state
}

fn original<const N: usize>(state: &mut State<N>, instruction: &Instruction) -> u64 {
    let before = state.instruction_hash(instruction);
    state.apply_one(instruction);
    state.instruction_hash(instruction).wrapping_sub(before)
}

fn exercise<const N: usize>() {
    let initial = start::<N>();
    let mut candidate = initial.clone();
    let mut reference = initial.clone();
    let mut hash = initial.position_hash();
    let mut instructions = Vec::new();
    for side in [SideId::One, SideId::Two] {
        for slot in 0..N {
            let target = SlotRef {
                side,
                slot: slot as u8,
            };
            // Fill beyond the inline bound, cross both bitmap words, then erase in reverse.
            let indices = [0, 63, 64, VOLATILE_COUNT - 1, 1, 2, 3, 4, 5];
            for pass in 0..4 {
                for index in indices {
                    let volatile = Volatile::ALL[index];
                    let old = reference.slot(target).volatiles.get(volatile);
                    let new = match pass {
                        0 => payload(index as u16 + 1, false),
                        1 => payload(index as u16 + 100, true),
                        2 => old, // identical write still obeys the old contract
                        _ => VolatileState::NONE,
                    };
                    let instruction = Instruction::SetVolatile {
                        target,
                        volatile,
                        old,
                        new,
                    };
                    let delta = candidate.apply_hashed(&instruction);
                    assert_eq!(delta, original(&mut reference, &instruction));
                    hash = hash.wrapping_add(delta);
                    assert_eq!(hash, candidate.position_hash());
                    assert_eq!(candidate, reference);
                    instructions.push(instruction);
                }
            }
        }
    }
    for instruction in instructions.iter().rev() {
        let before = candidate.instruction_hash(instruction);
        candidate.reverse_one(instruction);
        reference.reverse_one(instruction);
        hash = hash.wrapping_add(candidate.instruction_hash(instruction).wrapping_sub(before));
        assert_eq!(hash, candidate.position_hash());
        assert_eq!(candidate, reference);
    }
    assert_eq!(candidate, initial);
}

#[test]
fn hashed_writes_and_reverse_match_original_for_singles_and_doubles() {
    exercise::<1>();
    exercise::<2>();
}

fn mismatched_old<const N: usize>() {
    for side in [SideId::One, SideId::Two] {
        for slot in 0..N {
            let target = SlotRef {
                side,
                slot: slot as u8,
            };
            for index in [0, 63, 64, VOLATILE_COUNT - 1] {
                for actual in [VolatileState::NONE, payload(41, false), payload(42, true)] {
                    for new in [
                        VolatileState::NONE,
                        actual,
                        payload(100, false),
                        payload(101, true),
                    ] {
                        let volatile = Volatile::ALL[index];
                        let mut candidate = start::<N>();
                        candidate.slot_mut(target).volatiles.set(volatile, actual);
                        let mut reference = candidate.clone();
                        let initial_hash = candidate.position_hash();
                        let forged_old = payload(65535, false);
                        assert_ne!(forged_old, actual);
                        let instruction = Instruction::SetVolatile {
                            target,
                            volatile,
                            old: forged_old,
                            new,
                        };
                        let delta = candidate.apply_hashed(&instruction);
                        assert_eq!(delta, original(&mut reference, &instruction));
                        assert_eq!(initial_hash.wrapping_add(delta), candidate.position_hash());
                        assert_eq!(candidate, reference);
                        assert_eq!(candidate.slot(target).volatiles.get(volatile), new);
                        candidate.reverse_one(&instruction);
                        reference.reverse_one(&instruction);
                        assert_eq!(candidate, reference);
                        // Reverse deliberately uses Instruction.old, not the previously read value.
                        assert_eq!(candidate.slot(target).volatiles.get(volatile), forged_old);
                    }
                }
            }
        }
    }
}

#[test]
fn mismatched_instruction_old_is_ignored_on_apply_and_used_on_reverse() {
    mismatched_old::<1>();
    mismatched_old::<2>();
}

#[test]
fn other_instruction_hash_paths_keep_original_behavior() {
    let initial = start::<2>();
    let mut candidate = initial.clone();
    let mut reference = initial.clone();
    let target = SlotRef {
        side: SideId::Two,
        slot: 1,
    };
    for instruction in [
        Instruction::SetTurn { old: 0, new: 4 },
        Instruction::SetLastMove {
            target,
            old: MoveId::NONE,
            new: moves::PROTECT,
        },
        Instruction::SetMoveActions {
            target,
            old: 0,
            new: 6,
        },
    ] {
        let hash = candidate.position_hash();
        let delta = candidate.apply_hashed(&instruction);
        assert_eq!(delta, original(&mut reference, &instruction));
        assert_eq!(hash.wrapping_add(delta), candidate.position_hash());
        assert_eq!(candidate, reference);
        candidate.reverse_one(&instruction);
        reference.reverse_one(&instruction);
        assert_eq!(candidate, initial);
        assert_eq!(reference, initial);
    }
}

#[cfg(feature = "experiment-volatile-hash-update-observer")]
#[test]
fn observer_proves_one_location_lookup_and_expected_rank_counts() {
    use lab_engine::volatile::hash_update_observer as observe;
    let target = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    for (old, new, expected_before_ranks, expected_after_ranks) in [
        (VolatileState::NONE, VolatileState::NONE, 0, 0),
        (VolatileState::NONE, payload(1, false), 2, 1),
        (payload(1, false), payload(2, true), 3, 1),
        (payload(2, true), payload(2, true), 3, 1),
        (payload(3, true), VolatileState::NONE, 2, 1),
    ] {
        let mut candidate = start::<2>();
        candidate
            .slot_mut(target)
            .volatiles
            .set(Volatile::ALL[64], old);
        let mut reference = candidate.clone();
        let instruction = Instruction::SetVolatile {
            target,
            volatile: Volatile::ALL[64],
            old: payload(999, false),
            new,
        };
        observe::reset();
        let reference_delta = original(&mut reference, &instruction);
        let before = observe::counts();
        observe::reset();
        let candidate_delta = candidate.apply_hashed(&instruction);
        let after = observe::counts();
        assert_eq!(before.location_queries, 3);
        assert_eq!(after.location_queries, 1);
        let compact = usize::from(cfg!(feature = "experiment-compact-volatiles"));
        assert_eq!(before.rank_queries, expected_before_ranks * compact);
        assert_eq!(after.rank_queries, expected_after_ranks * compact);
        assert_eq!(candidate_delta, reference_delta);
        assert_eq!(candidate, reference);
    }
}
