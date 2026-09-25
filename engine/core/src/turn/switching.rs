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
//! (copies a random traceable adjacent foe's ability and starts it at once), Air Lock and Cloud
//! Nine (their `WeatherChange` event has no implemented handler), and abilities whose `onStart`
//! only announces them; of items, the Seeds' `onStart` (`onSwitchInPriority: -1`, after every
//! ability). Anything else that could fire during a switch-in (other
//! `onStart`/`onSwitchIn`/`onBeforeSwitchIn`/`onUpdate` handlers, items that act on switch-in)
//! makes the turn unsupported.

use crate::dex::{abilities, items, AbilityFlags, AbilityId, ItemId, SpeciesId, NO_BOOSTS};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SlotRef, Status, SwitchFlag, BOOST_COUNT};
use crate::volatile::Volatile;

use super::abilities::{SUB_ABILITY, SUB_ITEM, SUB_SIDE_CONDITION};
use super::battle::{Battle, BoostEffect};
use super::moves::{set_terrain, set_weather};
use super::order::boosted_stat;
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

/// What an ability does when it starts (switch-in, Trace copy, or `setAbility` after a forme
/// change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartEffect {
    None,
    Weather(Weather),
    Terrain(Terrain),
    Intimidate,
    Trace,
    /// Air Lock, Cloud Nine: `eachEvent('WeatherChange')` (the weather they suppress is read
    /// through `Battle::effective_weather`).
    WeatherChange,
    /// Download: SpA +1 if the foes' Defense total is at least their Special Defense total,
    /// else Atk +1.
    Download,
    /// Intrepid Sword (Atk +1), Dauntless Shield (Def +1), Supersweet Syrup (adjacent foes'
    /// evasion -1): once per battle.
    OncePerBattle,
    /// Costar: the holder's stat stages become its ally's.
    Costar,
    /// Hospitality: adjacent allies heal a quarter of their max HP.
    Hospitality,
    /// Screen Cleaner: Reflect, Light Screen and Aurora Veil end on both sides.
    ScreenCleaner,
    /// Curious Medicine: adjacent allies' stat stages are cleared.
    CuriousMedicine,
    /// Pastel Veil: the holder's and its allies' poison is cured (also whenever anyone switches
    /// in, `onAnySwitchIn`).
    PastelVeil,
    /// Protosynthesis / Quark Drive: `singleEvent('WeatherChange' / 'TerrainChange')` on the
    /// holder (`abilities::paradox_change`).
    Paradox,
    /// Wind Rider: Atk +1 if Tailwind is up on the holder's side.
    WindRider,
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
    // `onSwitchIn` logs and calls `onStart`; `onStart` and `onEnd` run
    // `eachEvent('WeatherChange')`.
    (
        abilities::AIR_LOCK,
        &["onEnd", "onStart", "onSwitchIn"],
        StartEffect::WeatherChange,
    ),
    (
        abilities::CLOUD_NINE,
        &["onEnd", "onStart", "onSwitchIn"],
        StartEffect::WeatherChange,
    ),
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
    (
        abilities::MOLD_BREAKER,
        &["onModifyMove", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::TERAVOLT,
        &["onModifyMove", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::TURBOBLAZE,
        &["onModifyMove", "onStart"],
        StartEffect::None,
    ),
    // Status-curing `onUpdate`: the cure runs at the Update after the switch-in
    // (`abilities::on_update`), not at the start.
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
        abilities::THERMAL_EXCHANGE,
        &["onDamagingHit", "onSetStatus", "onUpdate"],
        StartEffect::None,
    ),
    // F19 Disguise: its `onUpdate` only acts on a Mimikyu whose ability absorbed a move's damage
    // (`forme::on_update`), never right after a switch-in.
    (
        abilities::DISGUISE,
        &["onCriticalHit", "onDamage", "onEffectiveness", "onUpdate"],
        StartEffect::None,
    ),
    // O68 switch-in abilities (`start_ability`).
    (abilities::DOWNLOAD, &["onStart"], StartEffect::Download),
    (
        abilities::INTREPID_SWORD,
        &["onStart"],
        StartEffect::OncePerBattle,
    ),
    (
        abilities::DAUNTLESS_SHIELD,
        &["onStart"],
        StartEffect::OncePerBattle,
    ),
    (
        abilities::SUPERSWEET_SYRUP,
        &["onStart"],
        StartEffect::OncePerBattle,
    ),
    // Messages only (Forewarn's `this.sample` picks which move it announces).
    (abilities::FRISK, &["onStart"], StartEffect::None),
    (abilities::FOREWARN, &["onStart"], StartEffect::None),
    (abilities::ANTICIPATION, &["onStart"], StartEffect::None),
    // `if (pokemon.baseSpecies.name === 'Ogerpon-…-Tera' && pokemon.terastallized ...)`:
    // Terastallization is not modelled (the ruleset refuses it), so they never act.
    (
        abilities::EMBODY_ASPECT_CORNERSTONE,
        &["onStart"],
        StartEffect::None,
    ),
    (
        abilities::EMBODY_ASPECT_HEARTHFLAME,
        &["onStart"],
        StartEffect::None,
    ),
    (
        abilities::EMBODY_ASPECT_TEAL,
        &["onStart"],
        StartEffect::None,
    ),
    (
        abilities::EMBODY_ASPECT_WELLSPRING,
        &["onStart"],
        StartEffect::None,
    ),
    // `onSwitchInPriority: -2` (`switch_in_priority`).
    (abilities::COSTAR, &["onStart"], StartEffect::Costar),
    (
        abilities::HOSPITALITY,
        &["onStart"],
        StartEffect::Hospitality,
    ),
    (
        abilities::SCREEN_CLEANER,
        &["onStart"],
        StartEffect::ScreenCleaner,
    ),
    (
        abilities::CURIOUS_MEDICINE,
        &["onStart"],
        StartEffect::CuriousMedicine,
    ),
    // Unnerve (`onSwitchInPriority: 1`): `onStart` sets `effectState.unnerved`, which its
    // `onFoeTryEatItem` reads (`abilities::try_eat_item`: an active Unnerve holder has always
    // started); `onEnd` clears it.
    (
        abilities::UNNERVE,
        &["onEnd", "onFoeTryEatItem", "onStart"],
        StartEffect::None,
    ),
    // Klutz (`onSwitchInPriority: 1`): `onStart` runs the item's End, which only logs; the
    // suppression is `items::ignoring_item` (F17).
    (abilities::KLUTZ, &["onStart"], StartEffect::None),
    // Pastel Veil: its `onAnySwitchIn` replaces the `onStart` fallback in the SwitchIn event and
    // runs for every active holder whenever anyone switches in (`run_switch_in`).
    (
        abilities::PASTEL_VEIL,
        &[
            "onAllySetStatus",
            "onAnySwitchIn",
            "onSetStatus",
            "onStart",
            "onUpdate",
        ],
        StartEffect::PastelVeil,
    ),
    // Own Tempo's confusion cure and Oblivious's (nothing to remove) are Update handlers too.
    (
        abilities::OWN_TEMPO,
        &["onHit", "onTryAddVolatile", "onTryBoost", "onUpdate"],
        StartEffect::None,
    ),
    (
        abilities::OBLIVIOUS,
        &["onImmunity", "onTryBoost", "onTryHit", "onUpdate"],
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
    // The auras and Aura Break only announce themselves on start.
    (
        abilities::FAIRY_AURA,
        &["onAnyBasePower", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::DARK_AURA,
        &["onAnyBasePower", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::AURA_BREAK,
        &["onAnyTryPrimaryHit", "onStart"],
        StartEffect::None,
    ),
    // `onSwitchInPriority: -2`; the condition's handlers act later (`abilities`).
    (
        abilities::PROTOSYNTHESIS,
        &[
            "condition.onEnd",
            "condition.onModifyAtk",
            "condition.onModifyDef",
            "condition.onModifySpA",
            "condition.onModifySpD",
            "condition.onModifySpe",
            "condition.onStart",
            "onEnd",
            "onStart",
            "onWeatherChange",
        ],
        StartEffect::Paradox,
    ),
    (
        abilities::QUARK_DRIVE,
        &[
            "condition.onEnd",
            "condition.onModifyAtk",
            "condition.onModifyDef",
            "condition.onModifySpA",
            "condition.onModifySpD",
            "condition.onModifySpe",
            "condition.onStart",
            "onEnd",
            "onStart",
            "onTerrainChange",
        ],
        StartEffect::Paradox,
    ),
    (
        abilities::WIND_RIDER,
        &["onSideConditionStart", "onStart", "onTryHit"],
        StartEffect::WindRider,
    ),
];

/// What `ability` does when it starts, or `None` if it has a switch-in handler that is not
/// implemented. `ModifySpe` handlers are allowed: the start order uses the stored Speed, which
/// right after switching in is the raw stat. A `suppressWeather` ability outside the table is
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

/// The first switch-in handler of an item that would fire and is not implemented, if any.
/// `onModifySpe` does not matter (the start order uses the raw Speed stat); the handlers
/// `items::start_handler_implemented` names (inert `onStart`s, the Seeds) and the `onUpdate`
/// of an item the engine supports on the field (berries, `update.rs`) are implemented.
pub fn item_start_handler(item: ItemId) -> Option<&'static str> {
    let handlers = item.data().handlers;
    (0..handlers.len())
        .filter_map(|i| start_handler(&handlers[i..=i]))
        .find(|&h| {
            h != "onModifySpe"
                && !super::items::start_handler_implemented(item, h)
                && !(h == "onUpdate" && item_supported_on_field(item))
        })
}

/// The first switch-in handler of a species that would fire, if any (none is implemented).
pub fn species_start_handler(species: SpeciesId) -> Option<&'static str> {
    start_handler(species.data().handlers)
}

/// Why `pokemon` cannot switch in, if it cannot: its ability or item must be implemented on the
/// field, and nothing may fire on its switch-in that is not implemented. At battle start
/// `on_field` is false: on-field support is checked before the first turn
/// (`support::check_state`) so leads with inert-at-start abilities still expand.
fn switch_in_problem<const N: usize>(
    b: &Battle<'_, N>,
    pokemon: PokemonRef,
    on_field: bool,
) -> Option<String> {
    let mon = b.mon(pokemon);
    let name = mon.species.data().name;
    // Trace is replaced by the ability it copies as it starts (`trace` checks that one; one that
    // keeps seeking is refused there).
    if on_field && !ability_supported_on_field(mon.ability) && mon.ability != abilities::TRACE {
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
    // The newcomer next to the Pokémon it would be refused with (`abilities`).
    let suppresses = mon.ability.data().suppress_weather;
    let paradox = mon.ability == abilities::PROTOSYNTHESIS;
    if suppresses || paradox {
        let clash = b.all_alive().into_iter().any(|s| {
            let other = b.ability(s);
            (suppresses && other == abilities::PROTOSYNTHESIS)
                || (paradox && other.data().suppress_weather)
        });
        if clash {
            return Some(format!(
                "{name}: Protosynthesis next to Air Lock / Cloud Nine (the suppressor's End \
                 WeatherChange)"
            ));
        }
    }
    super::update::berry_problem(mon)
}

/// Showdown `switchIn` without its `runSwitch`: a healthy old occupant runs `BeforeSwitchOut`
/// (no implemented handler), the gen 5+ `eachEvent('Update')` and `SwitchOut` (Regenerator,
/// Natural Cure: `abilities::on_switch_out`); the old occupant leaves (its ability and types
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
        if b.mon(outgoing).hp > 0 {
            super::update::update_event(b)?;
            super::abilities::on_switch_out(b, slot);
        }
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
        previous: Box::new(previous),
        party_index: Some(party_index),
    });
    Ok(())
}

