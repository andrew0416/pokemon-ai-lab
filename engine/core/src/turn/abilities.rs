//! Ability handlers on the damage path, and the handler ordering of Showdown's modifier
//! events.
//!
//! Showdown runs `BasePower`, `ModifyAtk`/`ModifySpA`/`ModifyDef`, `ModifyAccuracy` and
//! `ModifyDamage` as one `runEvent` each: every handler that applies calls `chainModify`, and
//! the chained modifier is applied to the value once at the end. The handlers run in
//! `speedSort` order (priority high to low, the holder's `pokemon.speed` high to low with side
//! and field effects at 0, then the effect type's sub-order), ties in random order. Chaining
//! rounds after every factor, so the order can change the result; [`chain`] reproduces it.
//!
//! Abilities of the target and its allies are ignored by a move that ignores abilities
//! ([`ability_for_move`]).

use crate::damage::{
    chain_modifiers, MOD_DOUBLE, MOD_HALF, MOD_ONE, MOD_ONE_POINT_FIVE, MOD_ONE_POINT_THREE,
    MOD_ONE_POINT_TWO, MOD_THREE_QUARTERS,
};
use crate::dex::{
    abilities, items, AbilityFlags, AbilityId, MoveCategory, MoveData, MoveFlags, Stat, Type,
};
use crate::field::Weather;
use crate::state::{Pokemon, SlotRef, Status};

use super::battle::Battle;
use super::order::modify;

/// Showdown's effect-type sub-orders (`resolvePriority`).
pub(crate) const SUB_MOVE: u32 = 0;
pub(crate) const SUB_SIDE_CONDITION: u32 = 4;
pub(crate) const SUB_FIELD_CONDITION: u32 = 5;
pub(crate) const SUB_ABILITY: u32 = 7;
pub(crate) const SUB_ITEM: u32 = 8;

/// One handler of a modifier event that applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Handler {
    pub priority: i32,
    /// The holder's `pokemon.speed`; 0 for side and field effects.
    pub speed: i32,
    pub sub_order: u32,
    /// The `chainModify` factor (4096 = 1x).
    pub modifier: u32,
}

impl Handler {
    /// A handler of the Pokémon in `holder` (its ability, item, or the move it uses).
    pub(crate) fn of<const N: usize>(
        b: &Battle<'_, N>,
        holder: SlotRef,
        priority: i32,
        sub_order: u32,
        modifier: u32,
    ) -> Handler {
        Handler {
            priority,
            speed: b.action_speed(holder),
            sub_order,
            modifier,
        }
    }

    /// A handler of a side or field effect.
    pub(crate) fn global(priority: i32, sub_order: u32, modifier: u32) -> Handler {
        Handler {
            priority,
            speed: 0,
            sub_order,
            modifier,
        }
    }
}

/// The event's chained modifier: the handlers' factors in Showdown's order.
pub(crate) fn chain<const N: usize>(b: &mut Battle<'_, N>, mut handlers: Vec<Handler>) -> u32 {
    handlers.retain(|h| h.modifier != MOD_ONE);
    // The first factor is exact and two factors commute: the order matters from three on.
    if handlers.len() >= 3 {
        speed_sort(b, &mut handlers);
    }
    let modifiers: Vec<u32> = handlers.iter().map(|h| h.modifier).collect();
    chain_modifiers(&modifiers, 0, u32::MAX)
}

/// Showdown `speedSort` with `comparePriority`. Ties are shuffled; a tie between equal
/// factors cannot change the result and is not branched on.
fn speed_sort<const N: usize>(b: &mut Battle<'_, N>, list: &mut [Handler]) {
    let key = |h: &Handler| (-h.priority, -h.speed, h.sub_order);
    let mut sorted = 0;
    while sorted < list.len() {
        let best = list[sorted..].iter().map(key).min().expect("non-empty");
        let tied: Vec<usize> = (sorted..list.len())
            .filter(|&i| key(&list[i]) == best)
            .collect();
        for (offset, &i) in tied.iter().enumerate() {
            list.swap(sorted + offset, i);
        }
        let count = tied.len();
        let group = &list[sorted..sorted + count];
        if group.iter().any(|h| h.modifier != group[0].modifier) {
            for k in 0..count {
                let pick = b.rng.uniform(count - k);
                list.swap(sorted + k, sorted + k + pick);
            }
        }
        sorted += count;
    }
}

