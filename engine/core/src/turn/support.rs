//! What the turn engine implements, checked before a turn runs.
//!
//! A turn is only simulated if every effect that could act in it is implemented: the moves
//! chosen, the abilities, items, statuses and volatiles on the field, and the field and side
//! effects. Anything else is a [`super::TurnError::Unsupported`] naming it. Showdown lists
//! each entry's behaviour as callback names (`handlers` in the dex); the tables here pin the
//! handler lists that are implemented, and a test fails if the dex lists change.

use crate::dex::{
    abilities, items, moves, AbilityId, ItemId, MoveCategory, MoveId, MoveTarget, Ohko,
    SelfDestruct, SelfSwitch, Type,
};
use crate::field::{FieldEffect, SideEffect, Weather, FIELD_EFFECT_COUNT, SIDE_EFFECT_COUNT};
use crate::state::{SideId, SlotRef, State};
use crate::volatile::Volatile;

use super::battle::{cured_on_update, weather_from};
use super::order::fractional_priority_tenths;

/// Moves with Showdown callbacks that are implemented, with the exact callback list.
pub(crate) const MOVES_WITH_HANDLERS: &[(MoveId, &[&str])] = &[
    (
        moves::PROTECT,
        &[
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (moves::DETECT, &["onHit", "onPrepareHit"]),
    (moves::GRASSY_GLIDE, &["onModifyPriority"]),
    (moves::LOW_KICK, &["basePowerCallback", "onTryHit"]),
    (moves::GRASS_KNOT, &["basePowerCallback", "onTryHit"]),
    (moves::FAKE_OUT, &["onDisableMove", "onTry"]),
    (moves::KNOCK_OFF, &["onAfterHit", "onBasePower"]),
    (
        moves::GRAVITY,
        &[
            "condition.durationCallback",
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onModifyAccuracy",
            "condition.onModifyMove",
        ],
    ),
    (
        moves::TRICK_ROOM,
        &[
            "condition.durationCallback",
            "condition.onFieldEnd",
            "condition.onFieldRestart",
            "condition.onFieldStart",
        ],
    ),
    (
        moves::TAILWIND,
        &[
            "condition.durationCallback",
            "condition.onModifySpe",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::REFLECT,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::LIGHT_SCREEN,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::AURORA_VEIL,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
            "onTry",
        ],
    ),
    (
        moves::ELECTRIC_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onSetStatus",
            "condition.onTryAddVolatile",
        ],
    ),
    (
        moves::GRASSY_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onResidual",
        ],
    ),
    (
        moves::MISTY_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onSetStatus",
            "condition.onTryAddVolatile",
        ],
    ),
    (
        moves::PSYCHIC_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onTryHit",
        ],
    ),
];

/// Items that raise one type's moves by 4915/4096 (`onBasePower`, priority 15) and do
/// nothing else.
pub(crate) const TYPE_BOOST_ITEMS: &[(ItemId, Type)] = &[
    (items::BLACK_BELT, Type::Fighting),
    (items::BLACK_GLASSES, Type::Dark),
    (items::CHARCOAL, Type::Fire),
    (items::DRAGON_FANG, Type::Dragon),
    (items::FAIRY_FEATHER, Type::Fairy),
    (items::HARD_STONE, Type::Rock),
    (items::MAGNET, Type::Electric),
    (items::METAL_COAT, Type::Steel),
    (items::MIRACLE_SEED, Type::Grass),
    (items::MYSTIC_WATER, Type::Water),
    (items::NEVER_MELT_ICE, Type::Ice),
    (items::ODD_INCENSE, Type::Psychic),
    (items::POISON_BARB, Type::Poison),
    (items::ROCK_INCENSE, Type::Rock),
    (items::ROSE_INCENSE, Type::Grass),
    (items::SEA_INCENSE, Type::Water),
    (items::SHARP_BEAK, Type::Flying),
    (items::SILK_SCARF, Type::Normal),
    (items::SILVER_POWDER, Type::Bug),
    (items::SOFT_SAND, Type::Ground),
    (items::SPELL_TAG, Type::Ghost),
    (items::TWISTED_SPOON, Type::Psychic),
    (items::WAVE_INCENSE, Type::Water),
];

