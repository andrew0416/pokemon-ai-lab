//! Transform and Imposter (EE1): Showdown `pokemon.transformInto(target, effect)` and what
//! `clearVolatile` undoes when the transformed Pokémon leaves the field.
//!
//! `transformInto` (`sim/pokemon.ts`; the Champions mod does not override it, nor the move or
//! the ability) fails when the target fainted, either Pokémon is under Illusion
//! (`Pokemon::illusion`, EE2: `abilities::illusion_before_switch_in`), the target is
//! behind a substitute, either Pokémon is transformed already, or the target is
//! Eternatus-Eternamax (the Ogerpon / Terapagos / Stellar cases need Terastallization, which
//! Champions turns off). Otherwise, in this order:
//! 1. `setSpecies(target.species, effect, true)`: the species; `pokemon.speed` becomes the
//!    Speed `spreadModify` gives the target's species with the user's own set (nature and SP),
//!    until the next `updateSpeed()`; the stored stats it computes are overwritten below.
//! 2. `transformed = true`; the target's weight (`weighthg`, Autotomize's reductions included).
//! 3. `setType(target.getTypes(true, true))` (a roosting target's `roost.typeWas`: its real
//!    types) and `addedType = target.addedType`.
//! 4. The target's stored stats (Attack to Speed; max HP and HP stay the user's).
//! 5. `moveSlots`: a virtual copy of each of the target's moves with `min(5, move.pp)` PP
//!    (`used` false: Last Resort's record starts over); `timesAttacked` is the target's.
//! 6. The target's stat stages, all seven.
//! 7. The critical-hit volatiles: Dragon Cheer, Focus Energy (G-Max Chi Strike: no Gigantamax)
//!    and Laser Focus are removed from the user, then each one the target has is added fresh
//!    (`addVolatile`: Laser Focus with its full duration) — Dragon Cheer with the target's
//!    `hasDragonType`.
//! 8. `setAbility(target.ability, this, null, true, true)`: no `cantsuppress` test and no
//!    `SetAbility` event; the old ability's `End`, the new one with a fresh `abilityState`, and
//!    its `Start` only when the id differs.
//!
//! While transformed, abilities flagged `notransform` (Disguise, Ice Face, Zero to Hero, Hunger
//! Switch, Gulp Missile, Neutralizing Gas, Protosynthesis, Quark Drive, ...) are ignored
//! (`ignoringAbility`), and the checks of `pokemon.baseSpecies` see the species the Pokémon
//! returns to ([`crate::state::Pokemon::untransformed_species`]).
//!
//! `clearVolatile` (switching out, fainting): `moveSlots = baseMoveSlots.slice()`, `transformed
//! = false`, the base ability, and `setSpecies(baseSpecies)` — [`revert_on_leave`], then
//! `Battle::clear_volatile` restores the base species' types and stored stats.

use crate::dex::{species, MoveId};
use crate::instruction::Instruction;
use crate::state::{Forme, MoveSlot, SlotRef, TransformBase, BOOST_COUNT};
use crate::volatile::{decode_types, Volatile, VolatileState};

use super::battle::Battle;
use super::switching::{end_ability, start_ability};
use super::TurnError;

/// The critical-hit volatiles `transformInto` copies (Showdown's `volatilesToCopy` without
/// `gmaxchistrike`), in its order.
const CRIT_VOLATILES: [Volatile; 3] = [
    Volatile::DragonCheer,
    Volatile::FocusEnergy,
    Volatile::LaserFocus,
];

/// A copied move slot: `pp = Math.min(5, move.pp)` of the dex move (Champions caps base PP at
/// 20), `maxpp` the same.
fn virtual_slot(id: MoveId) -> MoveSlot {
    MoveSlot {
        id,
        pp: id.data().pp.min(5),
        disabled: false,
    }
}

/// `transformInto`'s first test: whether the Pokémon in `user` cannot transform into the one
/// in `target`.
fn fails<const N: usize>(b: &Battle<'_, N>, user: SlotRef, target: SlotRef) -> bool {
    let (Some(u), Some(t)) = (b.alive(user), b.alive(target)) else {
        // `pokemon.fainted` (a Pokémon at 0 HP whose faint is processed has left the slot).
        return true;
    };
    let (user_mon, target_mon) = (b.mon(u), b.mon(t));
    user_mon.illusion
        || target_mon.illusion
        || b.has_substitute(target)
        || target_mon.transformed.is_some()
        || user_mon.transformed.is_some()
        || target_mon.species == species::ETERNATUS_ETERNAMAX
}

