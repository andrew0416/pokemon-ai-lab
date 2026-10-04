//! Focused R5d checks of the field overlay, in addition to the stored Showdown fixtures.
//! These synthetic states exercise event contracts; they do not assert set legality.

use super::*;
use crate::dex::species;
use crate::state::{Pokemon, Slot, State};
use crate::turn::branch::{Chooser, RollMode};
use crate::volatile::VolatileState;

const A: SlotRef = SlotRef {
    side: SideId::One,
    slot: 0,
};
const B: SlotRef = SlotRef {
    side: SideId::One,
    slot: 1,
};
const T: SlotRef = SlotRef {
    side: SideId::Two,
    slot: 0,
};
const U: SlotRef = SlotRef {
    side: SideId::Two,
    slot: 1,
};
const SOURCE: PokemonRef = PokemonRef {
    side: SideId::One,
    party: 2,
};
const OCCUPANT: PokemonRef = PokemonRef {
    side: SideId::One,
    party: 0,
};

fn field() -> State<2> {
    let mut state = State::<2>::default();
    for side in [SideId::One, SideId::Two] {
        for (index, mon) in state.side_mut(side).party[..3].iter_mut().enumerate() {
            *mon = Pokemon {
                species: species::PIKACHU,
                level: 50,
                max_hp: 1000,
                hp: 1000,
                stats: [100, 100, 100, 100, 100 + index as i16],
                types: [Type::Normal, Type::None],
                ..Pokemon::default()
            };
        }
        for slot in 0..2 {
            state.side_mut(side).slots[slot].party_index = Some(slot as u8);
        }
    }
    state.pokemon_mut(SOURCE).species = species::SLOWKING;
    state.pokemon_mut(SOURCE).ability = abilities::NORMALIZE;
    state.pokemon_mut(SOURCE).item = items::LIFE_ORB;
    state
}

fn set_ability(state: &mut State<2>, slot: SlotRef, ability: AbilityId) {
    let pokemon = state.active_ref(slot).unwrap();
    state.pokemon_mut(pokemon).ability = ability;
    state.pokemon_mut(pokemon).base_ability = ability;
}

fn set_hp(state: &mut State<2>, pokemon: PokemonRef, hp: i16) {
    *state.pokemon_mut(pokemon) = Pokemon {
        hp,
        ..state.pokemon(pokemon).clone()
    };
}

fn active() -> VolatileState {
    VolatileState {
        active: true,
        ..VolatileState::NONE
    }
}

fn assert_reversible(b: &Battle<'_, 2>, before: &State<2>) {
    let mut replay = before.clone();
    let mut hash = replay.position_hash();
    for instruction in &b.log {
        hash = hash.wrapping_add(replay.apply_hashed(instruction));
        assert_eq!(hash, replay.position_hash(), "{instruction:?}");
    }
    assert_eq!(replay, *b.state);
    assert_eq!(format!("{replay:?}"), format!("{:?}", b.state));
    assert_eq!(before.position_hash().wrapping_add(b.hash_delta), hash);
    replay.reverse(&b.log);
    assert_eq!(replay, *before);
    assert_eq!(format!("{replay:?}"), format!("{before:?}"));
    assert_eq!(replay.position_hash(), before.position_hash());
}

