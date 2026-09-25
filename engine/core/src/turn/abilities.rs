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
    abilities, items, moves, AbilityFlags, AbilityId, ItemId, MoveCategory, MoveData, MoveFlags,
    MoveId, Stat, Type, NO_BOOSTS,
};
use crate::field::{SideEffect, Weather};
use crate::state::{Pokemon, SideId, SlotRef, State, Status};
use crate::volatile::{Volatile, VolatileState};

use super::battle::{cured_on_update, Battle, BoostEffect};
use super::order::modify;

/// Showdown's effect-type sub-orders (`resolvePriority`).
pub(crate) const SUB_MOVE: u32 = 0;
/// A Pokémon's volatile or status condition.
pub(crate) const SUB_CONDITION: u32 = 2;
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
/// Beam, or any move of a Mold Breaker user) skips the breakable abilities of everyone but its
/// user, unless an Ability Shield protects them.
pub(crate) fn ability_for_move<const N: usize>(
    b: &Battle<'_, N>,
    holder: SlotRef,
    user: SlotRef,
    data: &MoveData,
) -> AbilityId {
    let ability = b.ability(holder);
    let ignores = data.ignore_ability
        || b.active_move
            .is_some_and(|m| m.user == user && m.ignore_ability);
    let suppressed = ignores
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
        // Sheer Force: `if (move.hasSheerForce || move.hasSheerForceBoost)
        // return this.chainModify([5325, 4096])` (`hasSheerForce` from its own ModifyMove).
        a if a == abilities::SHEER_FORCE => {
            (sheer_force_deletes_secondaries(data) || data.has_sheer_force_boost).then_some(5325)
        }
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
    // The user's `charge` volatile (priority 9): `if (move.type === 'Electric')
    // return this.chainModify(2)`.
    if move_type == Type::Electric && b.volatile(user, Volatile::Charge).active {
        let p = priority(
            moves::CHARGE.data().event_orders,
            "condition.onBasePowerPriority",
        );
        out.push(Handler::of(b, user, p, SUB_CONDITION, MOD_DOUBLE));
    }
    out
}

/// The `charge` volatile's `onAfterMove` and `onMoveAborted` for the Pokémon in `user` after it
/// used (or failed to use) `id` of type `move_type`: an Electric move other than Charge ends it
/// (`removeVolatile`: nothing for a fainted user). `AfterMove` sees the type after ModifyType,
/// `MoveAborted` the move's own type.
pub(crate) fn charge_after_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    move_type: Type,
) {
    if move_type == Type::Electric && id != moves::CHARGE {
        b.remove_volatile(user, Volatile::Charge);
    }
}

/// `runEvent('SideConditionStart', side, source, condition)` after a side condition starts on
/// `side` (`side.addSideCondition`): Wind Power (`onSideConditionStart`, every active Pokémon
/// of that side) gets `charge` when it is Tailwind. Wind Rider, the other handler, is refused
/// on the field.
pub(crate) fn side_condition_start<const N: usize>(
    b: &mut Battle<'_, N>,
    side: SideId,
    effect: SideEffect,
) {
    if effect != SideEffect::Tailwind {
        return;
    }
    for holder in b.alive_slots(side) {
        if b.ability(holder) == abilities::WIND_POWER {
            b.add_volatile(holder, Volatile::Charge);
        }
    }
}

/// Anger Shell and Berserk (Champions): `onDamage` sets `abilityState.checked*` to
/// `!(effect.effectType === 'Move' && !effect.multihit)` — a single-hit move's damage (or a
/// confusion self-hit) leaves the half-HP check pending until `AfterMoveSecondary`, and the
/// holder's healing berries wait ([`try_eat_item`]); any other damage, or a multi-hit move's,
/// clears it. The pending check is [`Volatile::AngerShellUnchecked`]. Called from
/// `Battle::damage` for every damage that reaches the Damage event; `multihit` is whether the
/// damaging effect is a multi-hit move.
pub(crate) fn on_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    from_move: bool,
    multihit: bool,
) {
    if !has_berserk_check(b.ability(target)) {
        return;
    }
    let pending = from_move && !multihit;
    let state = b.volatile(target, Volatile::AngerShellUnchecked);
    if pending != state.active {
        b.set_volatile_state(
            target,
            Volatile::AngerShellUnchecked,
            VolatileState {
                active: pending,
                ..VolatileState::NONE
            },
        );
    }
}