/// Items with callbacks that are implemented while the holder is on the field.
pub(crate) const ITEMS_WITH_HANDLERS: &[(ItemId, &[&str])] = &[
    (items::LEFTOVERS, &["onResidual"]),
    (
        items::LIFE_ORB,
        &["onAfterMoveSecondarySelf", "onModifyDamage"],
    ),
    (items::FOCUS_SASH, &["onDamage"]),
];

/// Abilities with callbacks that are implemented while the holder is on the field.
/// Abilities whose only callback is `onStart` act only on switch-in (see `switching`).
pub(crate) const ABILITIES_WITH_HANDLERS: &[(AbilityId, &[&str])] = &[
    (abilities::SAND_RUSH, &["onImmunity", "onModifySpe"]),
    (abilities::CHLOROPHYLL, &["onModifySpe"]),
    (abilities::SWIFT_SWIM, &["onModifySpe"]),
    (abilities::SLUSH_RUSH, &["onModifySpe"]),
    (abilities::PRANKSTER, &["onModifyPriority"]),
    (abilities::GALE_WINGS, &["onModifyPriority"]),
    (abilities::TRIAGE, &["onModifyPriority"]),
    // Damage handlers (`Battle::damage`).
    (abilities::ROCK_HEAD, &["onDamage"]),
    (abilities::MAGIC_GUARD, &["onDamage"]),
    // `onDamage` in `Battle::damage`; `onTryHit` (OHKO immunity) in `moves`.
    (abilities::STURDY, &["onDamage", "onTryHit"]),
    // Extra PP in `moves::deduct_pressure_pp`; `onStart` only announces the ability.
    (abilities::PRESSURE, &["onDeductPP", "onStart"]),
    // Status immunities (`Battle::set_status_blocked`, `status_immune`,
    // `add_volatile_blocked`). `onUpdate` cures are unreachable: see `cured_on_update`.
    (abilities::WATER_VEIL, &["onSetStatus", "onUpdate"]),
    (abilities::IMMUNITY, &["onSetStatus", "onUpdate"]),
    (
        abilities::INSOMNIA,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
    ),
    (
        abilities::VITAL_SPIRIT,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
    ),
    (abilities::LIMBER, &["onSetStatus", "onUpdate"]),
    (abilities::MAGMA_ARMOR, &["onImmunity", "onUpdate"]),
    // `onStart` only announces the ability.
    (abilities::COMATOSE, &["onSetStatus", "onStart"]),
    (abilities::LEAF_GUARD, &["onSetStatus", "onTryAddVolatile"]),
    (
        abilities::SWEET_VEIL,
        &["onAllySetStatus", "onAllyTryAddVolatile"],
    ),
    (abilities::AROMA_VEIL, &["onAllyTryAddVolatile"]),
];

pub(crate) fn type_boost_item(item: ItemId) -> Option<Type> {
    TYPE_BOOST_ITEMS
        .iter()
        .find(|&&(i, _)| i == item)
        .map(|&(_, t)| t)
}

fn listed<T: PartialEq + Copy>(table: &[(T, &[&str])], id: T) -> bool {
    table.iter().any(|&(i, _)| i == id)
}

/// Abilities without callbacks that Showdown's core checks by name (`hasAbility`), with
/// behaviour not implemented here. Levitate (grounding) and the `onCriticalHit: false`
/// abilities are implemented.
const CORE_CHECKED_ABILITIES: &[AbilityId] = &[
    abilities::CORROSION,
    abilities::DANCER,
    abilities::EARLY_BIRD,
    abilities::MULTITYPE,
    abilities::RKS_SYSTEM,
    abilities::PERSISTENT,
];

/// Items without callbacks that Showdown's core checks by name, not implemented here.
/// Weather rocks, Light Clay and Terrain Extender (durations) are implemented; Heavy-Duty
/// Boots and Protective Pads only affect unsupported hazards and contact abilities.
const CORE_CHECKED_ITEMS: &[ItemId] = &[
    items::BLUNDER_POLICY,
    items::GRIP_CLAW,
    items::BINDING_BAND,
    items::ULTRANECROZIUM_Z,
];

/// Whether an ability is inert or implemented while its holder is on the field.
pub(crate) fn ability_supported_on_field(ability: AbilityId) -> bool {
    let data = ability.data();
    // A constant `onFractionalPriority` is implemented for Stall only (`order`).
    if CORE_CHECKED_ABILITIES.contains(&ability)
        || data.fractional_priority_tenths != fractional_priority_tenths(ability)
    {
        return false;
    }
    data.handlers.is_empty()
        || data.handlers == ["onStart"]
        || listed(ABILITIES_WITH_HANDLERS, ability)
}