/// A handler of the batched `fieldEvent('SwitchIn')`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwitchInHandler {
    /// The entry hazards of the newcomer's side (`conditions::entry_hazards`; side conditions,
    /// sub-order 4, their holder the newcomer).
    Hazards,
    /// The newcomer's ability `onStart` with its `onSwitchInPriority` (sub-order 7).
    Ability(AbilityId),
    /// The newcomer's item `onStart` with its `onSwitchInPriority`, or any active Pokémon's item
    /// `onAnySwitchIn` with its `onAnySwitchInPriority` (`items::switch_in_item`; sub-order 8).
    Item(ItemId),
    /// `onAnySwitchIn` of a Pastel Veil holder already on the field: its `onStart` again.
    PastelVeilAny,
}

/// Showdown `runSwitch` for the Pokémon that just switched in: one `fieldEvent('SwitchIn')`
/// over the entry hazards of each newcomer's side, their abilities' start handlers
/// (`onSwitchInPriority`: Unnerve 1, Costar and Hospitality -2, the rest 0), their items'
/// (`onSwitchInPriority`: the Seeds and Room Service, -1) and every active Pokémon's
/// `onAnySwitchIn` (Pastel Veil 0, White Herb -2, Mirror Herb -3). Showdown sorts the handlers
/// by priority, then their holder's Speed (`speedSort(getAllActive())` once for the whole event,
/// so equal Speeds are drawn uniformly at random once and keep that order in every priority
/// group), then sub-order (side condition 4, ability 7, item 8); the engine draws the same
/// order once over the holders with a handler. Speeds are the raw stats (Showdown's
/// `pokemon.speed`, the stored stat of a Pokémon that has not had a turn; a Pokémon already on
/// the field carries its last action Speed there, which the engine does not model). A handler
/// is skipped if its holder fainted, an ability handler also if the holder's ability changed
/// before its turn came; the event stops once the hazards end the battle.
pub(crate) fn run_switch_in<const N: usize>(
    b: &mut Battle<'_, N>,
    newcomers: &[SlotRef],
) -> Result<(), TurnError> {
    // (priority, holder, sub-order, handler)
    let mut handlers: Vec<(i32, SlotRef, u32, SwitchInHandler)> = Vec::new();
    for &slot in newcomers {
        let Some(pokemon) = b.alive(slot) else {
            continue;
        };
        let mon = b.mon(pokemon);
        handlers.push((0, slot, SUB_SIDE_CONDITION, SwitchInHandler::Hazards));
        handlers.push((
            switch_in_priority(mon.ability),
            slot,
            SUB_ABILITY,
            SwitchInHandler::Ability(mon.ability),
        ));
        // The effective item: `singleEvent('SwitchIn')` skips a suppressed item's handler.
        let item = b.item(slot);
        if let Some(priority) = super::items::switch_in_priority(item) {
            handlers.push((priority, slot, SUB_ITEM, SwitchInHandler::Item(item)));
        }
    }
    for slot in b.all_alive() {
        let mon = b.slot_mon(slot).expect("alive");
        let item = b.item(slot);
        if let Some(priority) = super::items::any_switch_in_priority(item) {
            handlers.push((priority, slot, SUB_ITEM, SwitchInHandler::Item(item)));
        }
        if !newcomers.contains(&slot) && mon.ability == abilities::PASTEL_VEIL {
            handlers.push((0, slot, SUB_ABILITY, SwitchInHandler::PastelVeilAny));
        }
    }
    // `speedOrder`: the holders by raw Speed, equal Speeds uniformly at random.
    let mut remaining: Vec<SlotRef> = Vec::new();
    for h in &handlers {
        if !remaining.contains(&h.1) {
            remaining.push(h.1);
        }
    }
    let speed = |b: &Battle<'_, N>, slot: SlotRef| b.slot_mon(slot).expect("alive").stats[4];
    let mut ranked: Vec<SlotRef> = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let best = remaining
            .iter()
            .map(|&s| speed(b, s))
            .max()
            .expect("non-empty");
        let tied: Vec<usize> = (0..remaining.len())
            .filter(|&i| speed(b, remaining[i]) == best)
            .collect();
        let pick = if tied.len() == 1 {
            tied[0]
        } else {
            tied[b.rng.uniform(tied.len())]
        };
        ranked.push(remaining.remove(pick));
    }
    let rank = |slot: SlotRef| ranked.iter().position(|&s| s == slot).expect("ranked");
    handlers.sort_by_key(|&(priority, slot, sub_order, _)| {
        (std::cmp::Reverse(priority), rank(slot), sub_order)
    });
    for (_, slot, _, handler) in handlers {
        if b.alive(slot).is_none() {
            continue;
        }
        match handler {
            SwitchInHandler::Hazards => {
                // Each hazard is followed by `faintMessages`; the event stops once the battle
                // is over.
                super::conditions::entry_hazards(b, slot)?;
                if b.is_over() {
                    return Ok(());
                }
            }
            SwitchInHandler::Ability(ability) => {
                if b.ability(slot) == ability {
                    start_ability(b, slot, ability)?;
                }
            }
            SwitchInHandler::Item(item) => super::items::switch_in_item(b, slot, item),
            SwitchInHandler::PastelVeilAny => {
                if b.ability(slot) == abilities::PASTEL_VEIL {
                    pastel_veil_cure(b, slot);
                }
            }
        }
    }
    Ok(())
}

