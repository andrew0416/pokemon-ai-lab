//! In-battle forme changes (WORKPLAN F19; Mega Evolution is `mega.rs`): Showdown
//! `formeChange` / `setSpecies` and the abilities that call them.
//!
//! Showdown `formeChange(species, source, isPermanent)` (the Champions override in
//! `data/mods/champions/scripts.ts` only drops the Mega `formeRegression`):
//! - `setSpecies`: species, types (`setType(species.types, true)`), weight and the stored
//!   stats (`spreadModify` of the set's nature and SP) become the forme's; max HP does not
//!   change (it is only set when the Pokémon has none yet).
//! - A permanent change (`isPermanent`) also makes the forme the Pokémon's `baseSpecies` (so it
//!   stays after switching out and, in Champions, after fainting: an ability's change never
//!   sets `formeRegression`), runs `updateMaxHp` (the HP lost so far is kept), and sets the
//!   forme's first ability (`setAbility(..., isFromFormeChange)`) unless the source is
//!   Disguise or Ice Face.
//! - A temporary change lasts while the Pokémon is active: `clearVolatile` (switch-out,
//!   fainting) ends with `setSpecies(baseSpecies)` ([`revert_on_leave`]).
//!
//! The state has no `baseSpecies`. The temporary formes the engine creates have a single base
//! ([`temporary_forme_base`]), and the scenario loader refuses them as a set's species, so the
//! base of a Pokémon in one of those formes is always that base.
//!
//! Types under Roost: Showdown filters Flying out of `getTypes()` while Roost is up; the engine
//! stores the filtered types and keeps the real ones in the volatile ([`set_types`]).

use crate::dex::{abilities, species, MoveCategory, MoveFlags, MoveId, SpeciesId, Type};
use crate::instruction::Instruction;
use crate::state::{Forme, PokemonRef, SlotRef};
use crate::volatile::{encode_types, Volatile, VolatileState};

use super::battle::{Battle, DamageSource};

/// How a forme change treats the Pokémon's base species and ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// `formeChange(species)`: lasts until the Pokémon leaves the field; max HP and ability stay.
    Temporary,
    /// `formeChange(species, disguise | iceface, true)`: the new base species with
    /// `updateMaxHp`; the ability stays.
    PermanentKeepAbility,
}

/// The base species a temporary forme created by an implemented ability returns to when its
/// Pokémon leaves the field. `None` for every other species (a permanent forme, or one the
/// engine never creates temporarily); the scenario loader refuses the `Some` ones as a set's
/// species, since Showdown would then keep them as the base.
pub fn temporary_forme_base(forme: SpeciesId) -> Option<SpeciesId> {
    let _ = forme;
    None
}

/// Showdown `formeChange` for the Pokémon in `slot` (see the module docs).
pub(crate) fn forme_change<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    species: SpeciesId,
    change: Change,
) {
    let Some(pokemon) = b.occupant(slot) else {
        return;
    };
    let mon = b.mon(pokemon);
    let old = mon.forme();
    let target = mon.forme_as(species);
    let permanent = change != Change::Temporary;
    let new = Forme {
        species,
        types: old.types,
        max_hp: if permanent { target.max_hp } else { old.max_hp },
        stats: target.stats,
        ability: old.ability,
        base_ability: old.base_ability,
    };
    let hp = mon.hp;
    let new_hp = new.hp_after(old.max_hp, hp);
    if new != old {
        b.apply(Instruction::SetForme {
            target: pokemon,
            old,
            new,
        });
    }
    // `setType(species.types, true)`, through Roost's filter.
    set_types(b, slot, pokemon, target.types);
    if new_hp != hp {
        // `updateMaxHp` adjusts HP silently, outside `damage`/`heal`.
        let amount = hp - new_hp;
        if amount > 0 {
            b.apply(Instruction::Damage {
                target: pokemon,
                amount,
            });
        } else {
            b.apply(Instruction::Heal {
                target: pokemon,
                amount: -amount,
            });
        }
    }
}

/// `clearVolatile`'s `setSpecies(baseSpecies)` for a Pokémon leaving the field (switch-out or
/// fainting): a temporary forme returns to its base species' types and stats; max HP, HP and
/// the (already reverted) ability stay.
pub(crate) fn revert_on_leave<const N: usize>(b: &mut Battle<'_, N>, pokemon: PokemonRef) {
    let mon = b.mon(pokemon);
    let Some(base) = temporary_forme_base(mon.species) else {
        return;
    };
    let old = mon.forme();
    let target = mon.forme_as(base);
    b.apply(Instruction::SetForme {
        target: pokemon,
        old,
        new: Forme {
            species: base,
            types: target.types,
            stats: target.stats,
            ..old
        },
    });
}