/// Whether an item is inert or implemented while its holder is on the field.
pub(crate) fn item_supported_on_field(item: ItemId) -> bool {
    let data = item.data();
    if CORE_CHECKED_ITEMS.contains(&item) || data.fractional_priority_tenths != 0 {
        return false;
    }
    data.handlers.is_empty()
        || listed(ITEMS_WITH_HANDLERS, item)
        || type_boost_item(item).is_some()
        // Mega Stones only matter for Knock Off, handled by `item_can_be_taken`.
        || (!data.mega_stone.is_empty() && data.handlers == ["onTakeItem"])
}

/// Why a move cannot be simulated, if it cannot.
pub(crate) fn move_unsupported(id: MoveId) -> Option<String> {
    let m = id.data();
    let name = m.name;
    let why = |what: &str| Some(format!("move {name}: {what}"));
    if !m.handlers.is_empty() && !listed(MOVES_WITH_HANDLERS, id) {
        return why(&format!("callbacks {:?} are not implemented", m.handlers));
    }
    match m.target {
        MoveTarget::Normal
        | MoveTarget::Any
        | MoveTarget::AdjacentFoe
        | MoveTarget::AllAdjacentFoes
        | MoveTarget::AllAdjacent
        | MoveTarget::User
        | MoveTarget::All
        | MoveTarget::AllySide
        | MoveTarget::FoeSide => {}
        other => return why(&format!("target {other:?}")),
    }
    if m.multihit.is_some() {
        return why("multi-hit");
    }
    if m.ohko != Ohko::No {
        return why("OHKO");
    }
    if m.self_switch != SelfSwitch::No || m.force_switch {
        return why("switching");
    }
    if m.selfdestruct != SelfDestruct::No {
        return why("self-destruct");
    }
    if m.breaks_protect
        || m.smart_target
        || m.multiaccuracy
        || m.calls_move
        || m.sleep_usable
        || m.steals_boosts
        || m.has_crash_damage
        || m.mind_blown_recoil
        || m.struggle_recoil
        || m.chloroblast_recoil
        || m.is_z
        || m.is_max
        || m.override_offensive_pokemon_target
    {
        return why("a special mechanic");
    }
    if m.stalling_move && id != moves::PROTECT && id != moves::DETECT {
        return why("stalling move");
    }
    let flags = m.flags;
    use crate::dex::MoveFlags as F;
    for (flag, what) in [
        (F::CHARGE, "two-turn"),
        (F::RECHARGE, "recharge"),
        (F::FUTUREMOVE, "future move"),
        (F::CANTUSETWICE, "can't use twice"),
    ] {
        if flags.contains(flag) {
            return why(what);
        }
    }
    if !m.slot_condition.is_none() {
        return why("slot condition");
    }
    if !m.volatile_status.is_none() && Volatile::from_condition(m.volatile_status).is_none() {
        return why(&format!("volatile {}", m.volatile_status.id()));
    }
    if !m.side_condition.is_none() && side_effect_of(m.side_condition.id()).is_none() {
        return why(&format!("side condition {}", m.side_condition.id()));
    }
    if !m.pseudo_weather.is_none() && !["gravity", "trickroom"].contains(&m.pseudo_weather.id()) {
        return why(&format!("field effect {}", m.pseudo_weather.id()));
    }
    if let Some(s) = m.self_effect {
        if !s.volatile_status.is_none()
            || !s.side_condition.is_none()
            || !s.pseudo_weather.is_none()
        {
            return why("self effect");
        }
    }
    for s in m.secondaries {
        if !s.volatile_status.is_none() && Volatile::from_condition(s.volatile_status).is_none() {
            return why(&format!("secondary volatile {}", s.volatile_status.id()));
        }
    }
    if m.category == MoveCategory::Status && m.base_power != 0 {
        return why("status move with base power");
    }
    None
}

pub(crate) fn side_effect_of(condition: &str) -> Option<SideEffect> {
    Some(match condition {
        "reflect" => SideEffect::Reflect,
        "lightscreen" => SideEffect::LightScreen,
        "auroraveil" => SideEffect::AuroraVeil,
        "tailwind" => SideEffect::Tailwind,
        _ => return None,
    })
}