/// Showdown `pokemon.transformInto(target)` for the Pokémon in `user` (see the module docs).
/// Returns whether it transformed.
pub(crate) fn transform_into<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
) -> Result<bool, TurnError> {
    if fails(b, user, target) {
        return Ok(false);
    }
    let pokemon = b.occupant(user).expect("alive");
    let copied = b.occupant(target).expect("alive");
    let mon = b.mon(pokemon).clone();
    let source = b.mon(copied).clone();
    if b.volatile(user, Volatile::Encore).active {
        return Err(b.unsupported(
            "Transform by an encored Pokémon (the encored move leaves the move slots)",
        ));
    }
    // What `clearVolatile` brings back: the base species (a temporary forme's base) and the
    // own move slots with their PP now.
    let base_species = super::forme::temporary_forme_base(mon.species).unwrap_or(mon.species);
    b.apply(Instruction::SetTransformed {
        target: pokemon,
        old: None,
        new: Some(TransformBase {
            species: base_species,
            moves: mon.moves,
        }),
    });
    // 1–4. The species with the target's stored stats (types below; the ability last).
    let old = mon.forme();
    let new = Forme {
        species: source.species,
        stats: source.stats,
        ..old
    };
    if new != old {
        b.apply(Instruction::SetForme {
            target: pokemon,
            old,
            new,
        });
    }
    let set_speed = mon.forme_as(source.species).stats[4];
    b.species_set_speed(pokemon, i32::from(set_speed));
    // The weight: the target's (its Autotomize reductions, on the same species).
    if mon.autotomized != source.autotomized {
        b.apply(Instruction::SetAutotomized {
            target: pokemon,
            old: mon.autotomized,
            new: source.autotomized,
        });
    }
    let roost = b.volatile(target, Volatile::Roost);
    let types = if roost.active && roost.counter != 0 {
        decode_types(roost.counter)
    } else {
        source.types
    };
    super::moves::set_types(b, user, types);
    let added = b.volatile(target, Volatile::AddedType);
    b.set_volatile_state(user, Volatile::AddedType, added);
    // 5. The virtual move slots, `timesAttacked`, a fresh `used` record.
    let mut moves = [MoveSlot::default(); 4];
    for (slot, copy) in moves
        .iter_mut()
        .zip(source.moves.iter().filter(|m| !m.id.is_none()))
    {
        *slot = virtual_slot(copy.id);
    }
    b.apply(Instruction::SetMoves {
        target: pokemon,
        old: mon.moves,
        new: moves,
    });
    let mut history = b.slot_history(user);
    history.times_attacked = b.slot_history(target).times_attacked;
    history.moves_used = 0;
    b.set_slot_history(user, history);
    // 6. The stat stages.
    let boosts: [i8; BOOST_COUNT] = b.state.slot(target).boosts;
    for (stat, &new) in boosts.iter().enumerate() {
        let old = b.state.slot(user).boosts[stat];
        if old != new {
            b.apply(Instruction::Boost {
                target: user,
                stat: stat as u8,
                amount: new - old,
            });
        }
    }
    // 7. The critical-hit volatiles.
    for volatile in CRIT_VOLATILES {
        b.remove_volatile(user, volatile);
    }
    for volatile in CRIT_VOLATILES {
        let theirs = b.volatile(target, volatile);
        if theirs.active && b.add_volatile(user, volatile) && volatile == Volatile::DragonCheer {
            let mine = b.volatile(user, volatile);
            b.set_volatile_state(
                user,
                volatile,
                VolatileState {
                    hidden: theirs.hidden,
                    ..mine
                },
            );
        }
    }
    // 8. `setAbility(pokemon.ability, this, null, true, true)`. A notransform ability is ignored
    // from now on (`abilities::ignoring_ability`), which `Battle::ability` checks only while
    // suppression is possible.
    b.suppression = true;
    let old_ability = mon.ability;
    let ability = source.ability;
    if !super::support::ability_supported_on_field(ability) {
        return Err(b.unsupported(format!(
            "Transform copying {} ({:?})",
            ability.data().name,
            ability.data().handlers
        )));
    }
    end_ability(b, user, old_ability)?;
    super::abilities::replace_ability(b, user, ability);
    if ability != old_ability {
        start_ability(b, user, ability)?;
    }
    Ok(true)
}

/// Imposter's `onSwitchIn` (priority 0, in the SwitchIn event) for its holder in `slot`: it
/// transforms into the foe in the opposite position (`pokemon.side.foe.active[length - 1 -
/// position]`), if one is there (an empty position has no Pokémon; a fainted one makes
/// `transformInto` fail).
pub(crate) fn imposter<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<(), TurnError> {
    let foe = SlotRef {
        side: slot.side.other(),
        slot: N as u8 - 1 - slot.slot,
    };
    transform_into(b, slot, foe)?;
    Ok(())
}

/// The Transform part of `clearVolatile` for a Pokémon leaving the field: its own move slots
/// come back (`baseMoveSlots`, with the PP they had when it transformed), `transformed` ends, and
/// its base species returns (`setSpecies(baseSpecies)`; `Battle::clear_volatile` then restores
/// that species' types and stored stats).
pub(crate) fn revert_on_leave<const N: usize>(
    b: &mut Battle<'_, N>,
    pokemon: crate::state::PokemonRef,
) {
    let mon = b.mon(pokemon);
    let Some(base) = mon.transformed else {
        return;
    };
    let (moves, old) = (mon.moves, mon.forme());
    b.apply(Instruction::SetMoves {
        target: pokemon,
        old: moves,
        new: base.moves,
    });
    b.apply(Instruction::SetTransformed {
        target: pokemon,
        old: Some(base),
        new: None,
    });
    if old.species != base.species {
        b.apply(Instruction::SetForme {
            target: pokemon,
            old,
            new: Forme {
                species: base.species,
                ..old
            },
        });
    }
}
