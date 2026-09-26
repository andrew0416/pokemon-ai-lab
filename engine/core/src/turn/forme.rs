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
use crate::field::{Terrain, Weather};
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
        f if f == species::WISHIWASHI_SCHOOL => Some(species::WISHIWASHI),
        f if f == species::MINIOR_METEOR => Some(species::MINIOR),
        f if f == species::MORPEKO_HANGRY => Some(species::MORPEKO),
        f if f == species::DARMANITAN_ZEN => Some(species::DARMANITAN),
        f if f == species::DARMANITAN_GALAR_ZEN => Some(species::DARMANITAN_GALAR),
        f if f == species::CHERRIM_SUNSHINE => Some(species::CHERRIM),
        // Relic Song (`moves::handlers::after_move_secondary_self`).
        f if f == species::MELOETTA_PIROUETTE => Some(species::MELOETTA),
        f if f == species::CRAMORANT_GULPING || f == species::CRAMORANT_GORGING => {
            Some(species::CRAMORANT)
        }
        _ => None,
    }
}

// ---- Gulp Missile ------------------------------------------------------------------------------

/// Gulp Missile catching its prey: its `onSourceTryPrimaryHit` for Surf (for each target of the
/// hit, before the substitute's handler) and Dive's charging `onTryMove` (before `ChargeMove`):
/// a Cramorant (`species.name === 'Cramorant'`) with Gulp Missile (`cantsuppress`) becomes
/// Cramorant-Gorging at half its max HP or less, else Cramorant-Gulping (a temporary forme with
/// Cramorant's types and stats).
pub(crate) fn gulp_missile_catch<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) {
    let Some(pokemon) = b.occupant(user) else {
        return;
    };
    let mon = b.mon(pokemon);
    if b.ability(user) != abilities::GULP_MISSILE || mon.species != species::CRAMORANT {
        return;
    }
    let forme = if 2 * i32::from(mon.hp) <= i32::from(mon.max_hp) {
        species::CRAMORANT_GORGING
    } else {
        species::CRAMORANT_GULPING
    };
    forme_change(b, user, forme, Change::Temporary);
}

/// Gulp Missile's `onDamagingHit` for the Cramorant in `holder` hit by the Pokémon in
/// `attacker`: with an attacker that has HP and a holder that is not semi-invulnerable, a
/// Cramorant-Gulping or -Gorging spits its prey: `this.damage(source.baseMaxhp / 4, source,
/// target)` (Magic Guard stops it), then Gulping's `this.boost({def: -1}, source, target, null,
/// true)` or Gorging's `source.trySetStatus('par', target, move)`, and it becomes Cramorant again
/// (`formeChange('cramorant', move)`).
pub(crate) fn gulp_missile_spit<const N: usize>(
    b: &mut Battle<'_, N>,
    holder: SlotRef,
    attacker: SlotRef,
) {
    let Some(pokemon) = b.occupant(holder) else {
        return;
    };
    let forme = b.mon(pokemon).species;
    let semi_invulnerable = [
        Volatile::Fly,
        Volatile::Bounce,
        Volatile::Dive,
        Volatile::Dig,
        Volatile::PhantomForce,
        Volatile::ShadowForce,
    ]
    .into_iter()
    .any(|v| b.volatile(holder, v).active);
    let full = forme == species::CRAMORANT_GULPING || forme == species::CRAMORANT_GORGING;
    let Some(source) = b.alive(attacker) else {
        return;
    };
    if semi_invulnerable || !full {
        return;
    }
    let max_hp = f64::from(b.mon(source).max_hp);
    b.damage(attacker, max_hp / 4.0, DamageSource::Indirect);
    if forme == species::CRAMORANT_GULPING {
        let mut drop = crate::dex::NO_BOOSTS;
        drop[1] = -1;
        b.boost_by(
            attacker,
            &drop,
            Some(holder),
            super::battle::BoostEffect::Ability(abilities::GULP_MISSILE),
        );
    } else {
        b.try_set_status_from(attacker, crate::state::Status::Paralyze, Some(holder));
    }
    forme_change(b, holder, species::CRAMORANT, Change::Temporary);
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
    // `setSpecies`: `this.speed = this.storedStats.spe` until the next `updateSpeed()`, and the
    // new forme's weight (Autotomize's reductions end).
    b.species_set(slot);
    b.reset_autotomize(pokemon);
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
    // `setType` / `setSpecies` drop the added type (Forest's Curse, Trick-or-Treat).
    if b.occupant(slot) == Some(pokemon) {
        super::conditions::clear_added_type(b, slot);
    }
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
/// !move.infiltrates`; Infiltrator sets `infiltrates` in the user's ModifyMove:
/// `ActiveMoveRef::infiltrates`).
fn hits_substitute<const N: usize>(
    b: &Battle<'_, N>,
    _user: SlotRef,
    target: SlotRef,
    id: MoveId,
) -> bool {
    b.state.slot(target).substitute_hp > 0
        && !id.data().flags.contains(MoveFlags::BYPASSSUB)
        && !b.active_move.is_some_and(|m| m.infiltrates)
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
    // `move.category`: after ModifyMove (Photon Geyser, Shell Side Arm).
    let category = b.move_category(id);
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
        .map_or(MoveCategory::Status, |m| m.category);
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

// ---- HP-dependent formes: Schooling, Shields Down; Hunger Switch ------------------------------

/// Schooling's `onStart` and `onResidual`: a Wishiwashi (`baseSpecies.baseSpecies`, level 20 or
/// more: always at level 50) above 1/4 of its max HP takes the School forme, at or below it the
/// Solo forme, temporarily.
fn schooling<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return;
    };
    if mon.species.data().base_species != species::WISHIWASHI {
        return;
    }
    // `pokemon.hp > pokemon.maxhp / 4`.
    let school = 4 * i32::from(mon.hp) > i32::from(mon.max_hp);
    if school && mon.species == species::WISHIWASHI {
        forme_change(b, slot, species::WISHIWASHI_SCHOOL, Change::Temporary);
    } else if !school && mon.species == species::WISHIWASHI_SCHOOL {
        forme_change(b, slot, species::WISHIWASHI, Change::Temporary);
    }
}