/// A handler's `on<Event>Priority` from the dex (0 when unset).
pub(crate) fn priority(orders: &[(&str, i16)], name: &str) -> i32 {
    orders
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0, |&(_, p)| i32::from(p))
}

/// The ability of the Pokémon in `holder` as the handlers of `user`'s move see it. Showdown
/// `suppressingAbility` (gen 8+): a move that ignores abilities (Sunsteel Strike, Moongeist
/// Beam) skips the breakable abilities of everyone but its user, unless an Ability Shield
/// protects them.
pub(crate) fn ability_for_move<const N: usize>(
    b: &Battle<'_, N>,
    holder: SlotRef,
    user: SlotRef,
    data: &MoveData,
) -> AbilityId {
    let ability = b.ability(holder);
    let suppressed = data.ignore_ability
        && holder != user
        && ability.data().flags.contains(AbilityFlags::BREAKABLE)
        && b.item(holder) != items::ABILITY_SHIELD;
    if suppressed {
        AbilityId::NONE
    } else {
        ability
    }
}

/// `BasePower` handlers of abilities: the user's `onBasePower`, its side's `onAllyBasePower`
/// (which includes the user), the target's `onSourceBasePower`. `base_power` is the move's
/// power before the event; `move_type` is the type of the move being used (after
/// ModifyType), which every type check here reads instead of `data.move_type`.
pub(crate) fn base_power_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    move_type: Type,
    base_power: i32,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let flag = |f: MoveFlags, modifier: u32| data.flags.contains(f).then_some(modifier);
    let ability = b.ability(user);
    let boost = match ability {
        // `this.modify(basePower, this.event.modifier) <= 60`: Technician has the highest
        // BasePower priority (30), so no factor is chained before it.
        a if a == abilities::TECHNICIAN => (base_power <= 60).then_some(MOD_ONE_POINT_FIVE),
        a if a == abilities::IRON_FIST => flag(MoveFlags::PUNCH, MOD_ONE_POINT_TWO),
        a if a == abilities::RECKLESS => {
            (data.recoil.is_some() || data.has_crash_damage).then_some(MOD_ONE_POINT_TWO)
        }
        a if a == abilities::TOUGH_CLAWS => flag(MoveFlags::CONTACT, MOD_ONE_POINT_THREE),
        a if a == abilities::SHARPNESS => flag(MoveFlags::SLICING, MOD_ONE_POINT_FIVE),
        a if a == abilities::STRONG_JAW => flag(MoveFlags::BITE, MOD_ONE_POINT_FIVE),
        a if a == abilities::MEGA_LAUNCHER => flag(MoveFlags::PULSE, MOD_ONE_POINT_FIVE),
        a if a == abilities::PUNK_ROCK => flag(MoveFlags::SOUND, MOD_ONE_POINT_THREE),
        _ => None,
    };
    if let Some(modifier) = boost {
        let p = priority(ability.data().event_orders, "onBasePowerPriority");
        out.push(Handler::of(b, user, p, SUB_ABILITY, modifier));
    }
    // Steely Spirit: `onAllyBasePower` of every active Pokémon on the user's side.
    for holder in b.alive_slots(user.side) {
        let ability = ability_for_move(b, holder, user, data);
        if ability == abilities::STEELY_SPIRIT && move_type == Type::Steel {
            let p = priority(ability.data().event_orders, "onAllyBasePowerPriority");
            out.push(Handler::of(b, holder, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
        }
    }
    // Dry Skin: the target's `onSourceBasePower`, Fire moves `chainModify(1.25)`.
    let defending = ability_for_move(b, target, user, data);
    if defending == abilities::DRY_SKIN && move_type == Type::Fire {
        let p = priority(defending.data().event_orders, "onSourceBasePowerPriority");
        out.push(Handler::of(b, target, p, SUB_ABILITY, 5120));
    }
    out
}

/// `ModifyAtk` (physical moves) or `ModifySpA` (special moves) handlers of abilities: the
/// user's `onModifyAtk`/`onModifySpA` and the target's `onSourceModifyAtk`/`onSourceModifySpA`.
/// `move_type` is the type of the move being used.
pub(crate) fn attack_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    move_type: Type,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let physical = data.category == MoveCategory::Physical;
    let (event, source_event) = if physical {
        ("onModifyAtkPriority", "onSourceModifyAtkPriority")
    } else {
        ("onModifySpAPriority", "onSourceModifySpAPriority")
    };
    // The target's `onSourceModifyAtk`/`onSourceModifySpA`: halve moves of some types.
    let defending = ability_for_move(b, target, user, data);
    let halved: &[Type] = match defending {
        a if a == abilities::THICK_FAT => &[Type::Ice, Type::Fire],
        a if a == abilities::HEATPROOF || a == abilities::WATER_BUBBLE => &[Type::Fire],
        a if a == abilities::PURIFYING_SALT => &[Type::Ghost],
        _ => &[],
    };
    if halved.contains(&move_type) {
        let p = priority(defending.data().event_orders, source_event);
        out.push(Handler::of(b, target, p, SUB_ABILITY, MOD_HALF));
    }
    let Some(attacker) = b.slot_mon(user) else {
        return out;
    };
    // Water Bubble: the user's Water moves `chainModify(2)` (no priority).
    if attacker.ability == abilities::WATER_BUBBLE && move_type == Type::Water {
        let p = priority(attacker.ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_DOUBLE));
    }
    let ability = attacker.ability;
    // Blaze, Torrent, Overgrow, Swarm: `attacker.hp <= attacker.maxhp / 3`.
    let pinch_type = match ability {
        a if a == abilities::BLAZE => Some(Type::Fire),
        a if a == abilities::TORRENT => Some(Type::Water),
        a if a == abilities::OVERGROW => Some(Type::Grass),
        a if a == abilities::SWARM => Some(Type::Bug),
        _ => None,
    };
    let pinch = 3 * i32::from(attacker.hp) <= i32::from(attacker.max_hp);
    // Solar Power: `onModifySpA` 1.5x in harsh sunlight (`effectiveWeather`).
    if !physical && ability == abilities::SOLAR_POWER && b.weather() == Weather::Sun {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    // Guts: `if (pokemon.status) return this.chainModify(1.5)` (Attack only).
    let guts = ability == abilities::GUTS && physical && attacker.status != Status::None;
    if (pinch_type == Some(move_type) && pinch) || guts {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    out
}

/// `ModifyDef` or `ModifySpD` handlers of abilities (by the stat the move targets): the
/// target's `onModifyDef`/`onModifySpD`.
pub(crate) fn defense_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    defense_stat: Stat,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let Some(defender) = b.slot_mon(target) else {
        return out;
    };
    let ability = ability_for_move(b, target, user, data);
    // Marvel Scale: `if (pokemon.status) return this.chainModify(1.5)` (Defense only).
    if ability == abilities::MARVEL_SCALE
        && defense_stat == Stat::Def
        && defender.status != Status::None
    {
        let p = priority(ability.data().event_orders, "onModifyDefPriority");
        out.push(Handler::of(b, target, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    out
}

/// Burn damage before `battle.damage`: 1/16 of max HP, halved by Heatproof's `onDamage`
/// (`effect.id === 'brn'`: `damage / 2` after the integer clamp to at least 1).
pub(crate) fn burn_damage(ability: AbilityId, max_hp: f64) -> f64 {
    let damage = max_hp / 16.0;
    if ability == abilities::HEATPROOF {
        damage.floor().max(1.0) / 2.0
    } else {
        damage
    }
}

/// Ability `onSetStatus` handlers that block a status (they only return `false`, so their
/// order does not matter): Water Bubble blocks burns, Purifying Salt every status.
///
/// Showdown skips a breakable ability for a move that ignores abilities; no move the engine
/// supports both ignores abilities and inflicts a status (`support.rs` tests this).
pub(crate) fn blocks_status(ability: AbilityId, status: Status) -> bool {
    match ability {
        a if a == abilities::WATER_BUBBLE => status == Status::Burn,
        a if a == abilities::PURIFYING_SALT => true,
        _ => false,
    }
}

/// Whether a burned user's physical damage is halved: Showdown skips it for Guts
/// (`!pokemon.hasAbility('guts')`).
pub(crate) fn burn_halves(attacker: &Pokemon, data: &MoveData) -> bool {
    attacker.status == Status::Burn
        && data.category == MoveCategory::Physical
        && attacker.ability != abilities::GUTS
}

/// The attacking stat after the handlers of the `ModifyAtk` event that return a new value
/// instead of chaining: Hustle's `return this.modify(atk, 1.5)`. The chained factor is
/// applied to the result at the end of the event.
pub(crate) fn attack_direct(ability: AbilityId, data: &MoveData, attack: i32) -> i32 {
    if ability == abilities::HUSTLE && data.category == MoveCategory::Physical {
        modify(attack, MOD_ONE_POINT_FIVE)
    } else {
        attack
    }
}

/// `ModifyAccuracy` handlers of abilities (moves with a numeric accuracy): the user's
/// `onSourceModifyAccuracy`.
pub(crate) fn accuracy_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let ability = b.ability(user);
    // Hustle: physical moves 3277/4096.
    if ability == abilities::HUSTLE && data.category == MoveCategory::Physical {
        let p = priority(
            ability.data().event_orders,
            "onSourceModifyAccuracyPriority",
        );
        out.push(Handler::of(b, user, p, SUB_ABILITY, 3277));
    }
    out
}

/// The STAB modifier after `ModifySTAB` (the user's own ability). Adaptability:
/// `if (move.forceSTAB || source.hasType(move.type)) return stab === 2 ? 2.25 : 2`, where a
/// STAB of 2 needs Terastallization (not modelled), so 2.
pub(crate) fn modify_stab(ability: AbilityId, stab: bool) -> u32 {
    match (stab, ability) {
        (false, _) => MOD_ONE,
        (true, a) if a == abilities::ADAPTABILITY => MOD_DOUBLE,
        (true, _) => MOD_ONE_POINT_FIVE,
    }
}

/// `ModifyDamage` handlers of abilities: the target's `onSourceModifyDamage` and every active
/// Pokémon's `onAnyModifyDamage`. `type_mod` is the hit's clamped effectiveness exponent
/// (`getMoveHitData(move).typeMod`); `move_type` is the type of the move being used.
pub(crate) fn modify_damage_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    move_type: Type,
    type_mod: i32,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let Some(defender) = b.slot_mon(target) else {
        return out;
    };
    let contact = data.flags.contains(MoveFlags::CONTACT);
    let full_hp = defender.hp >= defender.max_hp;
    let ability = ability_for_move(b, target, user, data);
    let modifier = match ability {
        a if a == abilities::PUNK_ROCK => data.flags.contains(MoveFlags::SOUND).then_some(MOD_HALF),
        a if a == abilities::SOLID_ROCK
            || a == abilities::FILTER
            || a == abilities::PRISM_ARMOR =>
        {
            (type_mod > 0).then_some(MOD_THREE_QUARTERS)
        }
        a if a == abilities::MULTISCALE || a == abilities::SHADOW_SHIELD => {
            full_hp.then_some(MOD_HALF)
        }
        // `mod = 1; Fire: mod *= 2; contact: mod /= 2; chainModify(mod)`.
        a if a == abilities::FLUFFY => match (move_type == Type::Fire, contact) {
            (true, false) => Some(MOD_DOUBLE),
            (false, true) => Some(MOD_HALF),
            _ => None,
        },
        a if a == abilities::ICE_SCALES => {
            (data.category == MoveCategory::Special).then_some(MOD_HALF)
        }
        a if a == abilities::AURA_GUARD => contact.then_some(MOD_HALF),
        _ => None,
    };
    if let Some(modifier) = modifier {
        let p = priority(ability.data().event_orders, "onSourceModifyDamagePriority");
        out.push(Handler::of(b, target, p, SUB_ABILITY, modifier));
    }
    // Friend Guard: `onAnyModifyDamage` of every active Pokémon, 0.75x when the target is an
    // ally other than the holder itself (the holder may be the user hitting its own ally).
    for holder in b.alive_slots(target.side) {
        if holder == target {
            continue;
        }
        let ability = ability_for_move(b, holder, user, data);
        if ability == abilities::FRIEND_GUARD {
            let p = priority(ability.data().event_orders, "onAnyModifyDamagePriority");
            out.push(Handler::of(b, holder, p, SUB_ABILITY, MOD_THREE_QUARTERS));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::{AbilityId, ItemId, MoveId};

    /// Every BasePower handler priority in the dex.
    fn base_power_priorities() -> Vec<(String, i32)> {
        let names = [
            "onBasePowerPriority",
            "onAllyBasePowerPriority",
            "onAnyBasePowerPriority",
            "onFoeBasePowerPriority",
            "onSourceBasePowerPriority",
            "condition.onBasePowerPriority",
        ];
        let mut out = Vec::new();
        let mut add = |what: String, orders: &[(&str, i16)]| {
            for (n, p) in orders {
                if names.contains(n) {
                    out.push((format!("{what} {n}"), i32::from(*p)));
                }
            }
        };
        for id in AbilityId::all() {
            add(id.id().to_owned(), id.data().event_orders);
        }
        for id in ItemId::all() {
            add(id.id().to_owned(), id.data().event_orders);
        }
        for id in MoveId::all() {
            add(id.id().to_owned(), id.data().event_orders);
        }
        out
    }

    #[test]
    fn technician_runs_before_every_other_base_power_handler() {
        let technician = priority(
            abilities::TECHNICIAN.data().event_orders,
            "onBasePowerPriority",
        );
        assert_eq!(technician, 30);
        for (what, p) in base_power_priorities() {
            if !what.starts_with("technician ") {
                assert!(p < technician, "{what}: {p}");
            }
        }
    }

    #[test]
    fn handler_priorities_match_showdown() {
        let bp = |a: AbilityId| priority(a.data().event_orders, "onBasePowerPriority");
        assert_eq!(bp(abilities::IRON_FIST), 23);
        assert_eq!(bp(abilities::RECKLESS), 23);
        assert_eq!(bp(abilities::TOUGH_CLAWS), 21);
        assert_eq!(bp(abilities::SHARPNESS), 19);
        assert_eq!(bp(abilities::STRONG_JAW), 19);
        assert_eq!(bp(abilities::MEGA_LAUNCHER), 19);
        assert_eq!(bp(abilities::PUNK_ROCK), 7);
        assert_eq!(
            priority(
                abilities::STEELY_SPIRIT.data().event_orders,
                "onAllyBasePowerPriority"
            ),
            22
        );
        assert!(!abilities::STEELY_SPIRIT
            .data()
            .flags
            .contains(AbilityFlags::BREAKABLE));
        assert!(abilities::PUNK_ROCK
            .data()
            .flags
            .contains(AbilityFlags::BREAKABLE));
    }
}
