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

use crate::dex::{abilities, species, AbilityId, MoveCategory, MoveFlags, MoveId, SpeciesId, Type};
use crate::field::Weather;
use crate::instruction::Instruction;
use crate::state::{Forme, PokemonRef, SlotRef};
use crate::volatile::{encode_types, Volatile, VolatileState};

use super::battle::{Battle, DamageSource};
use super::TurnError;

/// How a forme change treats the Pokémon's base species and ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// `formeChange(species)`: lasts until the Pokémon leaves the field; max HP and ability stay.
    Temporary,
    /// `formeChange(species, disguise | iceface, true)`: the new base species with
    /// `updateMaxHp`; the ability stays.
    PermanentKeepAbility,
    /// `formeChange(species, effect, true)` from another ability: also the forme's first
    /// ability, as the ability and its base (`setAbility(..., isFromFormeChange)`). The only
    /// caller, Zero to Hero, keeps Zero to Hero, which has neither `onEnd` nor `onStart`, so
    /// `setAbility`'s End and Start do nothing.
    Permanent,
}

/// The base species a temporary forme created by an implemented ability returns to when its
/// Pokémon leaves the field. `None` for every other species (a permanent forme, or one the
/// engine never creates temporarily); the scenario loader refuses the `Some` ones as a set's
/// species, since Showdown would then keep them as the base.
pub fn temporary_forme_base(forme: SpeciesId) -> Option<SpeciesId> {
    match forme {
        f if f == species::AEGISLASH_BLADE => Some(species::AEGISLASH),
        _ => None,
    }
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
    let (ability, base_ability) = if change == Change::Permanent {
        debug_assert_eq!(old.ability, target.ability, "End/Start would have to run");
        (target.ability, target.base_ability)
    } else {
        (old.ability, old.base_ability)
    };
    let new = Forme {
        species,
        types: old.types,
        max_hp: if permanent { target.max_hp } else { old.max_hp },
        stats: target.stats,
        ability,
        base_ability,
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

// ---- Disguise and Ice Face --------------------------------------------------------------------

/// The formes whose Disguise still works (`['mimikyu', 'mimikyutotem'].includes(species.id)`).
fn disguised(species: SpeciesId) -> bool {
    species == species::MIMIKYU || species == species::MIMIKYU_TOTEM
}

/// Whether the ability shields its holder of species `species` from a move of `category` (the
/// condition shared by the ability's `onDamage`, `onCriticalHit` and `onEffectiveness`):
/// Disguise on an undisguised Mimikyu against any move, Ice Face on Eiscue (with its face)
/// against a physical move.
fn shield_up(ability: AbilityId, species: SpeciesId, category: MoveCategory) -> bool {
    (ability == abilities::DISGUISE && disguised(species))
        || (ability == abilities::ICE_FACE
            && species == species::EISCUE
            && category == MoveCategory::Physical)
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
/// Mimikyu, Ice Face on Eiscue against a physical move. The handlers are breakable and skip a
/// hit on a substitute; their `runImmunity(move)` test always passes here, since `getDamage`
/// stops at an immunity first.
pub(crate) fn shields_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    id: MoveId,
) -> bool {
    let Some(mon) = b.slot_mon(target) else {
        return false;
    };
    let category = id.data().category;
    category != MoveCategory::Status
        && shield_up(b.ability_unless_broken(target), mon.species, category)
        && !hits_substitute(b, user, target, id)
}

/// The target's `onDamage` handler of priority 1 (the first Damage handler; returning 0 ends
/// the event, so no other Damage handler runs) for a move's damage (`effect.effectType ===
/// 'Move'`: the move's hits and the confusion self-hit): Disguise on an undisguised Mimikyu,
/// or Ice Face on Eiscue against a physical move (`effect.category === 'Physical'`; the
/// confusion self-hit has no category), sets `effectState.busted` and the damage becomes 0.
/// Both are breakable. The forme changes at the next Update ([`on_update`]). Returns whether
/// the damage was absorbed.
///
/// A move's damage to its own user can only be the confusion self-hit (no damaging move can
/// target its user), which is how the self-hit is told apart from the move in progress.
pub(crate) fn absorbs_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    pokemon: PokemonRef,
) -> bool {
    let category = b
        .active_move
        .filter(|m| m.user != target)
        .map_or(MoveCategory::Status, |m| m.id.data().category);
    let ability = b.ability_unless_broken(target);
    let species = b.mon(pokemon).species;
    let absorbed = (ability == abilities::DISGUISE && disguised(species))
        || shield_up(ability, species, category);
    if absorbed && !b.busted.contains(&pokemon) {
        b.busted.push(pokemon);
    }
    absorbed
}

/// The ability `onUpdate` of Disguise and Ice Face for the Pokémon in `slot`
/// (`eachEvent('Update')`; breakable, so a move that ignores abilities postpones them to a
/// later Update):
/// - a busted Mimikyu becomes Mimikyu-Busted (Mimikyu-Busted-Totem) permanently, keeping its
///   ability, then loses 1/8 of its max HP (`this.damage(pokemon.baseMaxhp / 8, pokemon,
///   pokemon, species)`: not a move's damage);
/// - a busted Eiscue becomes Eiscue-Noice permanently, keeping its ability.
pub(crate) fn on_update<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let Some(index) = b.busted.iter().position(|&p| p == pokemon) else {
        return;
    };
    let species = b.mon(pokemon).species;
    let ability = b.ability_unless_broken(slot);
    if ability == abilities::DISGUISE && disguised(species) {
        b.busted.remove(index);
        let busted = if species == species::MIMIKYU_TOTEM {
            species::MIMIKYU_BUSTED_TOTEM
        } else {
            species::MIMIKYU_BUSTED
        };
        forme_change(b, slot, busted, Change::PermanentKeepAbility);
        let max_hp = f64::from(b.mon(pokemon).max_hp);
        b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
    } else if ability == abilities::ICE_FACE && species == species::EISCUE {
        b.busted.remove(index);
        forme_change(b, slot, species::EISCUE_NOICE, Change::PermanentKeepAbility);
    }
}