/// Shields Down's `onStart` and `onResidual`: a Minior (`baseSpecies.baseSpecies`) above half
/// its max HP takes the Meteor forme, at or below half its core (`pokemon.set.species`),
/// temporarily. The set's species is not in the state: the engine only follows plain Minior
/// (whose core is Minior itself); a core of another colour would have to come back from the
/// Meteor forme, so its change to Meteor is refused.
fn shields_down<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) -> Result<(), TurnError> {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return Ok(());
    };
    if mon.species.data().base_species != species::MINIOR {
        return Ok(());
    }
    // `pokemon.hp > pokemon.maxhp / 2`.
    let meteor = 2 * i32::from(mon.hp) > i32::from(mon.max_hp);
    if meteor && mon.species != species::MINIOR_METEOR {
        if mon.species != species::MINIOR {
            return Err(b.unsupported(format!(
                "Shields Down on {}: the core colour (the set's species) is not in the state",
                mon.species.data().name
            )));
        }
        forme_change(b, slot, species::MINIOR_METEOR, Change::Temporary);
    } else if !meteor && mon.species == species::MINIOR_METEOR {
        forme_change(b, slot, species::MINIOR, Change::Temporary);
    }
    Ok(())
}

/// Shields Down's `onSetStatus` (every status, from any source) and `onTryAddVolatile` (Yawn):
/// whether the holder in `slot` is protected, i.e. is Minior-Meteor. Not breakable.
pub(crate) fn shields_up<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    b.slot_mon(slot).is_some_and(|m| {
        m.ability == abilities::SHIELDS_DOWN && m.species == species::MINIOR_METEOR
    })
}

/// Hunger Switch's `onResidual`: a Morpeko (`species.baseSpecies`) switches between its Full
/// Belly and Hangry formes every turn, temporarily (Terastallization, which stops it, is off).
fn hunger_switch<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return;
    };
    if mon.species.data().base_species != species::MORPEKO {
        return;
    }
    let target = if mon.species == species::MORPEKO {
        species::MORPEKO_HANGRY
    } else {
        species::MORPEKO
    };
    forme_change(b, slot, target, Change::Temporary);
}