/// The ability's `onSwitchInPriority` (0 when unset).
fn switch_in_priority(ability: AbilityId) -> i32 {
    super::abilities::priority(ability.data().event_orders, "onSwitchInPriority")
}

/// Pastel Veil's `onStart`: every Pokémon on the holder's side not fainted (`alliesAndSelf()`)
/// that is poisoned or badly poisoned is cured.
fn pastel_veil_cure<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) {
    for ally in b.alive_slots(holder.side) {
        let pokemon = b.alive(ally).expect("alive");
        if matches!(b.mon(pokemon).status, Status::Poison | Status::Toxic) {
            b.cure_status(pokemon);
        }
    }
}

/// Showdown `setBoost` for every stage (no boost events).
fn set_boosts<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, boosts: [i8; BOOST_COUNT]) {
    for (stat, &new) in boosts.iter().enumerate() {
        let old = b.state.slot(slot).boosts[stat];
        if old != new {
            b.apply(Instruction::Boost {
                target: slot,
                stat: stat as u8,
                amount: new - old,
            });
        }
    }
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
        StartEffect::WeatherChange => weather_change(b)?,
        StartEffect::Download => download(b, slot),
        StartEffect::OncePerBattle => once_per_battle(b, slot, ability)?,
        // Costar: `const ally = pokemon.allies()[0]` (not fainted); every stage copied (the
        // critical-hit volatiles it also copies do not exist in the engine).
        StartEffect::Costar => {
            if let Some(ally) = b.alive_slots(slot.side).into_iter().find(|&s| s != slot) {
                let boosts = b.state.slot(ally).boosts;
                set_boosts(b, slot, boosts);
            }
        }
        // Hospitality: `this.heal(ally.baseMaxhp / 4, ally, pokemon)` for each adjacent ally.
        StartEffect::Hospitality => {
            for ally in b.alive_slots(slot.side) {
                if ally != slot {
                    let max_hp = f64::from(b.slot_mon(ally).expect("alive").max_hp);
                    b.heal(ally, max_hp / 4.0);
                }
            }
        }
        // Screen Cleaner: Reflect, Light Screen, Aurora Veil end on the holder's side and the
        // foe's (`removeSideCondition`; their `onSideEnd` only logs).
        StartEffect::ScreenCleaner => {
            for effect in [
                SideEffect::Reflect,
                SideEffect::LightScreen,
                SideEffect::AuroraVeil,
            ] {
                for side in [slot.side, slot.side.other()] {
                    b.set_side_effect(side, effect, Effect::NONE);
                }
            }
        }
        // Curious Medicine: `ally.clearBoosts()` for each adjacent ally.
        StartEffect::CuriousMedicine => {
            for ally in b.alive_slots(slot.side) {
                if ally != slot {
                    set_boosts(b, ally, [0; BOOST_COUNT]);
                }
            }
        }
        StartEffect::PastelVeil => pastel_veil_cure(b, slot),
        StartEffect::Paradox => super::abilities::paradox_change(b, slot),
        StartEffect::WindRider => {
            if b.side_effect_active(slot.side, SideEffect::Tailwind) {
                super::abilities::wind_rider_boost(b, slot);
            }
        }
    }
    Ok(())
}

