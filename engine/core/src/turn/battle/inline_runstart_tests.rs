//! Storage and replay contract tests run with P11 OFF as well as ON.

use super::*;
use crate::dex::{species, Type};
use crate::instruction::Instruction;
use crate::state::{Pokemon, SideId, SlotRef, State};

fn entries(count: usize) -> Vec<(PokemonRef, i32)> {
    (0..count)
        .map(|index| {
            (
                PokemonRef {
                    side: if index % 2 == 0 {
                        SideId::One
                    } else {
                        SideId::Two
                    },
                    party: (index % 6) as u8,
                },
                317 - index as i32 * 43,
            )
        })
        .collect()
}

#[test]
fn runstart_lengths_order_clone_and_ownership_are_preserved() {
    for count in [0, 1, 2, 4, 5, 12, 31] {
        let expected = entries(count);
        let mut state = State::<2>::default();
        let mut chooser = Chooser::new();
        let mut b = Battle::new(&mut state, &mut chooser);
        b.speed_snapshot.clone_from(&expected);
        let start = b.run_start();
        let cloned = start.clone();
        assert_eq!(start.speed_snapshot.as_slice(), expected.as_slice());
        assert_eq!(cloned.speed_snapshot.as_slice(), expected.as_slice());
        // The working snapshot is still a Vec. Mutating it cannot change either owned copy.
        let working: &mut Vec<(PokemonRef, i32)> = &mut b.speed_snapshot;
        working.clear();
        working.extend(entries(7));
        assert_eq!(start.speed_snapshot.as_slice(), expected.as_slice());
        assert_eq!(cloned.speed_snapshot.as_slice(), expected.as_slice());
        #[cfg(feature = "experiment-inline-runstart")]
        {
            assert_eq!(start.speed_snapshot.spilled(), count > 4);
            assert_eq!(cloned.speed_snapshot.spilled(), count > 4);
        }
        assert_eq!(start.history_readers, cloned.history_readers);
        assert_eq!(start.suppression, cloned.suppression);
    }
}

fn field() -> State<2> {
    let mut state = State::<2>::default();
    for side in [SideId::One, SideId::Two] {
        for (index, pokemon) in state.side_mut(side).party[..3].iter_mut().enumerate() {
            *pokemon = Pokemon {
                species: species::PIKACHU,
                level: 50,
                hp: 100,
                max_hp: 100,
                stats: [100, 100, 100, 100, 80 + index as i16 * 13],
                types: [Type::Normal, Type::None],
                ..Pokemon::default()
            };
        }
        for index in 0..2 {
            state.side_mut(side).slots[index].party_index = Some(index as u8);
        }
    }
    state
}

#[test]
fn newcomer_vec_growth_and_replay_do_not_mutate_runstart() {
    let mut state = field();
    let original = state.clone();
    let mut chooser = Chooser::new();
    chooser.begin_run();
    let slot = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    let (start, buffers, expected) = {
        let mut b = Battle::new(&mut state, &mut chooser);
        assert_eq!(b.speed_snapshot.len(), 4);
        let expected = b.speed_snapshot.clone();
        let start = b.run_start();
        b.apply(Instruction::Switch {
            slot,
            previous: Box::new(b.state.slot(slot).clone()),
            party_index: Some(2),
        });
        b.update_speed(slot);
        assert_eq!(
            b.speed_snapshot.len(),
            5,
            "newcomer stays in the mutable Vec"
        );
        assert_eq!(start.speed_snapshot.as_slice(), expected.as_slice());
        b.apply(Instruction::SetTurn { old: 0, new: 1 });
        (start, b.into_buffers(), expected)
    };
    state.reverse(&buffers.log);
    assert_eq!(state, original);
    let replay = Battle::replay(&mut state, &mut chooser, &start, buffers);
    assert_eq!(replay.speed_snapshot, expected);
    assert!(
        replay.speed_snapshot.capacity() >= 5,
        "working capacity still reused"
    );
    assert_eq!(replay.history_readers, start.history_readers);
    assert_eq!(replay.suppression, start.suppression);
    assert!(replay.log.is_empty());
    assert_eq!(replay.hash_delta, 0);
    assert!(replay.raw_speed.is_empty());
    assert_eq!(replay.state, &original);
    assert_eq!(replay.rng.probability().to_bits(), 1.0f64.to_bits());
}

#[test]
fn replay_restores_inline_and_spilled_entries_exactly() {
    for count in [0, 1, 2, 4, 5, 12, 31] {
        let mut state = State::<1>::default();
        let original = state.clone();
        let mut chooser = Chooser::new();
        let expected = entries(count);
        let (start, buffers) = {
            let mut b = Battle::new(&mut state, &mut chooser);
            b.speed_snapshot.clone_from(&expected);
            let start = b.run_start();
            b.speed_snapshot.clear();
            b.speed_snapshot.extend(entries(2));
            (start, b.into_buffers())
        };
        let replay = Battle::replay(&mut state, &mut chooser, &start, buffers);
        assert_eq!(replay.speed_snapshot, expected);
        assert_eq!(replay.state, &original);
        assert_eq!(replay.state.position_hash(), original.position_hash());
    }
}
