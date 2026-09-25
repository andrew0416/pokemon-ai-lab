//! Switching: Showdown `switchIn` (the old active leaves, the newcomer takes the position)
//! and `runSwitch` (the newcomers' switch-in handlers, batched), for the battle start, chosen
//! switches during a turn, and replacements after faints.
//!
//! `runSwitch` gathers every queued `runSwitch` action and runs `fieldEvent('SwitchIn')` for
//! all of them at once: the handlers (abilities' `onStart`) are sorted by their holder's stored
//! Speed, which right after switching in is the raw Speed stat, and equal Speeds are ordered
//! uniformly at random (`speedSort`; confirmed against the oracle, `tie-start.initial.json`).
//! A handler whose holder's ability changed before it ran is skipped.
//!
//! Implemented start handlers: the four weather and four terrain setters, Intimidate, Trace
//! (copies a random traceable adjacent foe's ability and starts it at once), and abilities
//! whose `onStart` only announces them. Anything else that could fire during a switch-in
//! (other `onStart`/`onSwitchIn`/`onBeforeSwitchIn`/`onUpdate` handlers, items that act on
//! switch-in, Air Lock) makes the turn unsupported.

use crate::dex::{abilities, items, AbilityFlags, AbilityId, ItemId, SpeciesId, NO_BOOSTS};
use crate::field::{FieldEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SlotRef, Status, BOOST_COUNT};

use super::battle::{cured_on_update, Battle, BoostEffect};
use super::moves::{set_terrain, set_weather};
use super::support::{ability_supported_on_field, item_supported_on_field};
use super::TurnError;

/// Events that can fire between a switch-in and the next action: `SwitchIn` (with the
/// `onStart` fallback for abilities and items), `BeforeSwitchIn`, `BattleStart` (species),
/// `Update`, and what starting an ability can trigger (`SetAbility`, `SetWeather`,
/// `WeatherChange`, `TerrainChange`). `ModifySpe` would matter if the start order used
/// modified Speed.
const START_EVENTS: [&str; 10] = [
    "Start",
    "SwitchIn",
    "BeforeSwitchIn",
    "BattleStart",
    "Update",
    "SetAbility",
    "SetWeather",
    "WeatherChange",
    "TerrainChange",
    "ModifySpe",
];

/// The first handler in `handlers` that can fire during a switch-in. Handlers of an effect's
/// own condition (`condition.on*`) belong to a volatile that does not exist yet.
pub fn start_handler(handlers: &'static [&'static str]) -> Option<&'static str> {
    start_handler_of(handlers)
}

/// What an ability does when it starts (switch-in, Trace copy, or `setAbility` after a forme
/// change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartEffect {
    None,
    Weather(Weather),
    Terrain(Terrain),
    Intimidate,
    Trace,
}