/// Download's `onStart`: the Defense and Special Defense of the foes not fainted (`foes()`),
/// with their stages but no modifiers (`getStat(stat, false, true)`; under Wonder Room the
/// stored stat is read with the other defense's stage), summed: SpA +1 if the Defense total
/// is positive and at least the Special Defense total, else Atk +1 if that total is positive.
fn download<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let wonder_room = b.field_active(FieldEffect::WonderRoom);
    let (mut def, mut spd) = (0, 0);
    for foe in b.alive_slots(slot.side.other()) {
        let mon = b.slot_mon(foe).expect("alive");
        let boosts = b.state.slot(foe).boosts;
        let (def_boost, spd_boost) = if wonder_room {
            (boosts[3], boosts[1])
        } else {
            (boosts[1], boosts[3])
        };
        def += boosted_stat(i32::from(mon.stats[1]), def_boost);
        spd += boosted_stat(i32::from(mon.stats[3]), spd_boost);
    }
    let stat = if def > 0 && def >= spd {
        2
    } else if spd > 0 {
        0
    } else {
        return;
    };
    let mut up = NO_BOOSTS;
    up[stat] = 1;
    b.boost_by(
        slot,
        &up,
        Some(slot),
        BoostEffect::Ability(abilities::DOWNLOAD),
    );
}