/// Showdown `setType(types)` on a Pokémon that may be roosting: the stored types keep Roost's
/// filter (Flying left out, Normal when nothing is left) and the volatile remembers the real
/// types to restore when it ends (nothing to restore without Flying), as
/// `moves::handlers::set_types` does.
fn set_types<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    pokemon: PokemonRef,
    types: [Type; 2],
) {
    let roost = b.volatile(slot, Volatile::Roost);
    let roosting = roost.active && b.occupant(slot) == Some(pokemon);
    let shown = if roosting && types.contains(&Type::Flying) {
        let mut kept = types
            .into_iter()
            .filter(|&t| t != Type::Flying && t != Type::None);
        [
            kept.next().unwrap_or(Type::Normal),
            kept.next().unwrap_or(Type::None),
        ]
    } else {
        types
    };
    if roosting {
        let counter = if shown == types {
            0
        } else {
            encode_types(types)
        };
        b.set_volatile_state(slot, Volatile::Roost, VolatileState { counter, ..roost });
    }
    let old = b.mon(pokemon).types;
    if old != shown {
        b.apply(Instruction::SetTypes {
            target: pokemon,
            old,
            new: shown,
        });
    }
}

// ---- Disguise ---------------------------------------------------------------------------------

/// The formes whose Disguise still works (`['mimikyu', 'mimikyutotem'].includes(species.id)`).
fn disguised(species: SpeciesId) -> bool {
    species == species::MIMIKYU || species == species::MIMIKYU_TOTEM
}

/// Showdown's `hitSub` test in the Disguise and Ice Face handlers: the target's substitute
/// takes the hit (`target.volatiles['substitute'] && !move.flags['bypasssub'] &&
/// !move.infiltrates`; Infiltrator sets `infiltrates` in the user's ModifyMove).
fn hits_substitute<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    id: MoveId,
) -> bool {
    b.state.slot(target).substitute_hp > 0
        && !id.data().flags.contains(MoveFlags::BYPASSSUB)
        && b.ability(user) != abilities::INFILTRATOR
}

/// Whether the target's ability cancels a critical hit (`onCriticalHit` returning `false`) and
/// makes every type neutral (`onEffectiveness` returning 0, which ends the Effectiveness event
/// for each of the target's types) for `user`'s damaging move `id`: Disguise on an undisguised
/// Mimikyu. Both handlers are breakable and skip a hit on a substitute; their
/// `runImmunity(move)` test always passes here, since `getDamage` stops at an immunity first.
pub(crate) fn shields_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    id: MoveId,
) -> bool {
    let Some(mon) = b.slot_mon(target) else {
        return false;
    };
    let ability = b.ability_unless_broken(target);
    let shielded = ability == abilities::DISGUISE && disguised(mon.species);
    shielded && id.data().category != MoveCategory::Status && !hits_substitute(b, user, target, id)
}

/// The target's `onDamage` handler of priority 1 (the first Damage handler; returning 0 ends
/// the event, so no other Damage handler runs) for a move's damage (`effect.effectType ===
/// 'Move'`: the move's hits and the confusion self-hit): Disguise on an undisguised Mimikyu
/// sets `effectState.busted` and the damage becomes 0. Breakable. The forme changes at the
/// next Update ([`on_update`]). Returns whether the damage was absorbed.
pub(crate) fn absorbs_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    pokemon: PokemonRef,
) -> bool {
    let ability = b.ability_unless_broken(target);
    let absorbed = ability == abilities::DISGUISE && disguised(b.mon(pokemon).species);
    if absorbed && !b.busted.contains(&pokemon) {
        b.busted.push(pokemon);
    }
    absorbed
}

/// The ability `onUpdate` of Disguise for the Pokémon in `slot` (`eachEvent('Update')`;
/// breakable, so a move that ignores abilities postpones it to a later Update): a busted
/// Mimikyu becomes Mimikyu-Busted (Mimikyu-Busted-Totem) permanently, keeping its ability,
/// then loses 1/8 of its max HP (`this.damage(pokemon.baseMaxhp / 8, pokemon, pokemon,
/// species)`: not a move's damage).
pub(crate) fn on_update<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let Some(index) = b.busted.iter().position(|&p| p == pokemon) else {
        return;
    };
    let species = b.mon(pokemon).species;
    if b.ability_unless_broken(slot) == abilities::DISGUISE && disguised(species) {
        b.busted.remove(index);
        let busted = if species == species::MIMIKYU_TOTEM {
            species::MIMIKYU_BUSTED_TOTEM
        } else {
            species::MIMIKYU_BUSTED
        };
        forme_change(b, slot, busted, Change::PermanentKeepAbility);
        let max_hp = f64::from(b.mon(pokemon).max_hp);
        b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disguise's handlers, the busted formes, and what their data keeps the same.
    #[test]
    fn disguise_matches_the_dex() {
        assert_eq!(
            abilities::DISGUISE.data().handlers,
            ["onCriticalHit", "onDamage", "onEffectiveness", "onUpdate"]
        );
        assert!(abilities::DISGUISE
            .data()
            .event_orders
            .contains(&("onDamagePriority", 1)));
        for (from, to) in [
            (species::MIMIKYU, species::MIMIKYU_BUSTED),
            (species::MIMIKYU_TOTEM, species::MIMIKYU_BUSTED_TOTEM),
        ] {
            assert_eq!(from.data().base_stats, to.data().base_stats);
            assert_eq!(from.data().types, to.data().types);
            assert_eq!(to.data().battle_only, [from]);
        }
    }
}