fn has_berserk_check(ability: AbilityId) -> bool {
    ability == abilities::ANGER_SHELL || ability == abilities::BERSERK
}

/// Anger Shell's / Berserk's `onAfterMoveSecondary` for the targets of the move's last hit
/// (`afterMoveSecondaryEvent`, skipped for a Sheer Force-boosted move): the pending check is
/// cleared; then, for a target other than the user, still standing, after a move that dealt
/// damage (`move.totalDamage`), with `damage` the HP the hit took from it
/// (`getLastAttackedBy().damage`, or `move.totalDamage` for a multi-hit move): if the damage took
/// it from above half its max HP to half or below, Anger Shell raises Atk, SpA and Spe by 1 and
/// lowers Def and SpD by 1, Berserk raises SpA by 1 (the holder is its own source). Their
/// relative order with the other AfterMoveSecondary handlers does not matter: each only
/// changes its holder.
pub(crate) fn after_move_secondary<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    damage: i32,
    total_damage: i32,
) {
    let ability = b.ability(target);
    if !has_berserk_check(ability) || b.occupant(target).is_none() {
        return;
    }
    b.delete_volatile(target, Volatile::AngerShellUnchecked);
    let Some(mon) = b.alive(target).map(|p| b.mon(p)) else {
        return;
    };
    if target == user || total_damage == 0 {
        return;
    }
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    // `target.hp <= target.maxhp / 2 && target.hp + damage > target.maxhp / 2`.
    if 2 * hp <= max_hp && 2 * (hp + damage) > max_hp {
        let mut boosts = NO_BOOSTS;
        if ability == abilities::ANGER_SHELL {
            boosts = [1, -1, 1, -1, 1, 0, 0];
        } else {
            boosts[2] = 1;
        }
        b.boost_by(target, &boosts, Some(target), BoostEffect::Ability(ability));
    }
}

/// The ability `onUpdate` handlers of the Pokémon in `slot` (`eachEvent('Update')`, before its
/// item's): the status cures of [`cured_on_update`] and Own Tempo's confusion cure
/// (`removeVolatile('confusion')`). All are breakable: a move that ignores abilities suppresses
/// them at the Update after its hit, and the Update after the action cures. Each only changes
/// its holder, so their order across Pokémon does not matter.
pub(crate) fn on_update<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let ability = b.ability_unless_broken(slot);
    if cured_on_update(ability, b.mon(pokemon).status) {
        b.cure_status(pokemon);
    }
    if ability == abilities::OWN_TEMPO && b.volatile(slot, Volatile::Confusion).active {
        b.remove_volatile(slot, Volatile::Confusion);
    }
}

/// `runEvent('SwitchOut')` for a healthy Pokémon leaving `slot` (WORKPLAN O54; Champions
/// versions, `data/mods/champions/abilities.ts`): Regenerator `pokemon.heal(baseMaxhp / 3)`
/// (truncated; nothing at full HP); Natural Cure `if (!pokemon.status || pokemon.status ===
/// 'fnt') return; pokemon.clearStatus()` (its `onCheckShow` is removed in Champions).
pub(crate) fn on_switch_out<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    match b.ability(slot) {
        a if a == abilities::REGENERATOR => {
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            b.heal(slot, max_hp / 3.0);
        }
        a if a == abilities::NATURAL_CURE => b.cure_status(pokemon),
        _ => {}
    }
}

/// Unburden (WORKPLAN O64) when its holder in `slot` used or lost its item: `onAfterUseItem`
/// (`useItem`, `eatItem`, Air Balloon's pop) and `onTakeItem` (`takeItem`: Knock Off, Trick,
/// Sticky Barb; it runs before the item's own TakeItem handler, so even a Mega Stone that stays
/// adds it) both `addVolatile('unburden')` (nothing on a fainted holder or when it is up). The
/// volatile doubles Speed while the holder has no item (`order.rs`).
pub(crate) fn unburden<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.ability(slot) == abilities::UNBURDEN {
        b.add_volatile(slot, Volatile::Unburden);
    }
}