/// Zen Mode's `onResidual` for a Darmanitan (`baseSpecies.baseSpecies`) in `slot`: at or below
/// half its max HP outside a Zen forme it gets the `zenmode` volatile, whose `onStart` changes
/// it to Darmanitan-Zen (Darmanitan-Galar-Zen for a Galarian one) temporarily; above half in a
/// Zen forme the volatile is removed (`addVolatile` first does nothing: the volatile exists
/// exactly while the Pokémon is in a Zen forme, the loader refusing a Zen forme as a set's
/// species), and its `onEnd` changes it back to its `battleOnly` forme. Nothing blocks the
/// volatile (no `TryAddVolatile` handler names it).
///
/// Leaving the field, the ability's `onEnd` (a healthy Pokémon: delete the volatile, back to
/// the `battleOnly` forme) and `clearVolatile` (the volatiles go, `setSpecies(baseSpecies)`)
/// give the same as [`revert_on_leave`].
fn zen_mode<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some((forme, hp, max_hp)) = b
        .alive(slot)
        .map(|p| b.mon(p))
        .map(|m| (m.species, m.hp, m.max_hp))
    else {
        return;
    };
    if forme.data().base_species != species::DARMANITAN {
        return;
    }
    let zen = [species::DARMANITAN_ZEN, species::DARMANITAN_GALAR_ZEN].contains(&forme);
    // `pokemon.hp <= pokemon.maxhp / 2`.
    let low = 2 * i32::from(hp) <= i32::from(max_hp);
    if low && !zen {
        // `condition.onStart`: `pokemon.species.name.includes('Galar')`.
        let galar = forme == species::DARMANITAN_GALAR;
        b.set_volatile_state(
            slot,
            Volatile::ZenMode,
            VolatileState {
                active: true,
                ..VolatileState::NONE
            },
        );
        let target = if galar {
            species::DARMANITAN_GALAR_ZEN
        } else {
            species::DARMANITAN_ZEN
        };
        forme_change(b, slot, target, Change::Temporary);
    } else if !low && zen {
        // `condition.onEnd`: `formeChange(pokemon.species.battleOnly)`.
        b.set_volatile_state(slot, Volatile::ZenMode, VolatileState::NONE);
        let base = temporary_forme_base(forme).expect("a Zen forme");
        forme_change(b, slot, base, Change::Temporary);
    }
}

/// Whether `ability` has a forme-changing `onResidual` ([`residual`]).
pub(crate) fn has_residual(ability: AbilityId) -> bool {
    [
        abilities::SCHOOLING,
        abilities::SHIELDS_DOWN,
        abilities::HUNGER_SWITCH,
        abilities::ZEN_MODE,
    ]
    .contains(&ability)
}

/// The ability's `onResidual` (`residual.rs`, order 29, ability sub-order) for its holder in
/// `slot`: Schooling, Shields Down, Hunger Switch, Zen Mode. Each only changes its holder.
pub(crate) fn residual<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    match ability {
        a if a == abilities::SCHOOLING => schooling(b, slot),
        a if a == abilities::SHIELDS_DOWN => shields_down(b, slot)?,
        a if a == abilities::HUNGER_SWITCH => hunger_switch(b, slot),
        a if a == abilities::ZEN_MODE => zen_mode(b, slot),
        _ => {}
    }
    Ok(())
}

// ---- Mimicry ----------------------------------------------------------------------------------

/// Mimicry's `onTerrainChange` (also its `onStart`, `singleEvent('TerrainChange')`) for the
/// holder in `slot`: its types become the terrain's type (Electric, Grass, Fairy, Psychic), or
/// its base species' types without a terrain (`pokemon.baseSpecies.types`), unless they already
/// are (`getTypes().join()`) or `setType` fails (Arceus and Silvally; Terastallization is off).
/// Not breakable. `field.terrain` is read as is (no terrain suppression exists).
fn mimicry<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let mon = b.mon(pokemon);
    let types = match b.terrain() {
        Terrain::Electric => [Type::Electric, Type::None],
        Terrain::Grassy => [Type::Grass, Type::None],
        Terrain::Misty => [Type::Fairy, Type::None],
        Terrain::Psychic => [Type::Psychic, Type::None],
        Terrain::None => {
            temporary_forme_base(mon.species)
                .unwrap_or(mon.species)
                .data()
                .types
        }
    };
    let num = mon.species.data().num;
    // `oldTypes.join() === types.join()` with `oldTypes = pokemon.getTypes()` (an added type
    // counts).
    if b.types(slot) == [types[0], types[1], Type::None] || num == 493 || num == 773 {
        return;
    }
    set_types(b, slot, pokemon, types);
}

// ---- what stays refused -----------------------------------------------------------------------