/// Ice Face's `onStart` (switch-in, `onSwitchInPriority: -2`) and `onWeatherChange` (every
/// `eachEvent('WeatherChange')` except the one an Air Lock / Cloud Nine start or end runs, which
/// it ignores: `sourceEffect.suppressWeather`): in snow (`field.isWeather`, the effective
/// weather) Eiscue-Noice gets its face back (`effectState.busted = false`,
/// `formeChange('Eiscue', this.effect, true)`: permanent, ability kept). `onWeatherChange` also
/// needs HP. The ability is breakable, so a weather that a Mold Breaker's move sets (its event
/// runs while the move is active) skips it.
pub(crate) fn ice_face_restore<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    if b.ability_unless_broken(slot) != abilities::ICE_FACE
        || b.mon(pokemon).species != species::EISCUE_NOICE
        || b.effective_weather() != Weather::Snow
    {
        return;
    }
    b.busted.retain(|&p| p != pokemon);
    forme_change(b, slot, species::EISCUE, Change::PermanentKeepAbility);
}

// ---- Stance Change ----------------------------------------------------------------------------

/// Stance Change's `onModifyMove` (priority 1) for the user in `user` using `id`: an Aegislash
/// (`species.baseSpecies`) takes the Shield forme for King's Shield and the Blade forme for any
/// damaging move, temporarily (`formeChange(targetForme)`); other status moves change nothing.
/// It runs in `useMoveInner`, so a move stopped in BeforeMove changes nothing, while a move
/// called by Sleep Talk does.
pub(crate) fn stance_change<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, id: MoveId) {
    let Some(mon) = b.slot_mon(user) else {
        return;
    };
    if mon.species.data().base_species != species::AEGISLASH {
        return;
    }
    let kings_shield = id == crate::dex::moves::KINGS_SHIELD;
    if id.data().category == MoveCategory::Status && !kings_shield {
        return;
    }
    let target = if kings_shield {
        species::AEGISLASH
    } else {
        species::AEGISLASH_BLADE
    };
    if mon.species != target {
        forme_change(b, user, target, Change::Temporary);
    }
}

// ---- Zero to Hero -----------------------------------------------------------------------------