/// Intrepid Sword (`this.boost({atk: 1}, pokemon)`), Dauntless Shield (`{def: 1}`) and
/// Supersweet Syrup (`this.boost({evasion: -1}, target, pokemon, null, true)` for every
/// adjacent foe not fainted; a substitute, which is refused, makes a foe immune) act once per
/// battle: Showdown sets `pokemon.swordBoost` / `.shieldBoost` / `.syrupTriggered` for good.
/// The state does not record those flags, so they act at the battle start (when no Pokémon has
/// been on the field yet) and a later start is refused.
fn once_per_battle<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    if !b.battle_start {
        return Err(b.unsupported(format!(
            "{} after the battle start (its once-per-battle flag is not in the state)",
            ability.data().name
        )));
    }
    let mut boosts = NO_BOOSTS;
    match ability {
        a if a == abilities::INTREPID_SWORD => boosts[0] = 1,
        a if a == abilities::DAUNTLESS_SHIELD => boosts[1] = 1,
        _ => {
            boosts[6] = -1;
            for foe in b.alive_slots(slot.side.other()) {
                b.boost_by(foe, &boosts, Some(slot), BoostEffect::Ability(ability));
            }
            return Ok(());
        }
    }
    b.boost_by(slot, &boosts, Some(slot), BoostEffect::Ability(ability));
    Ok(())
}