/// Why a Pokémon cannot be on the field, if its forme ability makes it unsupported although
/// the ability is supported for other species (`support::check_state`, and
/// `switching::switch_in_problem` for a switch-in during a turn):
/// - Battle Bond acts only for Greninja-Bond (`onSourceAfterFaint`: +1 Atk, SpA and Spe once per
///   battle, `source.bondTriggered`, which the state does not record) and Greninja-Ash (Water
///   Shuriken hits 3 times); on any other species (Greninja itself) both handlers do nothing.
pub(crate) fn field_problem(mon: &crate::state::Pokemon) -> Option<String> {
    // Symbiosis holding an item it could not pass (`abilities::item_moves`).
    if let Some(why) = super::abilities::symbiosis_problem(mon) {
        return Some(why);
    }
    let bond_forme = mon.species == species::GRENINJA_BOND || mon.species == species::GRENINJA_ASH;
    (mon.ability == abilities::BATTLE_BOND && bond_forme).then(|| {
        format!(
            "{}: Battle Bond (its once-per-battle `bondTriggered` is not in the state)",
            mon.species.data().name
        )
    })
}

// ---- switch-in and field events ---------------------------------------------------------------

/// `singleEvent('Start')` of a forme ability (`switching::StartEffect::Forme`, run in the
/// switch-in's `fieldEvent('SwitchIn')` at the ability's `onSwitchInPriority`): Ice Face,
/// Schooling, Shields Down.
pub(crate) fn on_start<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    match ability {
        a if a == abilities::ICE_FACE => ice_face_restore(b, slot),
        a if a == abilities::SCHOOLING => schooling(b, slot),
        a if a == abilities::SHIELDS_DOWN => shields_down(b, slot)?,
        a if a == abilities::MIMICRY => mimicry(b, slot),
        // Flower Gift's `onStart` (`onSwitchInPriority: -2`): `singleEvent('WeatherChange')`,
        // which no ability-ignoring move can suppress.
        a if a == abilities::FLOWER_GIFT => flower_gift(b, slot, false),
        _ => {}
    }
    Ok(())
}

/// The forme abilities' `onWeatherChange` for the Pokémon in `slot`
/// (`field_events::weather_changed`): Ice Face, Flower Gift.
pub(crate) fn weather_changed<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.ability(slot) == abilities::ICE_FACE {
        ice_face_restore(b, slot);
    }
    if b.ability(slot) == abilities::FLOWER_GIFT {
        flower_gift(b, slot, true);
    }
}

// ---- Flower Gift ------------------------------------------------------------------------------

/// Flower Gift's `onWeatherChange` for the Pokémon in `slot` (also run by its `onStart`): a
/// Cherrim (`baseSpecies.baseSpecies`) with HP takes the Sunshine forme in harsh sunlight
/// (`pokemon.effectiveWeather()`: the suppressors and its own Utility Umbrella hide the sun) and
/// goes back to Cherrim otherwise, temporarily (`formeChange(..., this.effect, false)`: the
/// base returns when it leaves the field). The ability is breakable: in the WeatherChange event
/// (`in_event`) a weather that an ability-ignoring move changes skips it.
pub(crate) fn flower_gift<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, in_event: bool) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let ability = if in_event {
        b.ability_unless_broken(slot)
    } else {
        b.ability(slot)
    };
    let current = b.mon(pokemon).species;
    if ability != abilities::FLOWER_GIFT
        || (current != species::CHERRIM && current != species::CHERRIM_SUNSHINE)
    {
        return;
    }
    let sun = b.weather_for(slot) == Weather::Sun;
    if sun && current != species::CHERRIM_SUNSHINE {
        forme_change(b, slot, species::CHERRIM_SUNSHINE, Change::Temporary);
    } else if !sun && current == species::CHERRIM_SUNSHINE {
        forme_change(b, slot, species::CHERRIM, Change::Temporary);
    }
}

/// Flower Gift's `onAllyModifyAtk` (priority 3) and `onAllyModifySpD` (priority 4) holders for
/// the Pokémon in `slot` whose stat the event modifies: an active Cherrim (the holder's
/// `baseSpecies.baseSpecies`, either forme) with Flower Gift on its side, itself included
/// (`alliesAndSelf()`), as `user`'s move sees it (breakable), while the modified Pokémon is in
/// harsh sunlight (`pokemon.effectiveWeather()`).
pub(crate) fn flower_gift_holders<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
    user: SlotRef,
    data: &crate::dex::MoveData,
) -> Vec<SlotRef> {
    if b.weather_for(slot) != Weather::Sun {
        return Vec::new();
    }
    b.alive_slots(slot.side)
        .into_iter()
        .filter(|&holder| {
            super::abilities::ability_for_move(b, holder, user, data) == abilities::FLOWER_GIFT
                && b.slot_mon(holder).is_some_and(|m| {
                    m.species == species::CHERRIM || m.species == species::CHERRIM_SUNSHINE
                })
        })
        .collect()
}