#[test]
fn displaced_unnerve_reads_current_suppression_and_shield() {
    for (gas, acid, shield, blocked) in [
        (false, false, false, true),
        (true, false, false, false),
        (true, false, true, true),
        (false, true, true, false),
    ] {
        let mut state = field();
        set_ability(&mut state, A, abilities::UNNERVE);
        if gas {
            set_ability(&mut state, U, abilities::NEUTRALIZING_GAS);
        }
        if acid {
            state
                .slot_mut(A)
                .volatiles
                .set(Volatile::GastroAcid, active());
        }
        if shield {
            state.pokemon_mut(OCCUPANT).item = items::ABILITY_SHIELD;
        }
        let before = state.clone();
        let mut rng = Chooser::new();
        let mut b = Battle::new(&mut state, &mut rng);
        let absent = AbsentUser::place(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
        assert_eq!(absent.slot, A);
        assert_eq!(!ability_events::try_eat_item(&b, T), blocked);
        assert!(!AbsentUser::displaced_unnerve(&b, B), "allies may eat");
        // The holder's ability is not cached when the source is seated.
        if gas && !acid {
            b.add_volatile(U, Volatile::NeutralizingGasEnding);
            assert!(!ability_events::try_eat_item(&b, T));
        }
        absent.remove(&mut b);
        assert_reversible(&b, &before);
    }
}

#[test]
fn displaced_unnerve_tracks_started_alive_and_other_holders() {
    for ability in [
        abilities::UNNERVE,
        abilities::AS_ONE_GLASTRIER,
        abilities::AS_ONE_SPECTRIER,
    ] {
        let mut state = field();
        set_ability(&mut state, A, ability);
        set_ability(&mut state, B, abilities::UNNERVE);
        let mut rng = Chooser::new();
        let mut b = Battle::new(&mut state, &mut rng);
        let absent = AbsentUser::place(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
        b.unstarted.push(OCCUPANT);
        assert!(
            !ability_events::try_eat_item(&b, T),
            "the second holder still acts"
        );
        b.unstarted.push(b.occupant(B).unwrap());
        assert!(ability_events::try_eat_item(&b, T));
        b.unstarted.retain(|&p| p != OCCUPANT);
        assert!(!ability_events::try_eat_item(&b, T));
        b.apply(Instruction::Damage {
            target: OCCUPANT,
            amount: 1000,
        });
        assert!(
            ability_events::try_eat_item(&b, T),
            "a fainted holder is not a foe"
        );
        absent.remove(&mut b);
    }
}

#[test]
fn cotton_down_updates_real_occupants_and_keeps_the_benched_source() {
    for (ability, item, speed, attack) in [
        (abilities::DEFIANT, ItemId::NONE, -1, 2),
        (abilities::CONTRARY, ItemId::NONE, 1, 0),
        (abilities::INNER_FOCUS, items::CLEAR_AMULET, 0, 0),
        (abilities::INNER_FOCUS, items::WHITE_HERB, -1, 0),
    ] {
        let mut state = field();
        set_ability(&mut state, A, ability);
        state.pokemon_mut(OCCUPANT).item = item;
        set_ability(&mut state, T, abilities::COTTON_DOWN);
        // Slot transport must preserve payloads excluded from active-only iteration too.
        state.slot_mut(A).volatiles.set(
            Volatile::Charge,
            VolatileState {
                counter: 7,
                hidden: 13,
                ..VolatileState::NONE
            },
        );
        let before = state.clone();
        let mut rng = Chooser::with_rolls(RollMode::Median);
        let mut b = Battle::new(&mut state, &mut rng);
        future_move_hit(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
        assert_eq!(b.state.slot(A).boosts[4], speed, "{ability:?}/{item:?}");
        assert_eq!(b.state.slot(A).boosts[0], attack);
        assert_eq!(b.state.slot(B).boosts[4], -1);
        assert_eq!(b.state.slot(U).boosts[4], -1);
        assert_eq!(b.state.slot(T).boosts[4], 0);
        assert_eq!(b.mon(OCCUPANT).item, item);
        if item == items::WHITE_HERB {
            // Champions White Herb has no Update handler. The future hit has no
            // AfterMove either: its order-29 residual later consumes the item.
            assert_eq!(b.mon(OCCUPANT).item, items::WHITE_HERB);
            item_events::on_residual(&mut b, A, items::WHITE_HERB);
            assert_eq!(b.state.slot(A).boosts[4], 0);
            assert_eq!(b.mon(OCCUPANT).item, ItemId::NONE);
        }
        assert_eq!(b.mon(SOURCE), before.pokemon(SOURCE));
        assert_eq!(
            b.volatile(A, Volatile::Charge),
            before.slot(A).volatiles.get(Volatile::Charge)
        );
        assert!(b.absent_user.is_none() && b.absent_occupant.is_none());
        assert_reversible(&b, &before);
    }
}

#[test]
fn real_field_update_eats_a_displaced_berry_after_the_foe_unnerve_faints() {
    let mut state = field();
    set_ability(&mut state, A, abilities::UNNERVE);
    set_ability(&mut state, T, abilities::UNNERVE);
    set_hp(&mut state, OCCUPANT, 100);
    state.pokemon_mut(OCCUPANT).item = items::SITRUS_BERRY;
    let target = state.active_ref(T).unwrap();
    set_hp(&mut state, target, 1);
    let before = state.clone();
    let mut rng = Chooser::with_rolls(RollMode::Median);
    let mut b = Battle::new(&mut state, &mut rng);
    future_move_hit(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
    assert_eq!(b.mon(OCCUPANT).hp_value(), 350);
    assert_eq!(b.mon(OCCUPANT).item, ItemId::NONE);
    assert_eq!(b.mon(OCCUPANT).last_item, items::SITRUS_BERRY);
    assert_eq!(b.state.slot(T).party_index, None);
    assert_eq!(b.state.slot(T).fainted_occupant, Some(target.party));
    assert_eq!(b.mon(SOURCE), before.pokemon(SOURCE));
    assert_reversible(&b, &before);
}

#[test]
#[cfg(test)] // The refusal inventory scans test modules as individual source files.
fn overlay_transports_faint_and_restores_context_on_result_error() {
    let mut state = field();
    let before = state.clone();
    let mut rng = Chooser::new();
    let mut b = Battle::new(&mut state, &mut rng);
    let absent = AbsentUser::place(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
    let result = AbsentUser::with_real_occupant(&mut b, T, |b| {
        b.damage(A, 1000.0, DamageSource::Indirect);
        b.faint_messages(false).unwrap();
        Err::<(), _>(b.unsupported("test overlay error"))
    });
    assert!(result.is_err());
    assert_eq!(b.absent_user, Some(A));
    assert_eq!(b.occupant(A), Some(SOURCE));
    absent.remove(&mut b);
    assert_eq!(b.state.slot(A).party_index, None);
    assert_eq!(b.state.slot(A).fainted_occupant, Some(OCCUPANT.party));
    assert_eq!(b.mon(OCCUPANT).hp_value(), 0);
    assert_eq!(b.mon(SOURCE), before.pokemon(SOURCE));
    assert_reversible(&b, &before);
}

#[test]
fn unrelated_cross_pokemon_handlers_still_fail_closed_without_mutation() {
    for ability in [
        abilities::BATTERY,
        abilities::NEUTRALIZING_GAS,
        abilities::AIR_LOCK,
        abilities::OPPORTUNIST,
        abilities::TABLETS_OF_RUIN,
    ] {
        let mut state = field();
        set_ability(&mut state, A, ability);
        set_ability(&mut state, B, ability);
        let before = state.clone();
        let mut rng = Chooser::new();
        let mut b = Battle::new(&mut state, &mut rng);
        assert!(
            future_move_hit(&mut b, T, SOURCE, moves::FUTURE_SIGHT).is_err(),
            "{ability:?}"
        );
        assert_eq!(*b.state, before);
        assert!(b.log.is_empty());
        assert!(b.absent_user.is_none() && b.absent_occupant.is_none());
    }
}

#[test]
#[cfg(test)] // Keep this test's Unsupported pattern out of the production inventory.
fn an_update_that_gains_a_cross_pokemon_handler_is_refused_and_cleans_up() {
    let mut state = field();
    set_ability(&mut state, A, abilities::TRACE);
    state
        .slot_mut(A)
        .volatiles
        .set(Volatile::TraceSeek, active());
    // The only traceable foe is Opportunist. Its later onFoeAfterBoost would read
    // the Maranga Berry boost; it must not silently disappear behind the source.
    set_ability(&mut state, T, abilities::TRACE);
    set_ability(&mut state, U, abilities::OPPORTUNIST);
    let target = state.active_ref(T).unwrap();
    state.pokemon_mut(target).item = items::MARANGA_BERRY;
    let before = state.clone();
    let mut rng = Chooser::with_rolls(RollMode::Median);
    let mut b = Battle::new(&mut state, &mut rng);
    let error = future_move_hit(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap_err();
    assert!(matches!(error, TurnError::Unsupported(ref why) if why.contains("Opportunist")));
    assert_eq!(b.mon(OCCUPANT).ability, abilities::OPPORTUNIST);
    assert!(b.absent_user.is_none() && b.absent_occupant.is_none());
    assert!(b.active_move.is_none());
    assert_eq!(b.mon(SOURCE), before.pokemon(SOURCE));
    assert_reversible(&b, &before);
}

#[test]
fn an_empty_seat_stays_empty_after_cotton_down() {
    let mut state = field();
    *state.slot_mut(A) = Slot::default();
    set_hp(&mut state, SOURCE, 0);
    set_ability(&mut state, T, abilities::COTTON_DOWN);
    let before = state.clone();
    let mut rng = Chooser::with_rolls(RollMode::Median);
    let mut b = Battle::new(&mut state, &mut rng);
    future_move_hit(&mut b, T, SOURCE, moves::FUTURE_SIGHT).unwrap();
    assert_eq!(*b.state.slot(A), Slot::default());
    assert_eq!(b.state.slot(B).boosts[4], -1);
    assert_eq!(b.mon(SOURCE), before.pokemon(SOURCE));
    assert_reversible(&b, &before);
}