/// The healing items Anger Shell's and Berserk's `onTryEatItem` hold back while their check is
/// pending.
const HEALING_BERRIES: [ItemId; 9] = [
    items::AGUAV_BERRY,
    items::ENIGMA_BERRY,
    items::FIGY_BERRY,
    items::IAPAPA_BERRY,
    items::MAGO_BERRY,
    items::SITRUS_BERRY,
    items::WIKI_BERRY,
    items::ORAN_BERRY,
    items::BERRY_JUICE,
];

/// `runEvent('TryEatItem', eater, null, null, item)` for the implemented handlers: Anger Shell
/// and Berserk (the eater's own ability) refuse a healing berry while their check is pending;
/// Unnerve (`onFoeTryEatItem`, WORKPLAN O63) on a foe not fainted (`foes()`) refuses every
/// berry once it has started (`effectState.unnerved`: an active holder always has, its start
/// runs first among the switch-in handlers). They only return booleans, so their order does
/// not matter. `false` = the berry is not eaten.
pub(crate) fn try_eat_item<const N: usize>(b: &Battle<'_, N>, eater: SlotRef) -> bool {
    let Some(mon) = b.slot_mon(eater) else {
        return false;
    };
    let item = mon.item;
    let pending = has_berserk_check(mon.ability)
        && b.volatile(eater, Volatile::AngerShellUnchecked).active
        && HEALING_BERRIES.contains(&item);
    let unnerved = b
        .alive_slots(eater.side.other())
        .into_iter()
        .any(|foe| b.ability(foe) == abilities::UNNERVE);
    !pending && !unnerved
}

/// The active Pokémon whose Flower Veil (breakable) protects `target`: `target` must be a Grass
/// type, and the holder is the target itself or an ally not at 0 HP (`onAlly*` handlers come from
/// `alliesAndSelf()`). The handlers only block, so which holder answers does not matter.
/// - `onAllyTryBoost`: every drop from another Pokémon (or from no source) is deleted
///   (`Battle::boost_by`, with [`flower_veil_first`]).
/// - `onAllySetStatus`: a status from another Pokémon is blocked unless the effect is Yawn
///   (`Battle::try_set_status_from`; Yawn's own end passes no source).
/// - `onAllyTryAddVolatile`: Yawn is blocked (`Battle::add_volatile_blocked`).
pub(crate) fn flower_veil_holder<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
) -> Option<SlotRef> {
    if !b.has_type(target, Type::Grass) {
        return None;
    }
    b.alive_slots(target.side)
        .into_iter()
        .find(|&s| b.ability_unless_broken(s) == abilities::FLOWER_VEIL)
}

/// Whether Flower Veil's `onAllyTryBoost` deletes the drops in `boost` before the target's own
/// Mirror Armor (`onTryBoost`) can reflect them: both have priority 0, so their holders' Speed
/// orders them (a tie at random), and whichever runs first leaves the other no drop. `false`
/// when no Flower Veil protects the target; `true` when Mirror Armor would not reflect anything
/// (the caller zeroes the remaining drops after the other TryBoost handlers either way). Guard
/// Dog (priority 2) runs before both; the other TryBoost handlers only delete.
pub(crate) fn flower_veil_first<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    boost: &[i8; crate::state::BOOST_COUNT],
    source: Option<SlotRef>,
    effect: BoostEffect,
) -> bool {
    let Some(holder) = flower_veil_holder(b, target) else {
        return false;
    };
    let stages = b.state.slot(target).boosts;
    let mirror_armor = b.ability_unless_broken(target) == abilities::MIRROR_ARMOR
        && source.is_some_and(|s| s != target)
        && effect != BoostEffect::Ability(abilities::MIRROR_ARMOR)
        && (0..boost.len()).any(|i| boost[i] < 0 && stages[i] > -6);
    if !mirror_armor {
        return true;
    }
    match b.action_speed(holder).cmp(&b.action_speed(target)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b.rng.uniform(2) == 0,
    }
}