/// Showdown `eachEvent('WeatherChange')`: every active Pokémon's `onWeatherChange` handlers,
/// in Speed order. None is implemented (Forecast, Flower Gift, Ice Face, Protosynthesis), so
/// the event does nothing, and a handler that would run makes it unsupported.
fn weather_change<const N: usize>(b: &Battle<'_, N>) -> Result<(), TurnError> {
    for slot in b.all_alive() {
        let mon = b.slot_mon(slot).expect("alive");
        let handlers = [
            (mon.ability.data().name, mon.ability.data().handlers),
            (mon.item.data().name, mon.item.data().handlers),
            (mon.species.data().name, mon.species.data().handlers),
        ];
        for (name, list) in handlers {
            if list.contains(&"onWeatherChange") {
                return Err(b.unsupported(format!("{name}: onWeatherChange")));
            }
        }
    }
    Ok(())
}

/// `singleEvent('End')` of the ability the Pokémon at `slot` loses while staying active
/// (`setAbility` during a forme change). Flash Fire's `onEnd` removes its volatile; Air Lock's
/// and Cloud Nine's end their suppression (the new ability no longer suppresses) and run
/// `WeatherChange`; Unnerve's has nothing to undo; an ability without `onEnd` does nothing.
/// Any other `onEnd` is unsupported.
pub(crate) fn end_ability<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    // `setAbility` then starts the new ability with a fresh `abilityState` (Anger Shell's and
    // Berserk's pending check is dropped).
    b.delete_volatile(slot, Volatile::AngerShellUnchecked);
    // Unnerve's `onEnd` clears `effectState.unnerved`, which only exists while it is active.
    if ability == abilities::UNNERVE {
        return Ok(());
    }
    // Unburden: `pokemon.removeVolatile('unburden')`.
    if ability == abilities::UNBURDEN {
        b.remove_volatile(slot, Volatile::Unburden);
        return Ok(());
    }
    if ability == abilities::FLASH_FIRE {
        b.remove_volatile(slot, Volatile::FlashFire);
        return Ok(());
    }
    // Protosynthesis / Quark Drive: `delete pokemon.volatiles[...]` (no condition `onEnd`).
    if ability == abilities::PROTOSYNTHESIS || ability == abilities::QUARK_DRIVE {
        b.delete_volatile(slot, Volatile::Protosynthesis);
        b.delete_volatile(slot, Volatile::QuarkDrive);
        return Ok(());
    }
    if ability == abilities::AIR_LOCK || ability == abilities::CLOUD_NINE {
        return weather_change(b);
    }
    if ability.data().handlers.contains(&"onEnd") {
        return Err(b.unsupported(format!(
            "ability {} ending ({:?})",
            ability.data().name,
            ability.data().handlers
        )));
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
    // After a switch during the battle the copied ability is on the field for the rest of the
    // turn, so it must be supported there too (at the battle start `support::check_state` checks
    // it before the first turn).
    if start_effect(copied).is_none() || (!b.battle_start && !ability_supported_on_field(copied)) {
        return Err(b.unsupported(format!(
            "Trace copying {} ({:?})",
            copied.data().name,
            copied.data().handlers
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

/// Showdown `dragIn(side, pos)` for a Pokémon with `forceSwitchFlag`: a uniformly random
/// bench member (`getRandomSwitchable`; nothing without one), unless `DragOut` blocks it
/// (Suction Cups, breakable), replaces the occupant and runs its `runSwitch` at once (`isDrag`).
/// Returns whether a switch happened.
pub(crate) fn drag_in<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<bool, TurnError> {
    let bench: Vec<u8> = super::residual::bench(b, slot.side).collect();
    if bench.is_empty() || b.alive(slot).is_none() {
        return Ok(false);
    }
    let pick = if bench.len() == 1 {
        0
    } else {
        b.rng.uniform(bench.len())
    };
    if b.ability_unless_broken(slot) == abilities::SUCTION_CUPS {
        return Ok(false);
    }
    switch_in(b, slot, bench[pick], true)?;
    run_switch_in(b, &[slot])?;
    Ok(true)
}

/// `runEvent('EmergencyExit', target)`: Emergency Exit / Wimp Out ask to switch out unless the
/// side has no bench, the holder is being dragged out or already flagged; every other active
/// Pokémon's `switchFlag` is cleared first (even Eject Button's).
pub(crate) fn emergency_exit<const N: usize>(b: &mut Battle<'_, N>, target: SlotRef) {
    if !matches!(
        b.ability(target),
        a if a == abilities::EMERGENCY_EXIT || a == abilities::WIMP_OUT
    ) {
        return;
    }
    if super::residual::bench(b, target.side).next().is_none()
        || b.force_switch.contains(&target)
        || b.state.slot(target).switch_flag != SwitchFlag::None
    {
        return;
    }
    for slot in b.all_alive() {
        b.clear_switch_flag(slot);
    }
    b.set_switch_flag(target, SwitchFlag::Effect);
}

/// Whether `hp_before` → the current HP crossed half (`hp <= maxhp / 2 && before > maxhp /
/// 2`) for a standing Pokémon: the Emergency Exit condition at `runAction`'s Update sites.
pub(crate) fn crossed_half<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
    hp_before: i16,
) -> bool {
    let Some(pokemon) = b.alive(slot) else {
        return false;
    };
    let mon = b.mon(pokemon);
    let (hp, max_hp, before) = (
        i32::from(mon.hp),
        i32::from(mon.max_hp),
        i32::from(hp_before),
    );
    2 * hp <= max_hp && 2 * before > max_hp
}

/// Emergency Exit for a Pokémon whose HP crossed half since `hp_before` (a `runSwitch` newcomer
/// hit by hazards, a Pokémon hurt by the residual phase).
pub(crate) fn emergency_exit_check<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    hp_before: i16,
) {
    if crossed_half(b, slot, hp_before) {
        emergency_exit(b, slot);
    }
}

/// Whether [`emergency_exit_check`] would flag the Pokémon (for places that cannot suspend).
pub(crate) fn emergency_exit_would_trigger<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
    hp_before: i16,
) -> bool {
    let ability = b.ability(slot);
    (ability == abilities::EMERGENCY_EXIT || ability == abilities::WIMP_OUT)
        && crossed_half(b, slot, hp_before)
        && super::residual::bench(b, slot.side).next().is_some()
        && b.state.slot(slot).switch_flag == SwitchFlag::None
}