/// The implemented side effects.
const SUPPORTED_SIDE_EFFECTS: [SideEffect; 4] = [
    SideEffect::Reflect,
    SideEffect::LightScreen,
    SideEffect::AuroraVeil,
    SideEffect::Tailwind,
];

/// Checks everything on the field before a turn.
pub(crate) fn check_state<const N: usize>(state: &State<N>) -> Result<(), String> {
    for i in 0..FIELD_EFFECT_COUNT {
        let effect = state.field[i];
        if !effect.is_active() {
            continue;
        }
        let supported = match i {
            x if x == FieldEffect::Weather as usize => matches!(
                weather_from(effect.value),
                Weather::Sun | Weather::Rain | Weather::Sand | Weather::Snow
            ),
            x if x == FieldEffect::Terrain as usize => true,
            x if x == FieldEffect::Gravity as usize || x == FieldEffect::TrickRoom as usize => true,
            _ => false,
        };
        if !supported {
            return Err(format!("field effect #{i} (value {})", effect.value));
        }
        if effect.turns == crate::field::Effect::PERMANENT {
            return Err(format!("field effect #{i} without a duration"));
        }
    }
    for side in [SideId::One, SideId::Two] {
        let s = state.side(side);
        for i in 0..SIDE_EFFECT_COUNT {
            if s.effects[i].is_active() && !SUPPORTED_SIDE_EFFECTS.iter().any(|&e| e as usize == i)
            {
                return Err(format!("side effect #{i}"));
            }
        }
        for slot in 0..N as u8 {
            let r = SlotRef { side, slot };
            let Some(mon) = state.active(r) else {
                continue;
            };
            let name = mon.species.data().name;
            if !ability_supported_on_field(mon.ability) {
                return Err(format!(
                    "{name}: ability {} ({:?})",
                    mon.ability.data().name,
                    mon.ability.data().handlers
                ));
            }
            if mon.ability == abilities::TRACE {
                return Err(format!("{name}: Trace still seeking a target"));
            }
            if cured_on_update(mon.ability, mon.status) {
                return Err(format!(
                    "{name}: {} would cure its status on Update (not implemented)",
                    mon.ability.data().name
                ));
            }
            if !item_supported_on_field(mon.item) {
                return Err(format!(
                    "{name}: item {} ({:?})",
                    mon.item.data().name,
                    mon.item.data().handlers
                ));
            }
            if !mon.species.data().handlers.is_empty() {
                return Err(format!("{name}: species callbacks"));
            }
            let slot_state = state.slot(r);
            if slot_state.substitute_hp != 0 || slot_state.dynamax.is_active() {
                return Err(format!("{name}: substitute or Dynamax"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_tables_match_the_dex() {
        for &(id, expected) in MOVES_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, expected) in ITEMS_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, expected) in ABILITIES_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, _) in TYPE_BOOST_ITEMS {
            let data = id.data();
            assert_eq!(data.handlers, ["onBasePower"], "{id:?}");
            assert!(
                data.event_orders.contains(&("onBasePowerPriority", 15)),
                "{id:?}"
            );
        }
        // Stall's only behaviour is its constant fractional priority.
        assert!(abilities::STALL.data().handlers.is_empty());
        assert_eq!(abilities::STALL.data().fractional_priority_tenths, -1);
        assert!(ability_supported_on_field(abilities::STALL));
        // Detect shares Protect's volatile.
        assert_eq!(
            moves::DETECT.data().volatile_status,
            moves::PROTECT.data().volatile_status
        );
    }

    #[test]
    fn common_moves_are_classified() {
        for id in [
            moves::HYPNOSIS,
            moves::ROCK_SLIDE,
            moves::HYPER_VOICE,
            moves::WOOD_HAMMER,
            moves::HIGH_HORSEPOWER,
            moves::IRON_HEAD,
            moves::FOCUS_BLAST,
            moves::PROTECT,
            moves::FAKE_OUT,
            moves::GRAVITY,
        ] {
            assert_eq!(move_unsupported(id), None, "{id:?}");
        }
        assert!(move_unsupported(moves::U_TURN).is_some());
        assert!(move_unsupported(moves::FOLLOW_ME).is_some());
        assert!(move_unsupported(moves::BULLET_SEED).is_some());
    }
}