/// `runEvent('SwitchOut')` for a healthy Pokémon leaving `slot` (next to
/// `abilities::on_switch_out`; a Pokémon has one ability): Zero to Hero turns a Palafin
/// (`baseSpecies.baseSpecies`) that is not in its Hero forme into Palafin-Hero permanently
/// (`formeChange('Palafin-Hero', this.effect, true)`), so it stays Hero on the bench and when it
/// comes back. Its `onSwitchIn` only shows a message (`heroMessageDisplayed`).
pub(crate) fn on_switch_out<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return;
    };
    if mon.ability == abilities::ZERO_TO_HERO
        && mon.species.data().base_species == species::PALAFIN
        && mon.species != species::PALAFIN_HERO
    {
        forme_change(b, slot, species::PALAFIN_HERO, Change::Permanent);
    }
}

// ---- switch-in and field events ---------------------------------------------------------------

/// `singleEvent('Start')` of a forme ability (`switching::StartEffect::Forme`, run in the
/// switch-in's `fieldEvent('SwitchIn')` at the ability's `onSwitchInPriority`).
pub(crate) fn on_start<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    if ability == abilities::ICE_FACE {
        ice_face_restore(b, slot);
    }
    Ok(())
}

/// The forme abilities' `onWeatherChange` for the Pokémon in `slot`
/// (`field_events::weather_changed`): Ice Face.
pub(crate) fn weather_changed<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.ability(slot) == abilities::ICE_FACE {
        ice_face_restore(b, slot);
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

    /// Every temporary forme is a battle-only forme of its base with the same types and base HP
    /// (a temporary change keeps max HP, and `revert_on_leave` restores the base's types).
    #[test]
    fn temporary_formes_match_the_dex() {
        for id in SpeciesId::all() {
            let Some(base) = temporary_forme_base(id) else {
                continue;
            };
            let (forme, base_data) = (id.data(), base.data());
            assert_eq!(forme.battle_only, [base], "{id:?}");
            assert_eq!(forme.base_stats[0], base_data.base_stats[0], "{id:?}");
            assert_eq!(forme.abilities[0], base_data.abilities[0], "{id:?}");
        }
        assert_eq!(
            temporary_forme_base(species::AEGISLASH_BLADE),
            Some(species::AEGISLASH)
        );
        assert_eq!(abilities::STANCE_CHANGE.data().handlers, ["onModifyMove"]);
        assert!(abilities::STANCE_CHANGE
            .data()
            .event_orders
            .contains(&("onModifyMovePriority", 1)));
    }

    /// Zero to Hero's permanent change keeps the ability (so `setAbility` has no End or Start to
    /// run) and the max HP.
    #[test]
    fn zero_to_hero_matches_the_dex() {
        let data = abilities::ZERO_TO_HERO.data();
        assert_eq!(data.handlers, ["onSwitchIn", "onSwitchOut"]);
        assert!(!data.handlers.contains(&"onEnd") && !data.handlers.contains(&"onStart"));
        let (palafin, hero) = (species::PALAFIN.data(), species::PALAFIN_HERO.data());
        assert_eq!(hero.abilities[0], abilities::ZERO_TO_HERO);
        assert_eq!(palafin.base_stats[0], hero.base_stats[0]);
        assert_eq!(palafin.types, hero.types);
    }

    /// Ice Face's handlers and orders; the Noice forme keeps the types and base HP.
    #[test]
    fn ice_face_matches_the_dex() {
        let data = abilities::ICE_FACE.data();
        assert_eq!(
            data.handlers,
            [
                "onCriticalHit",
                "onDamage",
                "onEffectiveness",
                "onStart",
                "onUpdate",
                "onWeatherChange"
            ]
        );
        assert!(data.event_orders.contains(&("onDamagePriority", 1)));
        assert!(data.event_orders.contains(&("onSwitchInPriority", -2)));
        let (face, noice) = (species::EISCUE.data(), species::EISCUE_NOICE.data());
        assert_eq!(face.types, noice.types);
        assert_eq!(face.base_stats[0], noice.base_stats[0]);
        assert_eq!(noice.battle_only, [species::EISCUE]);
    }
}