/// Whether the active Pokémon in `slot` is trapped, so it cannot choose to switch: Showdown's
/// `pokemon.trapped` as `endTurn` sets it before the choices (`runEvent('TrapPokemon')`). The
/// state between turns is the state `endTurn` saw (after the replacements), so it is derived
/// here instead of stored. Implemented handlers:
/// - the foes' `onFoeTrapPokemon` (every foe not at 0 HP is adjacent in singles and doubles):
///   Shadow Tag traps a Pokémon without Shadow Tag, Arena Trap a grounded one, Magnet Pull a
///   Steel type, each through `tryTrap`, which the Ghost type's `trapped` immunity stops (no
///   `Immunity` handler covers `trapped`);
/// - Shed Shell's `onTrapPokemon` (priority -10, after every other): `pokemon.trapped = false`.
///
/// `onFoeMaybeTrapPokemon` only sets the `maybeTrapped` display flag. Other trapping effects
/// (Mean Look, partial trapping, Ingrain, Fairy Lock, ...) are not implemented. A switch the
/// move request forbids is rejected by `Ruleset::validate_slot_action` (`ActionError::Trapped`),
/// so `Ruleset::joint_actions` never generates it; forced switches (replacements) ignore it.
pub fn trapped<const N: usize>(state: &State<N>, slot: SlotRef) -> bool {
    let foe_traps = State::<N>::slot_refs().any(|s| {
        s.side != slot.side
            && state.active(s).is_some_and(|m| {
                m.hp > 0
                    && [
                        abilities::SHADOW_TAG,
                        abilities::ARENA_TRAP,
                        abilities::MAGNET_PULL,
                    ]
                    .contains(&m.ability)
            })
    });
    if !foe_traps {
        return false;
    }
    // Grounding needs the battle's view of the state (no move in progress between turns).
    let mut copy = state.clone();
    let mut chooser = super::branch::Chooser::new();
    let b = Battle::new(&mut copy, &mut chooser);
    trapped_in(&b, slot)
}

fn trapped_in<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return false;
    };
    let immune = b.natural_immune(slot, crate::dex::TypeImmunities::TRAPPED);
    let trapped = !immune
        && b.alive_slots(slot.side.other())
            .into_iter()
            .any(|foe| match b.ability(foe) {
                a if a == abilities::SHADOW_TAG => mon.ability != abilities::SHADOW_TAG,
                a if a == abilities::ARENA_TRAP => b.is_grounded(slot),
                a if a == abilities::MAGNET_PULL => mon.types.contains(&Type::Steel),
                _ => false,
            });
    trapped && mon.item != items::SHED_SHELL
}

// ---- Protosynthesis / Quark Drive / Booster Energy (WORKPLAN O72, O98) ------------------------

/// The condition a paradox ability adds, and whether its field state holds now: Protosynthesis
/// reads `this.field.isWeather('sunnyday')` (the field's effective weather: Cloud Nine and Air
/// Lock hide it, Utility Umbrella does not), Quark Drive `isTerrain('electricterrain')`.
fn paradox<const N: usize>(b: &Battle<'_, N>, ability: AbilityId) -> Option<(Volatile, bool)> {
    match ability {
        a if a == abilities::PROTOSYNTHESIS => Some((
            Volatile::Protosynthesis,
            b.effective_weather() == Weather::Sun,
        )),
        a if a == abilities::QUARK_DRIVE => Some((
            Volatile::QuarkDrive,
            b.terrain() == crate::field::Terrain::Electric,
        )),
        _ => None,
    }
}

