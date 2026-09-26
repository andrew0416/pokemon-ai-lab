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

use super::abilities::{SUB_ABILITY, SUB_ITEM, SUB_SIDE_CONDITION, SUB_SLOT_CONDITION};
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
    /// The forme abilities' `onStart` (`forme::on_start`): Ice Face, Schooling, Shields Down,
    /// Mimicry.
    Forme,
    /// Supreme Overlord: `abilityState.fallen = min(side.totalFainted, 5)` when that is not 0
    /// (`abilities::supreme_overlord_start`).
    SupremeOverlord,
    /// Commander's `onStart` (outside the SwitchIn event, which runs its `onAnySwitchIn`): its
    /// `onUpdate` (`abilities::commander_update`).
    Commander,
    /// Gorilla Tactics: `abilityState.choiceLock = ""` (the lock volatile goes).
    GorillaTactics,
    /// Slow Start: `effectState.counter = 5` (`abilities::slow_start_start`).
    SlowStart,
    /// Truant: the `truant` volatile goes, or comes for a holder that has already acted
    /// (`abilities::truant_start`).
    Truant,
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
    // Opus AA: Orichalcum Pulse's `onStart` sets sun (`field.setWeather('sunnyday')`, as Drought),
    // Hadron Engine's Electric Terrain (as Electric Surge); their stat modifiers are in
    // `abilities::attack_handlers`.
    (
        abilities::ORICHALCUM_PULSE,
        &["onModifyAtk", "onStart"],
        StartEffect::Weather(Weather::Sun),
    ),
    (
        abilities::HADRON_ENGINE,
        &["onModifySpA", "onStart"],
        StartEffect::Terrain(Terrain::Electric),
    ),
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
    // F19 Ice Face (`onSwitchInPriority: -2`): its face back in snow (`forme::ice_face_restore`).
    (
        abilities::ICE_FACE,
        &[
            "onCriticalHit",
            "onDamage",
            "onEffectiveness",
            "onStart",
            "onUpdate",
            "onWeatherChange",
        ],
        StartEffect::Forme,
    ),
    // F19 Schooling and Shields Down (`onSwitchInPriority: -1`): the forme for the HP
    // (`forme::on_start`); their `onResidual` is in `residual.rs`.
    (
        abilities::SCHOOLING,
        &["onResidual", "onStart"],
        StartEffect::Forme,
    ),
    (
        abilities::SHIELDS_DOWN,
        &["onResidual", "onSetStatus", "onStart", "onTryAddVolatile"],
        StartEffect::Forme,
    ),
    // F19 Mimicry (`onSwitchInPriority: -1`): `onStart` runs its `onTerrainChange`
    // (`forme::on_start`); the TerrainChange event in `field_events::terrain_changed`.
    (
        abilities::MIMICRY,
        &["onStart", "onTerrainChange"],
        StartEffect::Forme,
    ),
    // Forecast (`onSwitchInPriority: -2`): `onStart` runs its `onWeatherChange`
    // (`forme::forecast`, Opus AA).
    (
        abilities::FORECAST,
        &["onStart", "onWeatherChange"],
        StartEffect::Forme,
    ),
    // Flower Gift (`onSwitchInPriority: -2`): `onStart` runs its `onWeatherChange`
    // (`forme::flower_gift`); its ModifyAtk / ModifySpD handlers are in `abilities`.
    (
        abilities::FLOWER_GIFT,
        &[
            "onAllyModifyAtk",
            "onAllyModifySpD",
            "onStart",
            "onWeatherChange",
        ],
        StartEffect::Forme,
    ),
    // F19 Zero to Hero: `onSwitchIn` only announces the Hero forme; `onSwitchOut` in
    // `forme::on_switch_out`.
    (
        abilities::ZERO_TO_HERO,
        &["onSwitchIn", "onSwitchOut"],
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
    // As One (`onSwitchInPriority: 1`): Unnerve's start, flag and berry block; its
    // `onSourceAfterFaint` is `abilities::after_faint`.
    (
        abilities::AS_ONE_GLASTRIER,
        &["onEnd", "onFoeTryEatItem", "onSourceAfterFaint", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::AS_ONE_SPECTRIER,
        &["onEnd", "onFoeTryEatItem", "onSourceAfterFaint", "onStart"],
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
    // The Ruin abilities only announce themselves on start; their `onAny*` stat handlers are in
    // `abilities::ruin_handler`.
    (
        abilities::TABLETS_OF_RUIN,
        &["onAnyModifyAtk", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::SWORD_OF_RUIN,
        &["onAnyModifyDef", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::VESSEL_OF_RUIN,
        &["onAnyModifySpA", "onStart"],
        StartEffect::None,
    ),
    (
        abilities::BEADS_OF_RUIN,
        &["onAnyModifySpD", "onStart"],
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
    // Supreme Overlord: `onStart` counts the fallen, `onBasePower` in
    // `abilities::base_power_handlers`, `onEnd` only logs (`end_ability` drops the state).
    (
        abilities::SUPREME_OVERLORD,
        &["onBasePower", "onEnd", "onStart"],
        StartEffect::SupremeOverlord,
    ),
    // Commander: `onStart` (a Start outside the SwitchIn event: Mega Evolution) and
    // `onAnySwitchIn` (`run_switch_in`) both run its `onUpdate` (`abilities::commander_update`,
    // also at every Update).
    (
        abilities::COMMANDER,
        &["onAnySwitchIn", "onStart", "onUpdate"],
        StartEffect::Commander,
    ),
    // Gorilla Tactics: `onStart` / `onEnd` reset the lock; the rest in `abilities`
    // (`gorilla_*`) and `attack_handlers`.
    (
        abilities::GORILLA_TACTICS,
        &[
            "onBeforeMove",
            "onDisableMove",
            "onEnd",
            "onModifyAtk",
            "onModifyMove",
            "onStart",
        ],
        StartEffect::GorillaTactics,
    ),
    // Neutralizing Gas: no `onStart` (a `Start` does nothing); its `onSwitchIn` (priority 2) is
    // `run_switch_in`'s (`abilities::neutralizing_gas_switch_in`), its `onEnd`
    // `abilities::neutralizing_gas_end` (switching out, fainting, losing the ability, Gastro
    // Acid); the suppression itself is `abilities::ignoring_ability`.
    (
        abilities::NEUTRALIZING_GAS,
        &["onEnd", "onSwitchIn"],
        StartEffect::None,
    ),
    // Opus U. Slow Start: `onStart` sets the counter, `onEnd` only logs (`end_ability` drops the
    // counter with the ability state); `onModifyAtk` / `onModifySpe` / `onResidual` in
    // `abilities` (`slow_start_halves`, `on_residual`).
    (
        abilities::SLOW_START,
        &[
            "onEnd",
            "onModifyAtk",
            "onModifySpe",
            "onResidual",
            "onStart",
        ],
        StartEffect::SlowStart,
    ),
    // Opus U. Truant: `onStart` (`abilities::truant_start`), `onBeforeMove`
    // (`abilities::truant_before_move`, from `moves::before_move`).
    (
        abilities::TRUANT,
        &["onBeforeMove", "onStart"],
        StartEffect::Truant,
    ),
    // Opus U. Opportunist: no `onStart`; its `onAnySwitchIn` is `run_switch_in`'s
    // (`SwitchInHandler::OpportunistAny`), the rest in `abilities::opportunist_*`.
    (
        abilities::OPPORTUNIST,
        &[
            "onAnyAfterMega",
            "onAnyAfterMove",
            "onAnyAfterTerastallization",
            "onAnySwitchIn",
            "onEnd",
            "onFoeAfterBoost",
            "onResidual",
        ],
        StartEffect::None,
    ),
];

/// What `ability` does when it starts, or `None` if it has a switch-in handler that is not
/// implemented. `ModifySpe` handlers are allowed: the start order uses the stored Speed, which
/// right after switching in is the raw stat. Every handler is looked at (an `onModifySpe` does
/// not hide a later `onStart`). A `suppressWeather` ability outside the table is refused.
pub(crate) fn start_effect(ability: AbilityId) -> Option<StartEffect> {
    if let Some(&(_, _, effect)) = START_HANDLERS.iter().find(|(id, ..)| *id == ability) {
        return Some(effect);
    }
    let data = ability.data();
    if data.suppress_weather {
        return None;
    }
    let unimplemented = (0..data.handlers.len())
        .filter_map(|i| start_handler(&data.handlers[i..=i]))
        .any(|h| h != "onModifySpe");
    (!unimplemented).then_some(StartEffect::None)
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
    if on_field {
        if let Some(why) = super::forme::field_problem(mon) {
            return Some(why);
        }
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
    let paradox = super::abilities::reacts_to_suppressor_end(mon.ability);
    if suppresses || paradox {
        let clash = b.all_alive().into_iter().any(|s| {
            let other = b.ability(s);
            (suppresses && super::abilities::reacts_to_suppressor_end(other))
                || (paradox && other.data().suppress_weather)
        });
        if clash {
            return Some(format!(
                "{name}: Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the \
                 suppressor's End WeatherChange)"
            ));
        }
    }
    // A Rivalry holder needs every gender decided (`abilities::rivalry_problem`).
    if on_field && mon.ability == abilities::RIVALRY {
        let undecided = b
            .state
            .sides
            .iter()
            .flat_map(|side| side.party.iter())
            .any(|m| !m.species.is_none() && m.gender == crate::dex::Gender::Random);
        if undecided {
            return Some(format!(
                "{name}: Rivalry next to a Pokémon of undecided gender"
            ));
        }
    }
    super::update::berry_problem(mon)
}

/// Showdown `switchIn` without its `runSwitch`: a healthy old occupant runs `BeforeSwitchOut`
/// (no implemented handler), the gen 5+ `eachEvent('Update')` and `SwitchOut` (Regenerator,
/// Natural Cure: `abilities::on_switch_out`; Zero to Hero: `forme::on_switch_out`); the old
/// occupant leaves (its ability and types
/// revert, its slot state resets); a fainted occupant still holding the position loses `fnt`
/// (`oldActive.status = ''`); the newcomer takes the position.
pub(crate) fn switch_in<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
    on_field: bool,
) -> Result<(), TurnError> {
    switch_in_as(b, slot, party_index, on_field, false)
}

/// [`switch_in`] for an `instaswitch` answering a mid-turn switch request (U-turn, Eject
/// Button, Emergency Exit, ...): `runAction`'s tail already ran `BeforeSwitchOut` for every
/// flagged Pokémon when it made the request and set `skipBeforeSwitchOutEventFlag`, so the
/// outgoing Pokémon leaves without `switchIn`'s BeforeSwitchOut and its Update (with two
/// switches in one batch, the second leaves before any Update sees the first newcomer or the
/// first leaver's absence, e.g. an Unnerve that no longer blocks its berry).
pub(crate) fn instaswitch_in<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
    on_field: bool,
) -> Result<(), TurnError> {
    switch_in_as(b, slot, party_index, on_field, true)
}

/// [`switch_in`], or with `skip_before_switch_out` the switch of `dragIn` (Roar, Dragon Tail,
/// Red Card) or of an [`instaswitch_in`]: `if (!oldActive.skipBeforeSwitchOutEventFlag &&
/// !isDrag) { runEvent('BeforeSwitchOut'); eachEvent('Update'); }`, so a dragged-out Pokémon
/// leaves without that Update (an Update condition that arose after the move's last Update,
/// such as Outrage's fatigue confusion next to a Persim Berry, waits for the Update after the
/// newcomer's `runSwitch`, when the dragged Pokémon is gone). `SwitchOut` still runs.
fn switch_in_as<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
    on_field: bool,
    skip_before_switch_out: bool,
) -> Result<(), TurnError> {
    let incoming = PokemonRef {
        side: slot.side,
        party: party_index,
    };
    if let Some(why) = switch_in_problem(b, incoming, on_field) {
        return Err(b.unsupported(why));
    }
    // What the newcomer takes over (`copyVolatileFrom`): the outgoing Pokémon's slot state, and
    // whether only its substitute passes (Shed Tail).
    let mut passed: Option<(crate::state::Slot, bool)> = None;
    if let Some(outgoing) = b.occupant(slot) {
        if b.mon(outgoing).hp > 0 {
            if !skip_before_switch_out {
                super::update::update_event(b)?;
            }
            super::abilities::on_switch_out(b, slot);
            super::forme::on_switch_out(b, slot);
            // `singleEvent('End', oldActive.getAbility())` while it is still active: Neutralizing
            // Gas's `onEnd` restarts the abilities it suppressed (no other ability's `End` acts
            // on a Pokémon that leaves; Unburden's and Zen Mode's remove volatiles that
            // `copy_volatile_from` leaves out).
            if b.raw_ability(slot) == abilities::NEUTRALIZING_GAS {
                super::abilities::neutralizing_gas_end(b, Some(slot))?;
            }
            // The switch's `sourceEffect` is the move its `switchFlag` names (`resolveAction`):
            // Baton Pass (`'copyvolatile'`) or Shed Tail (`'shedtail'`) copy after the `End`
            // events, before the outgoing Pokémon's `clearVolatile`.
            let shed_tail = match b.state.slot(slot).switch_flag {
                SwitchFlag::CopyVolatile => Some(false),
                SwitchFlag::ShedTail => Some(true),
                _ => None,
            };
            if let Some(shed_tail) = shed_tail {
                passed = Some((b.state.slot(slot).clone(), shed_tail));
            }
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
    if let Some((from, shed_tail)) = passed {
        copy_volatile_from(b, slot, &from, shed_tail)?;
    }
    // `switchIn` queued the newcomer's `runSwitch` (a drag runs it at once).
    b.awaiting_run_switch = true;
    b.unstarted.push(incoming);
    Ok(())
}

/// Showdown `pokemon.copyVolatileFrom(oldActive, switchCause)` for the newcomer now in `slot`,
/// from the outgoing Pokémon's slot state `from`: Baton Pass passes the stat stages
/// (`this.boosts = pokemon.boosts`) and every volatile that is not `noCopy`
/// ([`Volatile::baton_pass`]) with its effect state (the substitute with its HP); Shed Tail
/// (`shed_tail`) only the substitute. Then `singleEvent('Copy')` for each copied volatile:
/// Power Trick and Power Shift swap the newcomer's stored Attack and Defense; Gastro Acid ends
/// on a `cantsuppress` ability. No copied volatile is linked (`trapped` / `trapper` are
/// `noCopy`), so no link moves. A volatile whose passing is not modelled is unsupported.
fn copy_volatile_from<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    from: &crate::state::Slot,
    shed_tail: bool,
) -> Result<(), TurnError> {
    use crate::volatile::{Passed, VolatileState};
    let mut copied = Vec::new();
    for (volatile, state) in from.volatiles.iter() {
        if shed_tail && volatile != Volatile::Substitute {
            continue;
        }
        match volatile.baton_pass() {
            Passed::Dropped => continue,
            Passed::Refused => {
                return Err(
                    b.unsupported(format!("Baton Pass passing the {} volatile", volatile.id()))
                );
            }
            Passed::Copied => copied.push((volatile, state)),
        }
    }
    if !shed_tail {
        for (stat, &amount) in from.boosts.iter().enumerate() {
            if amount != 0 {
                b.apply(Instruction::Boost {
                    target: slot,
                    stat: stat as u8,
                    amount,
                });
            }
        }
    }
    for &(volatile, state) in &copied {
        b.apply(Instruction::SetVolatile {
            target: slot,
            volatile,
            old: VolatileState::NONE,
            new: state,
        });
        if volatile == Volatile::Substitute {
            b.set_substitute_hp(slot, from.substitute_hp);
        }
    }
    for (volatile, _) in copied {
        match volatile {
            Volatile::PowerTrick | Volatile::PowerShift => {
                super::conditions::swap_stored_stats(b, slot, 0, 1);
            }
            Volatile::GastroAcid => {
                let locked = b
                    .raw_ability(slot)
                    .data()
                    .flags
                    .contains(AbilityFlags::CANTSUPPRESS);
                if locked {
                    b.remove_volatile(slot, Volatile::GastroAcid);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A handler of the batched `fieldEvent('SwitchIn')`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwitchInHandler {
    /// The slot conditions of the newcomer's position (Healing Wish's `onSwitchIn`; sub-order
    /// 3, before the hazards).
    SlotConditions,
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
    /// Commander's `onAnySwitchIn`: its `onUpdate` for the holder.
    CommanderAny,
    /// Opportunist's `onAnySwitchIn` (priority -3): its copied raises.
    OpportunistAny,
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
    // `runSwitch` takes every queued `runSwitch` action at once.
    b.awaiting_run_switch = false;
    b.unstarted.clear();
    // (priority, holder, sub-order, handler)
    let mut handlers: Vec<(i32, SlotRef, u32, SwitchInHandler)> = Vec::new();
    for &slot in newcomers {
        let Some(pokemon) = b.alive(slot) else {
            continue;
        };
        let mon = b.mon(pokemon);
        handlers.push((0, slot, SUB_SLOT_CONDITION, SwitchInHandler::SlotConditions));
        handlers.push((0, slot, SUB_SIDE_CONDITION, SwitchInHandler::Hazards));
        // `getCallback`: an ability with `onAnySwitchIn` has no `onStart` fallback in the
        // SwitchIn event (Commander, Opportunist; Pastel Veil's are the same handler at the same
        // priority).
        if mon.ability != abilities::COMMANDER && mon.ability != abilities::OPPORTUNIST {
            handlers.push((
                switch_in_priority(mon.ability),
                slot,
                SUB_ABILITY,
                SwitchInHandler::Ability(mon.ability),
            ));
        }
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
        // Opportunist's `onAnySwitchIn` (priority -3) for every active holder.
        if mon.ability == abilities::OPPORTUNIST {
            let priority = super::abilities::priority(
                mon.ability.data().event_orders,
                "onAnySwitchInPriority",
            );
            handlers.push((priority, slot, SUB_ABILITY, SwitchInHandler::OpportunistAny));
        }
        // Commander's `onAnySwitchIn` (priority -2) for every active holder, the newcomers
        // included.
        if mon.ability == abilities::COMMANDER {
            let priority = super::abilities::priority(
                mon.ability.data().event_orders,
                "onAnySwitchInPriority",
            );
            handlers.push((priority, slot, SUB_ABILITY, SwitchInHandler::CommanderAny));
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
            SwitchInHandler::SlotConditions => super::conditions::slot_condition_switch_in(b, slot),
            SwitchInHandler::Hazards => {
                // Each hazard is followed by `faintMessages`; the event stops once the battle
                // is over.
                super::conditions::entry_hazards(b, slot)?;
                if b.is_over() {
                    return Ok(());
                }
            }
            // `singleEvent` skips a suppressed ability (`ignoringAbility`) or one that changed,
            // and a breakable one while a move suppresses it (`eventid === 'SwitchIn' &&
            // flags.breakable && suppressingAbility(target)`: a Pokémon dragged in by a Mold
            // Breaker's Roar, whose move is active until the action's `clearActiveMove()`).
            SwitchInHandler::Ability(ability) => {
                if b.ability_unless_broken(slot) == ability {
                    if ability == abilities::NEUTRALIZING_GAS {
                        super::abilities::neutralizing_gas_switch_in(b, slot);
                    } else {
                        start_ability(b, slot, ability)?;
                    }
                }
            }
            SwitchInHandler::Item(item) => super::items::switch_in_item(b, slot, item),
            // The same `SwitchIn` event (breakable, like the holder's own).
            SwitchInHandler::PastelVeilAny => {
                if b.ability_unless_broken(slot) == abilities::PASTEL_VEIL {
                    pastel_veil_cure(b, slot);
                }
            }
            SwitchInHandler::CommanderAny => {
                if b.ability(slot) == abilities::COMMANDER {
                    super::abilities::commander_update(b, slot);
                }
            }
            SwitchInHandler::OpportunistAny => super::abilities::opportunist_use(b, slot),
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
    // The switch action's `runAction` tail runs `eachEvent('Update')` after `switchIn` queued
    // `runSwitch` and before it runs: the newcomer's berries and other Update effects act
    // before the entry hazards (a Sitrus holder at 20 HP eats first, then takes the rocks).
    super::update::update_event(b)?;
    run_switch_in(b, &[slot])
}

/// `singleEvent('Start')` of `ability` for the Pokémon at `slot`: nothing while the holder
/// ignores its ability (`ignoringAbility`: Gastro Acid, Neutralizing Gas).
pub(crate) fn start_ability<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    if b.ability(slot) != ability {
        return Ok(());
    }
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
                // `if (target.volatiles['substitute']) this.add('-immune', target);`
                if b.has_substitute(foe) {
                    continue;
                }
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
        StartEffect::Forme => super::forme::on_start(b, slot, ability)?,
        StartEffect::SupremeOverlord => super::abilities::supreme_overlord_start(b, slot),
        StartEffect::Commander => super::abilities::commander_update(b, slot),
        StartEffect::GorillaTactics => b.delete_volatile(slot, Volatile::GorillaTactics),
        StartEffect::SlowStart => super::abilities::slow_start_start(b, slot),
        StartEffect::Truant => super::abilities::truant_start(b, slot),
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
/// adjacent foe not fainted; a substitute makes a foe immune) act once per battle: Showdown sets `pokemon.swordBoost` / `.shieldBoost` / `.syrupTriggered` for good.
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
                if !b.has_substitute(foe) {
                    b.boost_by(foe, &boosts, Some(slot), BoostEffect::Ability(ability));
                }
            }
            return Ok(());
        }
    }
    b.boost_by(slot, &boosts, Some(slot), BoostEffect::Ability(ability));
    Ok(())
}

/// Showdown `eachEvent('WeatherChange', airlock | cloudnine)`: every active Pokémon's
/// `onWeatherChange` handlers, in Speed order. Ice Face's returns at once for a source with
/// `suppressWeather`; no other is implemented (Forecast, Flower Gift, Protosynthesis), so the
/// event does nothing, and a handler that would run makes it unsupported.
fn weather_change<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    for slot in b.all_alive() {
        let mon = b.slot_mon(slot).expect("alive");
        let ability = b.ability(slot);
        let ability_handlers = if ability == abilities::ICE_FACE || ability == abilities::FORECAST {
            &[][..]
        } else {
            ability.data().handlers
        };
        let handlers = [
            (ability.data().name, ability_handlers),
            (mon.item.data().name, mon.item.data().handlers),
            (mon.species.data().name, mon.species.data().handlers),
        ];
        for (name, list) in handlers {
            if list.contains(&"onWeatherChange") {
                return Err(b.unsupported(format!("{name}: onWeatherChange")));
            }
        }
    }
    // Forecast's `onWeatherChange` (each changes only its holder, so the Speed order is moot).
    for slot in b.all_alive() {
        super::forme::forecast(b, slot);
    }
    Ok(())
}

/// `singleEvent('End')` of the ability the Pokémon at `slot` loses while staying active
/// (`setAbility` during a forme change, or Gastro Acid suppressing it: `End` runs even for a
/// suppressed ability). Flash Fire's `onEnd` removes its volatile; Air Lock's and Cloud Nine's
/// end their suppression (the new ability no longer suppresses) and run `WeatherChange`;
/// Neutralizing Gas's restarts the abilities it suppressed (`abilities::neutralizing_gas_end`);
/// Unnerve's has nothing to undo; an ability without `onEnd` does nothing. Any other `onEnd` is
/// unsupported.
pub(crate) fn end_ability<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), TurnError> {
    // `setAbility` then starts the new ability with a fresh `abilityState` (Anger Shell's and
    // Berserk's pending check is dropped).
    b.delete_volatile(slot, Volatile::AngerShellUnchecked);
    // Unnerve's and As One's `onEnd` clears `effectState.unnerved`, which only exists while it
    // is active.
    if [
        abilities::UNNERVE,
        abilities::AS_ONE_GLASTRIER,
        abilities::AS_ONE_SPECTRIER,
    ]
    .contains(&ability)
    {
        return Ok(());
    }
    // Supreme Overlord's `onEnd` only logs; its `abilityState.fallen` goes with the ability.
    if ability == abilities::SUPREME_OVERLORD {
        b.delete_volatile(slot, Volatile::SupremeOverlord);
        return Ok(());
    }
    // Slow Start's `onEnd` only logs; its `abilityState.counter` goes with the ability.
    if ability == abilities::SLOW_START {
        b.delete_volatile(slot, Volatile::SlowStart);
        return Ok(());
    }
    // Opportunist's `onEnd`: `delete this.effectState.boosts`.
    if ability == abilities::OPPORTUNIST {
        b.delete_volatile(slot, Volatile::Opportunist);
        return Ok(());
    }
    // Cud Chew and Ripen (no `onEnd`): their `abilityState.berry` / `.berryWeaken` go with the
    // ability.
    if ability == abilities::CUD_CHEW {
        b.delete_volatile(slot, Volatile::CudChew);
        return Ok(());
    }
    if ability == abilities::RIPEN {
        b.delete_volatile(slot, Volatile::RipenWeaken);
        return Ok(());
    }
    // Gorilla Tactics' `onEnd`: `pokemon.abilityState.choiceLock = ""`.
    if ability == abilities::GORILLA_TACTICS {
        b.delete_volatile(slot, Volatile::GorillaTactics);
        return Ok(());
    }
    // Protean / Libero (no `onEnd`): their `abilityState.protean` / `.libero` goes with the
    // ability.
    if ability == abilities::PROTEAN || ability == abilities::LIBERO {
        b.delete_volatile(slot, Volatile::ProteanUsed);
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
    if ability == abilities::NEUTRALIZING_GAS {
        return super::abilities::neutralizing_gas_end(b, Some(slot));
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
/// that (and the No Ability edge case) is unsupported. An effective Ability Shield stops the
/// seeking: Trace stays.
fn trace<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> Result<(), TurnError> {
    let pokemon = b.alive(holder).expect("the holder is active");
    let foes = b.alive_slots(holder.side.other());
    // `foeActive.ability === 'noability'`, `target.getAbility()`: the raw abilities.
    if foes
        .iter()
        .any(|&f| b.raw_ability(f) == abilities::NO_ABILITY)
    {
        return Err(b.unsupported("Trace next to No Ability"));
    }
    // `pokemon.hasItem('Ability Shield')` (the effective item: under Magic Room Trace seeks and
    // the shield's `onSetAbility` is skipped too): `effectState.seek = false`, so Trace never
    // copies (its `onUpdate` returns while `seek` is false, and only `onStart` sets it).
    if b.item(holder) == items::ABILITY_SHIELD {
        return Ok(());
    }
    let targets: Vec<SlotRef> = foes
        .into_iter()
        .filter(|&f| {
            !b.raw_ability(f)
                .data()
                .flags
                .contains(AbilityFlags::NOTRACE)
        })
        .collect();
    if targets.is_empty() {
        return Err(
            b.unsupported("Trace has no traceable foe and would keep seeking on later Updates")
        );
    }
    let target = targets[b.rng.uniform(targets.len())];
    let copied = b.raw_ability(target);
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
/// (Suction Cups or Guard Dog, breakable), replaces the occupant and runs its `runSwitch` at once
/// (`isDrag`).
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
    if super::moves::drag_out_ability(b.ability_unless_broken(slot))
        || super::conditions::drag_out_blocked(b, slot)
    {
        return Ok(false);
    }
    switch_in_as(b, slot, bench[pick], true, true)?;
    run_switch_in(b, &[slot])?;
    Ok(true)
}

/// `runEvent('EmergencyExit', target)`: Emergency Exit / Wimp Out ask to switch out unless the
/// side has no bench, the holder is being dragged out or already flagged; every other active
/// Pokémon's `switchFlag` is cleared first (even Eject Button's: [`clear_active_switch_flags`]).
pub(crate) fn emergency_exit<const N: usize>(b: &mut Battle<'_, N>, target: SlotRef) {
    if !emergency_exit_acts(b, target) {
        return;
    }
    clear_active_switch_flags(b);
    b.set_switch_flag(target, SwitchFlag::Effect);
}

/// Emergency Exit's and Wimp Out's `for (const side of this.sides) for (const active of
/// side.active) active.switchFlag = false;`: every position of both sides, not only the living
/// Pokémon — `side.active` also holds a Pokémon at 0 HP whose faint is not processed and a
/// fainted one still holding its position (the only such Pokémon with a flag is a user its
/// own recoil knocked out after Emergency Exit flagged it: `moves::user_emergency_exit`).
fn clear_active_switch_flags<const N: usize>(b: &mut Battle<'_, N>) {
    for side in [crate::state::SideId::One, crate::state::SideId::Two] {
        for slot in Battle::<N>::slots(side) {
            b.clear_switch_flag(slot);
        }
    }
}

/// Whether `runEvent('EmergencyExit', target)` would flag the Pokémon in `target`: it has
/// Emergency Exit or Wimp Out, its side has a bench (`canSwitch`), and it is neither being
/// dragged out nor already flagged. Its HP is not checked (the handler does not).
pub(crate) fn emergency_exit_acts<const N: usize>(b: &Battle<'_, N>, target: SlotRef) -> bool {
    matches!(
        b.ability(target),
        a if a == abilities::EMERGENCY_EXIT || a == abilities::WIMP_OUT
    ) && super::residual::bench(b, target.side).next().is_some()
        && !b.force_switch.contains(&target)
        && b.state.slot(target).switch_flag == SwitchFlag::None
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
    crossed_half(b, slot, hp_before) && emergency_exit_acts(b, slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SideId, State};

    const P1A: SlotRef = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    const P1B: SlotRef = SlotRef {
        side: SideId::One,
        slot: 1,
    };
    const P2A: SlotRef = SlotRef {
        side: SideId::Two,
        slot: 0,
    };
    const P2B: SlotRef = SlotRef {
        side: SideId::Two,
        slot: 1,
    };

    /// Two full parties (100 HP each), the first two members of each in the active positions;
    /// the first member of side one has `ability`.
    fn state_with(ability: AbilityId) -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, p) in state.side_mut(side).party.iter_mut().enumerate() {
                p.species = SpeciesId(i as u16 + 1);
                p.max_hp = 100;
                p.hp = 100;
            }
            for s in 0..2 {
                state.side_mut(side).slots[s].party_index = Some(s as u8);
            }
        }
        state.side_mut(SideId::One).party[0].ability = ability;
        state
    }

    /// Opus CC unit B17: Emergency Exit clears the `switchFlag` of every Pokémon in
    /// `side.active` before flagging its holder, a Pokémon at 0 HP whose faint is not processed
    /// and a fainted one still holding its position included (Showdown `onEmergencyExit`), not
    /// only the living ones. The only such Pokémon with a flag is a user its own recoil knocked
    /// out after Emergency Exit flagged it (`moves::user_emergency_exit`, oracle
    /// `dd-emergency-exit-recoil-eject-pack`).
    #[test]
    fn emergency_exit_clears_every_active_positions_flag() {
        let mut state = state_with(abilities::EMERGENCY_EXIT);
        // A living ally flagged by its U-turn.
        state.slot_mut(P1B).switch_flag = SwitchFlag::Move;
        // A foe at 0 HP, its faint not processed yet, still flagged.
        state.side_mut(SideId::Two).party[0].hp = 0;
        state.slot_mut(P2A).switch_flag = SwitchFlag::Effect;
        // A fainted foe still holding its position (processed faint), flagged.
        state.side_mut(SideId::Two).party[1].hp = 0;
        state.slot_mut(P2B).party_index = None;
        state.slot_mut(P2B).fainted_occupant = Some(1);
        state.slot_mut(P2B).switch_flag = SwitchFlag::Effect;
        let mut chooser = super::super::branch::Chooser::new();
        let mut b = Battle::new(&mut state, &mut chooser);
        assert!(emergency_exit_acts(&b, P1A));
        emergency_exit(&mut b, P1A);
        assert_eq!(b.state.slot(P1A).switch_flag, SwitchFlag::Effect);
        for slot in [P1B, P2A, P2B] {
            assert_eq!(b.state.slot(slot).switch_flag, SwitchFlag::None, "{slot:?}");
        }
    }

    /// An Emergency Exit that does not act (its holder is already flagged) clears nothing.
    #[test]
    fn emergency_exit_that_does_not_act_keeps_the_flags() {
        let mut state = state_with(abilities::WIMP_OUT);
        state.slot_mut(P1A).switch_flag = SwitchFlag::Move;
        state.slot_mut(P2A).switch_flag = SwitchFlag::Effect;
        let mut chooser = super::super::branch::Chooser::new();
        let mut b = Battle::new(&mut state, &mut chooser);
        assert!(!emergency_exit_acts(&b, P1A));
        emergency_exit(&mut b, P1A);
        assert_eq!(b.state.slot(P1A).switch_flag, SwitchFlag::Move);
        assert_eq!(b.state.slot(P2A).switch_flag, SwitchFlag::Effect);
    }
}