/// Abilities with an implemented start, with the exact handler lists they were implemented
/// against (pinned by a test in `support`).
pub(crate) const START_HANDLERS: &[(AbilityId, &[&str], StartEffect)] = &[
    (
        abilities::TRACE,
        &["onStart", "onUpdate"],
        StartEffect::Trace,
    ),
    (
        abilities::DROUGHT,
        &["onStart"],
        StartEffect::Weather(Weather::Sun),
    ),
    (
        abilities::DRIZZLE,
        &["onStart"],
        StartEffect::Weather(Weather::Rain),
    ),
    (
        abilities::SAND_STREAM,
        &["onStart"],
        StartEffect::Weather(Weather::Sand),
    ),
    (
        abilities::SNOW_WARNING,
        &["onStart"],
        StartEffect::Weather(Weather::Snow),
    ),
    (
        abilities::ELECTRIC_SURGE,
        &["onStart"],
        StartEffect::Terrain(Terrain::Electric),
    ),
    (
        abilities::GRASSY_SURGE,
        &["onStart"],
        StartEffect::Terrain(Terrain::Grassy),
    ),
    (
        abilities::MISTY_SURGE,
        &["onStart"],
        StartEffect::Terrain(Terrain::Misty),
    ),
    (
        abilities::PSYCHIC_SURGE,
        &["onStart"],
        StartEffect::Terrain(Terrain::Psychic),
    ),
    (abilities::INTIMIDATE, &["onStart"], StartEffect::Intimidate),
    // `onStart` only announces the ability.
    (
        abilities::COMATOSE,
        &["onSetStatus", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::PRESSURE,
        &["onDeductPP", "onStart"],
        StartEffect::None,
    ),
    // Gluttony's `onStart` only sets a flag the berries read (`update.rs` treats it as set).
    (
        abilities::GLUTTONY,
        &["onDamage", "onStart"],
        StartEffect::None,
    ),
    // Status-curing `onUpdate`: nothing to cure on switch-in, because a holder that already
    // has the status is refused (`cured_on_update`).
    (
        abilities::WATER_VEIL,
        &["onSetStatus", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::IMMUNITY,
        &["onSetStatus", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::INSOMNIA,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::VITAL_SPIRIT,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::LIMBER,
        &["onSetStatus", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::MAGMA_ARMOR,
        &["onImmunity", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::WATER_BUBBLE,
        &[
            "onModifyAtk",
            "onModifySpA",
            "onSetStatus",
            "onSourceModifyAtk",
            "onSourceModifySpA",
            "onUpdate",
        ],
        StartEffect::None,
    ),
];

/// What `ability` does when it starts, or `None` if it has a switch-in handler that is not
/// implemented. `ModifySpe` handlers are allowed: the start order uses the stored Speed, which
/// right after switching in is the raw stat. Air Lock / Cloud Nine (`suppressWeather`) are
/// refused.
pub(crate) fn start_effect(ability: AbilityId) -> Option<StartEffect> {
    if let Some(&(_, _, effect)) = START_HANDLERS.iter().find(|(id, ..)| *id == ability) {
        return Some(effect);
    }
    let data = ability.data();
    if data.suppress_weather {
        return None;
    }
    match start_handler(data.handlers) {
        None | Some("onModifySpe") => Some(StartEffect::None),
        Some(_) => None,
    }
}

/// Whether a Pokémon with this ability can switch in (its switch-in effect, if any, is
/// implemented).
pub fn switch_in_supported(ability: AbilityId) -> bool {
    start_effect(ability).is_some()
}

/// The first switch-in handler of an item that would fire and is not implemented. An
/// `onUpdate` of an item the engine supports on the field (berries, `update.rs`) is
/// implemented and does not count.
pub fn item_start_handler(item: ItemId) -> Option<&'static str> {
    let handlers = item.data().handlers;
    match start_handler(handlers) {
        Some("onUpdate") if item_supported_on_field(item) => {
            let rest: Vec<&'static str> = handlers
                .iter()
                .copied()
                .filter(|h| *h != "onUpdate")
                .collect();
            start_handler_of(&rest)
        }
        other => other,
    }
}

fn start_handler_of(handlers: &[&'static str]) -> Option<&'static str> {
    handlers.iter().copied().find(|h| {
        let Some(event) = h.strip_prefix("on") else {
            return false;
        };
        let event = ["Ally", "Foe", "Any", "Source"]
            .iter()
            .find_map(|p| event.strip_prefix(*p).filter(|e| START_EVENTS.contains(e)))
            .unwrap_or(event);
        START_EVENTS.contains(&event)
    })
}

/// The first switch-in handler of a species that would fire, if any (none is implemented).
pub fn species_start_handler(species: SpeciesId) -> Option<&'static str> {
    start_handler(species.data().handlers)
}

/// Why `pokemon` cannot switch in, if it cannot: its ability or item must be implemented on the
/// field, nothing may fire on its switch-in that is not implemented, and its status must not be
/// one its ability would cure on the next Update (no Update event yet, see `cured_on_update`).
/// At battle start `on_field` is false: on-field support is checked before the first turn
/// (`support::check_state`) so leads with inert-at-start abilities still expand.
fn switch_in_problem<const N: usize>(
    b: &Battle<'_, N>,
    pokemon: PokemonRef,
    on_field: bool,
) -> Option<String> {
    let mon = b.mon(pokemon);
    let name = mon.species.data().name;
    if on_field && !ability_supported_on_field(mon.ability) {
        return Some(format!(
            "{name}: ability {} ({:?})",
            mon.ability.data().name,
            mon.ability.data().handlers
        ));
    }
    if on_field && !item_supported_on_field(mon.item) {
        return Some(format!(
            "{name}: item {} ({:?})",
            mon.item.data().name,
            mon.item.data().handlers
        ));
    }
    if let Some(handler) = item_start_handler(mon.item) {
        return Some(format!(
            "{name}: item {} switch-in handler {handler}",
            mon.item.data().name
        ));
    }
    if let Some(handler) = species_start_handler(mon.species) {
        return Some(format!("{name}: species switch-in handler {handler}"));
    }
    if start_effect(mon.ability).is_none() {
        return Some(format!(
            "{name}: ability {} switch-in handler {}",
            mon.ability.data().name,
            start_handler(mon.ability.data().handlers).unwrap_or("suppressWeather")
        ));
    }
    if cured_on_update(mon.ability, mon.status) {
        return Some(format!(
            "{name}: {} would cure its status on Update",
            mon.ability.data().name
        ));
    }
    super::update::berry_problem(mon)
}

/// Showdown `switchIn` without its `runSwitch`: the old occupant leaves (its ability and types
/// revert, its slot state resets); a fainted occupant still holding the position loses `fnt`
/// (`oldActive.status = ''`); the newcomer takes the position.
pub(crate) fn switch_in<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
    on_field: bool,
) -> Result<(), TurnError> {
    let incoming = PokemonRef {
        side: slot.side,
        party: party_index,
    };
    if let Some(why) = switch_in_problem(b, incoming, on_field) {
        return Err(b.unsupported(why));
    }
    if let Some(outgoing) = b.occupant(slot) {
        b.clear_volatile(outgoing);
    }
    if let Some(fainted) = b.state.slot(slot).fainted_occupant {
        let fainted = PokemonRef {
            side: slot.side,
            party: fainted,
        };
        let old = b.mon(fainted).status;
        if old == Status::Fainted {
            b.apply(Instruction::ChangeStatus {
                target: fainted,
                old,
                new: Status::None,
            });
        }
    }
    let previous = b.state.slot(slot).clone();
    b.apply(Instruction::Switch {
        slot,
        previous,
        party_index: Some(party_index),
    });
    Ok(())
}

/// Showdown `runSwitch` for the Pokémon that just switched in: their abilities' start handlers
/// in Speed order (raw stat; equal Speeds uniformly at random), each skipped if the holder's
/// ability changed before its turn came.
pub(crate) fn run_switch_in<const N: usize>(
    b: &mut Battle<'_, N>,
    newcomers: &[SlotRef],
) -> Result<(), TurnError> {
    let mut pending: Vec<(SlotRef, AbilityId, i16)> = newcomers
        .iter()
        .filter_map(|&slot| {
            let pokemon = b.alive(slot)?;
            let mon = b.mon(pokemon);
            Some((slot, mon.ability, mon.stats[4]))
        })
        .collect();
    while !pending.is_empty() {
        let best = pending.iter().map(|h| h.2).max().expect("non-empty");
        let tied: Vec<usize> = (0..pending.len())
            .filter(|&i| pending[i].2 == best)
            .collect();
        let pick = if tied.len() == 1 {
            tied[0]
        } else {
            tied[b.rng.uniform(tied.len())]
        };
        let (slot, ability, _) = pending.remove(pick);
        if b.alive(slot).is_none() || b.ability(slot) != ability {
            continue;
        }
        start_ability(b, slot, ability)?;
    }
    Ok(())
}

/// `switchIn` + its own `runSwitch`, for a switch chosen during a turn (a mid-turn `runSwitch`
/// is queued at order 101 and runs before the next chosen switch, so it is never batched).
pub(crate) fn run_switch<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
) -> Result<(), TurnError> {
    switch_in(b, slot, party_index, true)?;
    run_switch_in(b, &[slot])
}

/// `singleEvent('Start')` of `ability` for the Pokémon at `slot`.
pub(crate) fn start_ability<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    let Some(effect) = start_effect(ability) else {
        return Err(b.unsupported(format!(
            "ability {} starting ({:?})",
            ability.data().name,
            ability.data().handlers
        )));
    };
    match effect {
        StartEffect::None => {}
        StartEffect::Weather(weather) => {
            set_weather(b, slot, weather);
        }
        StartEffect::Terrain(terrain) => {
            set_terrain(b, slot, terrain);
        }
        StartEffect::Intimidate => {
            let mut drop = NO_BOOSTS;
            drop[0] = -1;
            debug_assert_eq!(drop.len(), BOOST_COUNT);
            for foe in b.alive_slots(slot.side.other()) {
                b.boost_by(
                    foe,
                    &drop,
                    Some(slot),
                    BoostEffect::Ability(abilities::INTIMIDATE),
                );
            }
        }
        StartEffect::Trace => trace(b, slot)?,
    }
    Ok(())
}

/// Trace's `onStart` → `Update`: copies the ability of a uniformly random adjacent foe whose
/// ability lacks `notrace`, then that ability starts at once (`setAbility` → `Start`). With no
/// traceable foe Trace keeps seeking on later Updates, which the engine cannot represent, so
/// that (and the No Ability / Ability Shield edge cases) is unsupported.
fn trace<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> Result<(), TurnError> {
    let pokemon = b.alive(holder).expect("the holder is active");
    let foes = b.alive_slots(holder.side.other());
    if foes.iter().any(|&f| b.ability(f) == abilities::NO_ABILITY) {
        return Err(b.unsupported("Trace next to No Ability"));
    }
    if b.mon(pokemon).item == items::ABILITY_SHIELD {
        return Err(b.unsupported("Trace holding Ability Shield"));
    }
    let targets: Vec<SlotRef> = foes
        .into_iter()
        .filter(|&f| !b.ability(f).data().flags.contains(AbilityFlags::NOTRACE))
        .collect();
    if targets.is_empty() {
        return Err(
            b.unsupported("Trace has no traceable foe and would keep seeking on later Updates")
        );
    }
    let target = targets[b.rng.uniform(targets.len())];
    let copied = b.ability(target);
    if copied.data().flags.contains(AbilityFlags::CANTSUPPRESS) {
        return Err(b.unsupported(format!(
            "Trace copying {} (cantsuppress: setAbility fails and Trace keeps seeking)",
            copied.data().name
        )));
    }
    if start_effect(copied).is_none() {
        return Err(b.unsupported(format!(
            "Trace copying {} ({:?})",
            copied.data().name,
            copied.data().handlers
        )));
    }
    let status = b.mon(pokemon).status;
    if cured_on_update(copied, status) {
        return Err(b.unsupported(format!(
            "Trace copying {} would cure {status:?} on Update",
            copied.data().name
        )));
    }
    b.apply(Instruction::SetAbility {
        target: pokemon,
        old: abilities::TRACE,
        new: copied,
    });
    start_ability(b, holder, copied)
}

/// `getActionSpeed()` of a fainted Pokémon still holding a position (the `instaswitch` action
/// that replaces it sorts by it): no boosts, no handlers (an inactive Pokémon has none, not
/// even Tailwind's), only Trick Room's negation.
pub(crate) fn fainted_action_speed<const N: usize>(b: &Battle<'_, N>, pokemon: PokemonRef) -> i32 {
    let spe = i32::from(b.mon(pokemon).stats[4]);
    if b.field_active(FieldEffect::TrickRoom) {
        -spe
    } else {
        spe
    }
}