/// Showdown `pokemon.getBestStat(false, true)`: the first of Atk, Def, SpA, SpD, Spe with the
/// highest stored stat after its stage (no modifiers; under Wonder Room Def and SpD keep their
/// stored values but take the other's stage, as Download does).
fn best_stat<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> u16 {
    let Some(mon) = b.slot_mon(slot) else {
        return 0;
    };
    let boosts = b.state.slot(slot).boosts;
    let wonder_room = b.field_active(crate::field::FieldEffect::WonderRoom);
    let mut best = (0u16, 0);
    for stat in 0..5usize {
        let stage = match stat {
            1 if wonder_room => boosts[3],
            3 if wonder_room => boosts[1],
            _ => boosts[stat],
        };
        let value = super::order::boosted_stat(i32::from(mon.stats[stat]), stage);
        if value > best.1 {
            best = (stat as u16, value);
        }
    }
    best.0
}

/// `pokemon.addVolatile('protosynthesis' / 'quarkdrive')`: nothing on a fainted Pokémon or when
/// it is up (no `onRestart`); the condition's `onStart` stores `bestStat` and, when Booster
/// Energy added it, `fromBooster`.
fn add_paradox<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    volatile: Volatile,
    from_booster: bool,
) {
    if b.alive(slot).is_none() || b.volatile(slot, volatile).active {
        return;
    }
    let state = VolatileState {
        active: true,
        counter: best_stat(b, slot),
        hidden: u8::from(from_booster),
        ..VolatileState::NONE
    };
    b.set_volatile_state(slot, volatile, state);
}

/// Protosynthesis's `onWeatherChange` / Quark Drive's `onTerrainChange` for the Pokémon in
/// `slot` (also their `onStart`, which runs the same handler): while the weather / terrain
/// holds, `addVolatile`; otherwise the condition ends (`removeVolatile`: its `onEnd` only
/// announces it) unless Booster Energy added it. Each only changes its holder, so the event's
/// Speed order does not matter.
pub(crate) fn paradox_change<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.alive(slot).is_none() {
        return;
    }
    let Some((volatile, holds)) = paradox(b, b.ability(slot)) else {
        return;
    };
    if holds {
        add_paradox(b, slot, volatile, false);
    } else if b.volatile(slot, volatile).hidden == 0 {
        b.remove_volatile(slot, volatile);
    }
}

/// Booster Energy's `onUpdate` (every Update, and from its `onStart` at switch-in; an active
/// holder has always started: the item cannot be given mid-battle by a supported effect): a
/// Protosynthesis holder outside sun or a Quark Drive holder outside Electric Terrain uses the
/// item (`useItem`: `lastItem`, Unburden), then `addVolatile` with `fromBooster`.
pub(crate) fn booster_energy<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.item(slot) != items::BOOSTER_ENERGY || b.alive(slot).is_none() {
        return;
    }
    let Some((volatile, holds)) = paradox(b, b.ability(slot)) else {
        return;
    };
    if !holds && b.use_item(slot) {
        add_paradox(b, slot, volatile, true);
    }
}

/// Booster Energy's `onTakeItem`: `if (source.baseSpecies.tags.includes("Paradox")) return
/// false` (the parameter is the holder). The Paradox tag is in the dex export's species `tags`,
/// which the Rust tables leave out; these are the species tagged in `data/champions.json`.
pub(crate) fn booster_energy_kept(species: crate::dex::SpeciesId) -> bool {
    use crate::dex::species;
    const PARADOX: [crate::dex::SpeciesId; 16] = [
        species::GREAT_TUSK,
        species::SCREAM_TAIL,
        species::BRUTE_BONNET,
        species::FLUTTER_MANE,
        species::SLITHER_WING,
        species::SANDY_SHOCKS,
        species::IRON_TREADS,
        species::IRON_BUNDLE,
        species::IRON_HANDS,
        species::IRON_JUGULIS,
        species::IRON_MOTH,
        species::IRON_THORNS,
        species::ROARING_MOON,
        species::IRON_VALIANT,
        species::WALKING_WAKE,
        species::IRON_LEAVES,
    ];
    let base = species.data().base_species;
    let base = if base.is_none() { species } else { base };
    PARADOX.contains(&base)
}