/// The abilities' `onTerrainChange` for the Pokémon in `slot` (`field_events::terrain_changed`,
/// before its item's): Mimicry.
pub(crate) fn terrain_changed<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.ability(slot) == abilities::MIMICRY {
        mimicry(b, slot);
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

    /// Every temporary forme is a battle-only forme of its base with the same base HP (a
    /// temporary change keeps max HP and the ability; `revert_on_leave` restores the base's
    /// types).
    #[test]
    fn temporary_formes_match_the_dex() {
        for id in SpeciesId::all() {
            let Some(base) = temporary_forme_base(id) else {
                continue;
            };
            let (forme, base_data) = (id.data(), base.data());
            assert_eq!(forme.battle_only, [base], "{id:?}");
            assert_eq!(forme.base_stats[0], base_data.base_stats[0], "{id:?}");
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

    /// The residual forme abilities' handler lists and orders (order 29, no sub-order: the
    /// ability's effect-type sub-order in `residual.rs`), and the formes' temporary bases.
    #[test]
    fn residual_formes_match_the_dex() {
        for (ability, handlers, switch_in) in [
            (
                abilities::SCHOOLING,
                &["onResidual", "onStart"][..],
                Some(-1),
            ),
            (
                abilities::SHIELDS_DOWN,
                &["onResidual", "onSetStatus", "onStart", "onTryAddVolatile"][..],
                Some(-1),
            ),
            (abilities::HUNGER_SWITCH, &["onResidual"][..], None),
            (
                abilities::ZEN_MODE,
                &[
                    "condition.onEnd",
                    "condition.onStart",
                    "onEnd",
                    "onResidual",
                ][..],
                None,
            ),
        ] {
            let data = ability.data();
            assert!(has_residual(ability));
            assert_eq!(data.handlers, handlers, "{ability:?}");
            assert!(data.event_orders.contains(&("onResidualOrder", 29)));
            assert!(!data
                .event_orders
                .iter()
                .any(|(name, _)| *name == "onResidualSubOrder"));
            let priority = data
                .event_orders
                .iter()
                .find(|(name, _)| *name == "onSwitchInPriority")
                .map(|&(_, p)| p);
            assert_eq!(priority, switch_in, "{ability:?}");
        }
        for (forme, base) in [
            (species::WISHIWASHI_SCHOOL, species::WISHIWASHI),
            (species::MINIOR_METEOR, species::MINIOR),
            (species::MORPEKO_HANGRY, species::MORPEKO),
            (species::DARMANITAN_ZEN, species::DARMANITAN),
            (species::DARMANITAN_GALAR_ZEN, species::DARMANITAN_GALAR),
        ] {
            assert_eq!(temporary_forme_base(forme), Some(base));
        }
        assert_eq!(crate::volatile::Volatile::ZenMode.id(), "zenmode");
    }

    /// Mimicry's handlers (not breakable: the TerrainChange event reads `b.ability`).
    #[test]
    fn mimicry_matches_the_dex() {
        let data = abilities::MIMICRY.data();
        assert_eq!(data.handlers, ["onStart", "onTerrainChange"]);
        assert!(data.event_orders.contains(&("onSwitchInPriority", -1)));
        assert!(!data.flags.contains(crate::dex::AbilityFlags::BREAKABLE));
    }

    /// The F19 ability that stays refused, with the reason pinned here: Power Construct
    /// (Zygarde-Complete is permanent but regresses on fainting (`formeRegression`) to the set's
    /// species (Zygarde or Zygarde-10%, not in the state) with `updateMaxHp`, and it recomputes
    /// `canMegaEvo`). Gulp Missile is implemented (Opus U: `gulp_missile_catch` / `_spit`).
    ///
    /// Battle Bond is supported only where it is inert ([`field_problem`]).
    #[test]
    fn unimplemented_forme_abilities_stay_refused() {
        use crate::turn::support::ability_supported_on_field;
        assert!(!ability_supported_on_field(abilities::POWER_CONSTRUCT));
        assert!(ability_supported_on_field(abilities::GULP_MISSILE));
        assert_eq!(
            abilities::BATTLE_BOND.data().handlers,
            ["onModifyMove", "onSourceAfterFaint"]
        );
        let mut greninja = crate::state::Pokemon {
            species: species::GRENINJA,
            ability: abilities::BATTLE_BOND,
            ..Default::default()
        };
        assert_eq!(field_problem(&greninja), None);
        for bond in [species::GRENINJA_BOND, species::GRENINJA_ASH] {
            greninja.species = bond;
            assert!(field_problem(&greninja).is_some(), "{bond:?}");
        }
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