/// Why a Protosynthesis holder and a weather-suppressing ability (Air Lock, Cloud Nine) cannot
/// be on the field together: the suppressor leaving (fainting, switching out) runs its `onEnd`
/// `WeatherChange`, which would start Protosynthesis in sun, and the engine does not run an
/// ability's `End` there.
pub(crate) fn paradox_suppressor_problem<const N: usize>(state: &State<N>) -> Option<String> {
    let actives: Vec<&Pokemon> = State::<N>::slot_refs()
        .filter_map(|s| state.active(s))
        .filter(|m| m.hp > 0)
        .collect();
    let paradox = actives
        .iter()
        .any(|m| m.ability == abilities::PROTOSYNTHESIS);
    let suppressor = actives.iter().any(|m| m.ability.data().suppress_weather);
    (paradox && suppressor).then(|| {
        "Protosynthesis next to Air Lock / Cloud Nine (the suppressor's End WeatherChange)".into()
    })
}

/// Sheer Force's `onModifyMove` condition: `move.secondaries && !move.hasSheerForceBoost`.
pub(crate) fn sheer_force_deletes_secondaries(data: &MoveData) -> bool {
    !data.secondaries.is_empty() && !data.has_sheer_force_boost
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
    if !physical && ability == abilities::SOLAR_POWER && b.weather_for(user) == Weather::Sun {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    // Guts: `if (pokemon.status) return this.chainModify(1.5)` (Attack only).
    let guts = ability == abilities::GUTS && physical && attacker.status != Status::None;
    if (pinch_type == Some(move_type) && pinch) || guts {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    // Flash Fire's volatile (a condition, priority 5): `if (move.type === 'Fire' &&
    // attacker.hasAbility('flashfire')) return this.chainModify(1.5)` (`move.type`: after
    // ModifyType).
    if move_type == Type::Fire
        && ability == abilities::FLASH_FIRE
        && b.volatile(user, Volatile::FlashFire).active
    {
        let name = if physical {
            "condition.onModifyAtkPriority"
        } else {
            "condition.onModifySpAPriority"
        };
        let p = priority(ability.data().event_orders, name);
        out.push(Handler::of(b, user, p, SUB_CONDITION, MOD_ONE_POINT_FIVE));
    }
    // Protosynthesis / Quark Drive's condition (priority 5): 5325/4096 when the best stat is
    // the one the event is for (`ModifyAtk` for physical moves, `ModifySpA` for special ones,
    // whatever stat the move attacks with). A volatile, so no ability-ignoring move skips it.
    let wanted = if physical { 0 } else { 2 };
    if let Some(v) = paradox_volatile_of(b, user).filter(|&(_, best)| best == wanted) {
        let name = if physical {
            "condition.onModifyAtkPriority"
        } else {
            "condition.onModifySpAPriority"
        };
        let p = priority(v.0.data().event_orders, name);
        out.push(Handler::of(b, user, p, SUB_CONDITION, 5325));
    }
    out
}

/// The paradox ability whose condition the Pokémon in `slot` has, with the condition's best
/// stat (0 Atk, 1 Def, 2 SpA, 3 SpD, 4 Spe; the condition's handlers are in the ability's data).
pub(crate) fn paradox_volatile_of<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
) -> Option<(AbilityId, u16)> {
    let proto = b.volatile(slot, Volatile::Protosynthesis);
    if proto.active {
        return Some((abilities::PROTOSYNTHESIS, proto.counter));
    }
    let quark = b.volatile(slot, Volatile::QuarkDrive);
    quark
        .active
        .then_some((abilities::QUARK_DRIVE, quark.counter))
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
    // Protosynthesis / Quark Drive's condition (priority 6): 5325/4096 on the best stat.
    let wanted = if defense_stat == Stat::Def { 1 } else { 3 };
    if let Some(v) = paradox_volatile_of(b, target).filter(|&(_, best)| best == wanted) {
        let name = if defense_stat == Stat::Def {
            "condition.onModifyDefPriority"
        } else {
            "condition.onModifySpDPriority"
        };
        let p = priority(v.0.data().event_orders, name);
        out.push(Handler::of(b, target, p, SUB_CONDITION, 5325));
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
/// order does not matter): Water Bubble blocks burns, Purifying Salt every status. The caller
/// passes the ability as the move in progress sees it (`ability_unless_broken`).
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
