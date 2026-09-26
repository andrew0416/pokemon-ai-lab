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
    abilities, items, moves, AbilityFlags, AbilityId, Gender, ItemId, MoveCategory, MoveData,
    MoveFlags, MoveId, SpeciesId, Stat, Type, NO_BOOSTS,
};
use crate::field::{SideEffect, Weather};
use crate::state::{Pokemon, PokemonRef, SideId, SlotRef, State, Status};
use crate::volatile::{Volatile, VolatileState};

use super::battle::{cured_on_update, Battle, BoostEffect};
use super::order::modify;

/// Showdown's effect-type sub-orders (`resolvePriority`).
pub(crate) const SUB_MOVE: u32 = 0;
/// A Pokémon's volatile or status condition.
pub(crate) const SUB_CONDITION: u32 = 2;
pub(crate) const SUB_SLOT_CONDITION: u32 = 3;
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
            speed: b.event_speed(holder),
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

// ---- ability suppression (Gastro Acid, Neutralizing Gas) ------------------------------------

/// Showdown `pokemon.ignoringAbility()` for the Pokémon in `slot` (no occupant: `true`, an
/// inactive Pokémon ignores its ability): a `cantsuppress` ability never is; the `gastroacid`
/// volatile suppresses; otherwise an Ability Shield (the effective item) or holding Neutralizing
/// Gas itself protects, and any active Pokémon with Neutralizing Gas (raw: `pokemon.ability`)
/// that is neither Gastro Acid'd nor ending (`abilityState.ending`,
/// [`Volatile::NeutralizingGasEnding`]) suppresses it, unless it is commanding. Transform is not
/// modelled (its `notransform` branch never applies).
pub(crate) fn ignoring_ability<const N: usize>(state: &State<N>, slot: SlotRef) -> bool {
    let Some(mon) = state.active(slot) else {
        return true;
    };
    let volatiles = &state.slot(slot).volatiles;
    let gastro_acid = volatiles.has(Volatile::GastroAcid);
    // The common case first: nothing on the field suppresses anything.
    if !gastro_acid && !neutralizing_gas_on_field(state) {
        return false;
    }
    if mon
        .ability
        .data()
        .flags
        .contains(AbilityFlags::CANTSUPPRESS)
    {
        return false;
    }
    if gastro_acid {
        return true;
    }
    let shielded = mon.item == items::ABILITY_SHIELD && !super::items::ignoring_item(state, slot);
    if shielded || mon.ability == abilities::NEUTRALIZING_GAS {
        return false;
    }
    !volatiles.has(Volatile::Commanding)
}

/// Whether an active Pokémon (fainted or not, until its faint is processed) suppresses the
/// others' abilities with Neutralizing Gas: `pokemon.ability === 'neutralizinggas' &&
/// !pokemon.volatiles['gastroacid'] && !pokemon.abilityState.ending`.
fn neutralizing_gas_on_field<const N: usize>(state: &State<N>) -> bool {
    State::<N>::slot_refs().any(|s| {
        state
            .active(s)
            .is_some_and(|m| m.ability == abilities::NEUTRALIZING_GAS)
            && !state.slot(s).volatiles.has(Volatile::GastroAcid)
            && !state.slot(s).volatiles.has(Volatile::NeutralizingGasEnding)
    })
}

/// The ability whose handlers act for the Pokémon in `slot` (`hasAbility`, `runEvent`):
/// `NONE` while it is suppressed ([`ignoring_ability`]).
pub(crate) fn effective_ability<const N: usize>(state: &State<N>, slot: SlotRef) -> AbilityId {
    match state.active(slot) {
        Some(mon) if !ignoring_ability(state, slot) => mon.ability,
        _ => AbilityId::NONE,
    }
}

/// Neutralizing Gas's `onSwitchIn` (priority 2) for its holder in `holder`: its
/// `abilityState.ending` starts false (a switch-in's volatiles are fresh). For the other active
/// Pokémon (not behind an Ability Shield, not commanding) it only ends Illusion and the primal
/// weathers, which are refused on the field. No ability's `End` runs: the suppressed abilities
/// just stop acting ([`ignoring_ability`]).
pub(crate) fn neutralizing_gas_switch_in<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) {
    b.delete_volatile(holder, Volatile::NeutralizingGasEnding);
}

/// Neutralizing Gas's `onEnd` for a holder leaving the field or losing its ability (switching
/// out, `setAbility`, Gastro Acid's `End`): `source` is its slot while it is still there, `None`
/// once it has fainted and left (the caller then checks its `ending` itself, and the holder no
/// longer counts: at 0 HP it is in no `foes()` list either).
/// - Nothing if another active Pokémon has Neutralizing Gas that acts (`hasAbility`), or if the
///   holder's `ending` is set already; otherwise `ending` is set (the holder stops suppressing).
/// - Every other active Pokémon, by `speedSort` (`pokemon.speed`, ties at random), whose ability
///   is not `cantsuppress` and that holds no Ability Shield (the effective item) runs its
///   ability's `Start` again (`singleEvent('Start')`, skipped while it is still suppressed:
///   `switching::start_ability`). Gluttony's `abilityState.gluttony = false` after its restart
///   is state the engine does not keep, so that restart is unsupported.
pub(crate) fn neutralizing_gas_end<const N: usize>(
    b: &mut Battle<'_, N>,
    source: Option<SlotRef>,
) -> Result<(), super::TurnError> {
    let others: Vec<SlotRef> = State::<N>::slot_refs()
        .filter(|&s| Some(s) != source && b.occupant(s).is_some())
        .collect();
    if others
        .iter()
        .any(|&s| b.ability(s) == abilities::NEUTRALIZING_GAS)
    {
        return Ok(());
    }
    if let Some(source) = source {
        if b.volatile(source, Volatile::NeutralizingGasEnding).active {
            return Ok(());
        }
        let ending = VolatileState {
            active: true,
            ..VolatileState::NONE
        };
        b.set_volatile_state(source, Volatile::NeutralizingGasEnding, ending);
    }
    let restarting: Vec<SlotRef> = others
        .into_iter()
        .filter(|&s| {
            !b.raw_ability(s)
                .data()
                .flags
                .contains(AbilityFlags::CANTSUPPRESS)
                && b.item(s) != items::ABILITY_SHIELD
        })
        .collect();
    let acts = |b: &Battle<'_, N>, s: SlotRef| {
        let ability = b.raw_ability(s);
        b.ability(s) == ability
            && super::switching::start_effect(ability) != Some(super::switching::StartEffect::None)
    };
    for &s in &restarting {
        if b.alive(s).is_none() && acts(b, s) {
            return Err(b.unsupported(format!(
                "{} restarting after Neutralizing Gas at 0 HP",
                b.raw_ability(s).data().name
            )));
        }
    }
    let order = speed_sorted(b, restarting, acts);
    for slot in order {
        let ability = b.raw_ability(slot);
        if b.alive(slot).is_none() {
            continue;
        }
        if ability == abilities::GLUTTONY {
            return Err(b.unsupported(
                "Gluttony restarting after Neutralizing Gas (its abilityState.gluttony = false)",
            ));
        }
        super::switching::start_ability(b, slot, ability)?;
    }
    Ok(())
}

/// Gastro Acid's condition `onStart` once the volatile is added to the Pokémon in `slot` (its
/// Ability Shield check is in `conditions::volatile_start`): the ability's `End`
/// (`singleEvent('End')` runs even for a suppressed ability: `switching::end_ability`, with
/// Neutralizing Gas's there).
pub(crate) fn gastro_acid_start<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<(), super::TurnError> {
    let ability = b.raw_ability(slot);
    super::switching::end_ability(b, slot, ability)
}

// ---- Poison Heal, Slow Start, Truant ----------------------------------------------------------

/// Poison Heal's `onDamage` (priority 1, before every other Damage handler) for the poison or
/// toxic damage of the Pokémon in `slot`: `this.heal(target.baseMaxhp / 8); return false;` — it
/// heals instead (`battle.heal`: nothing at full HP, Heal Block stops it) and takes no damage.
/// Returns whether it acted.
pub(crate) fn poison_heal<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) -> bool {
    let Some(mon) = b.slot_mon(slot) else {
        return false;
    };
    if b.ability(slot) != abilities::POISON_HEAL {
        return false;
    }
    let max_hp = f64::from(mon.max_hp);
    b.heal(slot, max_hp / 8.0);
    true
}

/// Slow Start's `onStart`: `this.effectState.counter = 5` ([`Volatile::SlowStart`]).
pub(crate) fn slow_start_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.occupant(slot).is_none() {
        return;
    }
    let state = VolatileState {
        active: true,
        counter: 5,
        ..VolatileState::NONE
    };
    b.set_volatile_state(slot, Volatile::SlowStart, state);
}

/// Whether Slow Start halves the Attack (`onModifyAtk`, priority 5) and Speed (`onModifySpe`)
/// of the Pokémon in `slot`: its ability acts and `effectState.counter` is not 0.
pub(crate) fn slow_start_halves<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    b.ability(slot) == abilities::SLOW_START && b.volatile(slot, Volatile::SlowStart).counter > 0
}

/// Truant's `onStart` for the Pokémon in `slot`: `pokemon.removeVolatile('truant')`, then `if
/// (pokemon.activeTurns && (pokemon.moveThisTurnResult !== undefined ||
/// !this.queue.willMove(pokemon))) pokemon.addVolatile('truant')` — a holder that was already
/// active when the turn started (`activeTurns`: not `newlySwitched`) and has moved this turn or
/// has no move to come loafs at its next move.
pub(crate) fn truant_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.alive(slot).is_none() {
        return;
    }
    b.remove_volatile(slot, Volatile::Truant);
    let history = b.state.slot(slot).history;
    let moved = history.move_this_turn_result != crate::state::MoveResult::Undefined;
    if !history.newly_switched && (moved || b.will_move(slot).is_none()) {
        b.add_volatile(slot, Volatile::Truant);
    }
}

/// Truant's `onBeforeMove` (priority 9: after the recharge turn, sleep and freeze, before
/// flinching) for the Pokémon in `slot`: with the `truant` volatile it loafs (`removeVolatile`,
/// `false`); otherwise it gets the volatile and moves. `false` = the move is not used.
pub(crate) fn truant_before_move<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) -> bool {
    if b.ability(slot) != abilities::TRUANT {
        return true;
    }
    if b.remove_volatile(slot, Volatile::Truant) {
        return false;
    }
    b.add_volatile(slot, Volatile::Truant);
    true
}

/// The residual order and sub-order of an ability's `onResidual` (`onResidualOrder`, and
/// `onResidualSubOrder` or the ability's effect-type sub-order).
pub(crate) fn residual_order(ability: AbilityId) -> (u32, u32) {
    let orders = ability.data().event_orders;
    let order = orders
        .iter()
        .find(|(n, _)| *n == "onResidualOrder")
        .map_or(super::order::ORDER_DEFAULT, |&(_, p)| p as u32);
    let sub_order = orders
        .iter()
        .find(|(n, _)| *n == "onResidualSubOrder")
        .map_or(SUB_ABILITY, |&(_, p)| p as u32);
    (order, sub_order)
}

/// Whether `ability` has an `onResidual` run by [`on_residual`] (`residual.rs` collects it).
pub(crate) fn has_residual(ability: AbilityId) -> bool {
    ability == abilities::SLOW_START
}

/// An ability's `onResidual` for its holder in `slot` (the caller checked that the ability
/// still acts):
/// - Slow Start: `if (pokemon.activeTurns && this.effectState.counter)` the counter drops by
///   one, and at 0 it is gone (`activeTurns` at the residual: the holder was active since the
///   turn started, `Battle::active_since_turn_start`).
pub(crate) fn on_residual<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    ability: AbilityId,
) -> Result<(), super::TurnError> {
    if ability == abilities::SLOW_START {
        let mut state = b.volatile(slot, Volatile::SlowStart);
        if b.active_since_turn_start(slot) && state.counter > 0 {
            state.counter -= 1;
            if state.counter == 0 {
                state = VolatileState::NONE;
            }
            b.set_volatile_state(slot, Volatile::SlowStart, state);
        }
    }
    Ok(())
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
        a if a == abilities::TOUGH_CLAWS => {
            super::items::makes_contact(b, user, data).then_some(MOD_ONE_POINT_THREE)
        }
        a if a == abilities::SHARPNESS => flag(MoveFlags::SLICING, MOD_ONE_POINT_FIVE),
        a if a == abilities::STRONG_JAW => flag(MoveFlags::BITE, MOD_ONE_POINT_FIVE),
        a if a == abilities::MEGA_LAUNCHER => flag(MoveFlags::PULSE, MOD_ONE_POINT_FIVE),
        a if a == abilities::PUNK_ROCK => flag(MoveFlags::SOUND, MOD_ONE_POINT_THREE),
        // Analytic (priority 21): `[5325, 4096]` unless another active Pokémon still has a move
        // in the queue (`this.queue.willMove(target)` over `getAllActive()`).
        a if a == abilities::ANALYTIC => {
            let moves_later = b
                .all_alive()
                .into_iter()
                .any(|s| s != user && b.will_move(s).is_some());
            (!moves_later).then_some(5325)
        }
        // Toxic Boost / Flare Boost (priority 19): a poisoned user's physical moves, a burned
        // user's special moves, 1.5x.
        a if a == abilities::TOXIC_BOOST => {
            let poisoned = b
                .slot_mon(user)
                .is_some_and(|m| matches!(m.status, Status::Poison | Status::Toxic));
            (poisoned && data.category == MoveCategory::Physical).then_some(MOD_ONE_POINT_FIVE)
        }
        a if a == abilities::FLARE_BOOST => {
            let burned = b.slot_mon(user).is_some_and(|m| m.status == Status::Burn);
            (burned && data.category == MoveCategory::Special).then_some(MOD_ONE_POINT_FIVE)
        }
        // Sand Force (priority 21): `[5325, 4096]` for Rock, Ground and Steel moves while
        // `this.field.isWeather('sandstorm')` (the field's effective weather).
        a if a == abilities::SAND_FORCE => {
            let sand = b.effective_weather() == Weather::Sand;
            let typed = matches!(move_type, Type::Rock | Type::Ground | Type::Steel);
            (sand && typed).then_some(5325)
        }
        // Rivalry (priority 24): with both genders known (not genderless), 1.25x for the same
        // gender, 0.75x for the other ([`rivalry_problem`] refuses an undecided gender).
        a if a == abilities::RIVALRY => {
            let gender = |slot: SlotRef| b.slot_mon(slot).map(|m| m.gender);
            match (gender(user), gender(target)) {
                (Some(mine), Some(theirs))
                    if mine != Gender::Genderless && theirs != Gender::Genderless =>
                {
                    Some(if mine == theirs {
                        5120
                    } else {
                        MOD_THREE_QUARTERS
                    })
                }
                _ => None,
            }
        }
        // Supreme Overlord (priority 21): `[powMod[fallen], 4096]` with the count its `onStart`
        // stored ([`supreme_overlord_start`]).
        a if a == abilities::SUPREME_OVERLORD => {
            const POW_MOD: [u32; 6] = [4096, 4506, 4915, 5325, 5734, 6144];
            let fallen = b.volatile(user, Volatile::SupremeOverlord);
            (fallen.active && fallen.counter > 0)
                .then(|| POW_MOD[usize::from(fallen.counter.min(5))])
        }
        _ => None,
    };
    if let Some(modifier) = boost {
        let p = priority(ability.data().event_orders, "onBasePowerPriority");
        out.push(Handler::of(b, user, p, SUB_ABILITY, modifier));
    }
    // `onAllyBasePower` of every active Pokémon on the user's side (`alliesAndSelf()`): Steely
    // Spirit (the holder's own moves too); Battery (special moves) and Power Spot (every move)
    // only for another Pokémon's move (`attacker !== this.effectState.target`). None is
    // breakable.
    for holder in b.alive_slots(user.side) {
        let ability = ability_for_move(b, holder, user, data);
        let modifier = match ability {
            a if a == abilities::STEELY_SPIRIT => {
                (move_type == Type::Steel).then_some(MOD_ONE_POINT_FIVE)
            }
            a if a == abilities::BATTERY => {
                (holder != user && data.category == MoveCategory::Special).then_some(5325)
            }
            a if a == abilities::POWER_SPOT => (holder != user).then_some(5325),
            _ => None,
        };
        if let Some(modifier) = modifier {
            let p = priority(ability.data().event_orders, "onAllyBasePowerPriority");
            out.push(Handler::of(b, holder, p, SUB_ABILITY, modifier));
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
/// `side` (`side.addSideCondition`), for every active Pokémon of that side when it is
/// Tailwind: Wind Power gets `charge`; Wind Rider (breakable: a Mold Breaker user's Tailwind
/// skips its allies' handler) `this.boost({atk: 1}, pokemon, pokemon)`. Each only changes its
/// holder.
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
        if b.ability_unless_broken(holder) == abilities::WIND_RIDER {
            wind_rider_boost(b, holder);
        }
    }
}

/// Wind Rider's `this.boost({atk: 1}, pokemon, pokemon)`; whether a stage changed.
pub(crate) fn wind_rider_boost<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> bool {
    let mut up = NO_BOOSTS;
    up[0] = 1;
    b.boost_by(
        holder,
        &up,
        Some(holder),
        BoostEffect::Ability(abilities::WIND_RIDER),
    )
}

/// `runEvent('HitProtect', attacker, defender, move)` in `checkMoveBypassesProtect`: the
/// attacker's `onHitProtect` — Unseen Fist (Champions: its `onModifyMove` is removed) and
/// Piercing Drill, neither breakable — returns `false` for a move with the `contact` flag (after
/// ModifyMove: Punching Glove removes it; Protective Pads do not matter), which lets it through
/// the protection and sets the target's `bypassProtect` (its damage is then quartered).
pub(crate) fn hit_protect<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> bool {
    [abilities::UNSEEN_FIST, abilities::PIERCING_DRILL].contains(&b.ability(user))
        && super::items::makes_contact(b, user, data)
}

/// The species' base species (`species.baseSpecies`; the dex leaves it unset on some bases).
fn base_species(species: SpeciesId) -> SpeciesId {
    let base = species.data().base_species;
    if base.is_none() {
        species
    } else {
        base
    }
}

/// Commander's `onUpdate`, which its `onStart` and `onAnySwitchIn` also run, for the holder in
/// `holder`:
/// - doubles only; nothing while a `runSwitch` is the next action (`queue.peek()`: between a
///   switch-in and its `runSwitch`, [`Battle::awaiting_run_switch`]);
/// - `ally = pokemon.allies()[0]` (the other active Pokémon not fainted); nothing if the holder
///   or the ally has a `switchFlag`;
/// - unless the holder is a Tatsugiri (`baseSpecies.baseSpecies`, Megas included) next to a
///   Dondozo, `commanding` ends (`removeVolatile`: no `onEnd`) — the ally fainted or is gone;
/// - otherwise, if it is not commanding yet and the ally is not commanded already, its queued
///   actions are cancelled (`queue.cancelAction`), it gets `commanding` and the ally gets
///   `commanded`, whose `onStart` raises Atk, Def, SpA, SpD and Spe by 2 (the boost's source is
///   the Tatsugiri: `addVolatile('commanded', pokemon)`). An ally that is `commanded` stays so
///   when its Tatsugiri faints.
pub(crate) fn commander_update<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) {
    use crate::dex::species;
    if N != 2 || b.awaiting_run_switch {
        return;
    }
    let Some(pokemon) = b.alive(holder) else {
        return;
    };
    let ally = b
        .alive_slots(holder.side)
        .into_iter()
        .find(|&s| s != holder);
    let flagged = |b: &Battle<'_, N>, slot: SlotRef| {
        b.state.slot(slot).switch_flag != crate::state::SwitchFlag::None
    };
    if flagged(b, holder) || ally.is_some_and(|a| flagged(b, a)) {
        return;
    }
    let commanding = b.volatile(holder, Volatile::Commanding).active;
    let tatsugiri = base_species(b.mon(pokemon).species) == species::TATSUGIRI;
    let dondozo = ally.filter(|&a| {
        b.slot_mon(a)
            .is_some_and(|m| base_species(m.species) == species::DONDOZO)
    });
    let (true, Some(ally)) = (tatsugiri, dondozo) else {
        if commanding {
            b.remove_volatile(holder, Volatile::Commanding);
        }
        return;
    };
    // `else { if (!ally.fainted) return; ... }`: the ally is never fainted here.
    if commanding || b.volatile(ally, Volatile::Commanded).active {
        return;
    }
    b.queue.retain(|action| action.pokemon != pokemon);
    b.add_volatile(holder, Volatile::Commanding);
    if b.add_volatile(ally, Volatile::Commanded) {
        b.boost_by(
            ally,
            &[2, 2, 2, 2, 2, 0, 0],
            Some(holder),
            BoostEffect::Ability(abilities::COMMANDER),
        );
    }
}

/// Whether the move `data` (of target type `target` after ModifyMove) that `user` uses skips
/// the `RedirectTarget` event (`move.tracksTarget`): Snipe Shot and Sky Drop by their data;
/// Stalwart and Propeller Tail (`onModifyMove`, priority 1, not breakable) set it to `move.target
/// !== 'scripted'` for every move of their holder, which also overrides the data. Their other
/// half, `getTarget` keeping the `originalTarget` of the action (only different after Ally
/// Switch), is refused in `handlers::swap_positions`.
pub(crate) fn tracks_target<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
    target: crate::dex::MoveTarget,
) -> bool {
    if tracks_original_target(b.ability(user)) {
        target != crate::dex::MoveTarget::Scripted
    } else {
        data.tracks_target
    }
}

/// Stalwart and Propeller Tail: Showdown's `getTarget` aims at the action's `originalTarget`
/// while it is active (`pokemon.hasAbility(['stalwart', 'propellertail'])`).
pub(crate) fn tracks_original_target(ability: AbilityId) -> bool {
    ability == abilities::STALWART || ability == abilities::PROPELLER_TAIL
}

/// Gorilla Tactics' `onModifyMove` for the move `id` its holder in `user` uses (any move but
/// Struggle, called and status moves included): `if (pokemon.abilityState.choiceLock) return;
/// pokemon.abilityState.choiceLock = move.id` ([`Volatile::GorillaTactics`]).
pub(crate) fn gorilla_modify_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) {
    if b.ability(user) != abilities::GORILLA_TACTICS
        || id == moves::STRUGGLE
        || b.volatile(user, Volatile::GorillaTactics).active
    {
        return;
    }
    let state = VolatileState {
        active: true,
        mv: id,
        ..VolatileState::NONE
    };
    b.set_volatile_state(user, Volatile::GorillaTactics, state);
}

/// Gorilla Tactics' `onBeforeMove` (priority 0): a move other than the locked one (and not
/// Struggle) fails, with no PP spent. `false` = the move is not used. As with the Choice
/// lock, only a lock set after the choice (never by a supported effect) reaches it: the other
/// moves cannot be chosen ([`gorilla_disabled_move`]).
pub(crate) fn gorilla_before_move<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let lock = b.volatile(user, Volatile::GorillaTactics);
    !(b.ability(user) == abilities::GORILLA_TACTICS
        && lock.active
        && lock.mv != id
        && id != moves::STRUGGLE)
}

/// Gorilla Tactics' `onDisableMove` (`endTurn`): while locked, every other move is disabled
/// (not hidden).
pub(crate) fn gorilla_disabled_move<const N: usize>(
    state: &State<N>,
    slot: SlotRef,
    id: MoveId,
) -> Option<String> {
    let mon = state.active(slot)?;
    let lock = state.slot(slot).volatiles.get(Volatile::GorillaTactics);
    let acts = effective_ability(state, slot) == abilities::GORILLA_TACTICS;
    (acts && lock.active && lock.mv != id).then(|| {
        format!(
            "{} is locked into {} by Gorilla Tactics",
            mon.species.data().name,
            lock.mv.data().name
        )
    })
}

/// Supreme Overlord's `onStart`: `if (pokemon.side.totalFainted)` the holder's
/// `abilityState.fallen = Math.min(pokemon.side.totalFainted, 5)`, kept as
/// [`Volatile::SupremeOverlord`] (the ability state is fresh at every switch-in and ability
/// change, as the volatile is). The count is fixed from then on: later faints only count at the
/// next start.
pub(crate) fn supreme_overlord_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let fallen = b.state.side(slot.side).history.total_fainted.min(5);
    if fallen == 0 || b.alive(slot).is_none() {
        return;
    }
    let state = VolatileState {
        active: true,
        counter: u16::from(fallen),
        ..VolatileState::NONE
    };
    b.set_volatile_state(slot, Volatile::SupremeOverlord, state);
}

/// `runEvent('AfterFaint', target, source, effect, length)` at the end of `faintMessages`
/// (unless the battle ended there), for the source of the last faint taken from the queue when
/// a move's damage caused it; `length` is how many faints were queued when the call began (a
/// spread move that knocks out both foes gives 2). The source's `onSourceAfterFaint`
/// (`effect.effectType === 'Move'`) runs only while it is active (an inactive holder ignores
/// its ability: fainted in the same batch, or switched out). Each boosts its holder by
/// `length` (`this.boost(..., source)`: the holder is its own source):
/// - Moxie, Chilling Neigh, As One (Glastrier; the boost's effect is Chilling Neigh): Attack;
/// - Grim Neigh, As One (Spectrier; Grim Neigh): Special Attack;
/// - Beast Boost, Eelevate: the stat of `getBestStat(true, true)`, the first of Atk, Def, SpA,
///   SpD, Spe with the highest stored stat (no stages, no modifiers, and Wonder Room only swaps
///   the stages it ignores). Eelevate is breakable, but the move whose damage caused the faint
///   is the holder's own, whose Mold Breaker does not suppress the holder's ability.
pub(crate) fn after_faint<const N: usize>(
    b: &mut Battle<'_, N>,
    source: PokemonRef,
    length: usize,
) {
    let Some(slot) = State::<N>::slot_refs().find(|&s| b.alive(s) == Some(source)) else {
        return;
    };
    let ability = b.ability(slot);
    let (stat, effect) = match ability {
        a if a == abilities::MOXIE || a == abilities::CHILLING_NEIGH => (0, a),
        a if a == abilities::AS_ONE_GLASTRIER => (0, abilities::CHILLING_NEIGH),
        a if a == abilities::GRIM_NEIGH => (2, a),
        a if a == abilities::AS_ONE_SPECTRIER => (2, abilities::GRIM_NEIGH),
        a if a == abilities::BEAST_BOOST || a == abilities::EELEVATE => {
            let stats = b.mon(source).stats;
            let mut best = 0;
            for stat in 1..stats.len() {
                if stats[stat] > stats[best] {
                    best = stat;
                }
            }
            (best, a)
        }
        _ => return,
    };
    let mut boosts = NO_BOOSTS;
    boosts[stat] = i8::try_from(length).unwrap_or(i8::MAX);
    b.boost_by(slot, &boosts, Some(slot), BoostEffect::Ability(effect));
}

/// Whether the engine can move `item` to another Pokémon (Symbiosis, Pickpocket, Magician): an
/// item with `Start` / `End` handlers only if Trick could move it (the Start on the new holder
/// is `moves::trick_item_start`), and not one whose other `TakeItem` handler is not
/// implemented.
pub(crate) fn item_moves(item: ItemId) -> bool {
    let data = item.data();
    let start_or_end = data
        .handlers
        .iter()
        .any(|h| ["onStart", "onEnd"].contains(h));
    let other_take_item = data.mega_stone.is_empty() && data.handlers.contains(&"onTakeItem");
    !other_take_item && (!start_or_end || super::moves::trick_moves_item(item))
}

/// Why a Pokémon on the field cannot be simulated because of Symbiosis: it holds an item it
/// could not pass ([`item_moves`]). The supported ways to give it another item (Trick,
/// Pickpocket, Magician) only move items that pass.
pub(crate) fn symbiosis_problem(mon: &Pokemon) -> Option<String> {
    (mon.ability == abilities::SYMBIOSIS && !mon.item.is_none() && !item_moves(mon.item)).then(
        || {
            format!(
                "{}: Symbiosis holding {} ({:?})",
                mon.species.data().name,
                mon.item.data().name,
                mon.item.data().handlers
            )
        },
    )
}

/// Symbiosis's `onAllyAfterUseItem` after the Pokémon in `receiver` used, ate or lost (Air
/// Balloon) its item (`runEvent('AfterUseItem')`; the holder's own use changes nothing, as it
/// then has no item to give):
/// - nothing if the receiver has a `switchFlag` (Eject Button sets it before using the item);
/// - for the active holder on the receiver's side (not breakable): `source.takeItem()` — the
///   item's own TakeItem handler must let the holder part with it (a Mega Stone of its species
///   stays); then its End on the holder (Mirror Herb forgets its copied raises);
/// - the item's TakeItem with the receiver and `pokemon.setItem(myItem)` (the receiver needs HP):
///   the receiver gets it and its Start runs (`moves::trick_item_start`), otherwise `source.item
///   = myItem.id` gives it back.
pub(crate) fn symbiosis<const N: usize>(b: &mut Battle<'_, N>, receiver: SlotRef) {
    use crate::instruction::Instruction;
    if b.occupant(receiver).is_none()
        || b.state.slot(receiver).switch_flag != crate::state::SwitchFlag::None
    {
        return;
    }
    let holders: Vec<SlotRef> = b
        .alive_slots(receiver.side)
        .into_iter()
        .filter(|&s| s != receiver && b.ability(s) == abilities::SYMBIOSIS)
        .collect();
    for holder in holders {
        let item = b.raw_item(holder);
        if item.is_none() || !b.item_can_be_taken(holder) {
            continue;
        }
        let giver = b.occupant(holder).expect("an active holder");
        b.apply(Instruction::SetItem {
            target: giver,
            old: item,
            new: ItemId::NONE,
        });
        if item == items::MIRROR_HERB {
            b.mirror_herb.retain(|&(p, _)| p != giver);
        }
        let taker = b.alive(receiver).filter(|&p| {
            let mon = b.mon(p);
            let base = base_species(mon.species);
            mon.item.is_none()
                && !item.data().cannot_be_taken
                && !item.data().mega_stone.iter().any(|&(from, _)| from == base)
        });
        let Some(taker) = taker else {
            b.apply(Instruction::SetItem {
                target: giver,
                old: ItemId::NONE,
                new: item,
            });
            continue;
        };
        b.apply(Instruction::SetItem {
            target: taker,
            old: ItemId::NONE,
            new: item,
        });
        super::moves::trick_item_start(b, receiver, item);
    }
}

/// Why a battle with an active Rivalry holder cannot be simulated: a Pokémon of either party
/// has an undecided gender ([`Gender::Random`]: no set gender, a species with a gender ratio;
/// Showdown drew it with the battle's PRNG), which Rivalry's `onBasePower` would read.
pub(crate) fn rivalry_problem<const N: usize>(state: &State<N>) -> Option<String> {
    let rivalry = State::<N>::slot_refs()
        .filter_map(|s| state.active(s))
        .any(|m| m.hp > 0 && m.ability == abilities::RIVALRY);
    if !rivalry {
        return None;
    }
    let undecided = state
        .sides
        .iter()
        .flat_map(|side| side.party.iter())
        .find(|m| !m.species.is_none() && m.gender == Gender::Random)?;
    Some(format!(
        "Rivalry next to {} of undecided gender (give the set a gender)",
        undecided.species.data().name
    ))
}

/// `victim.takeItem(thief)` then `thief.setItem(item)` (Pickpocket, Magician; the thief holds
/// nothing): the TakeItem event (the victim's Unburden, then the item's own handler: a Mega
/// Stone of the victim's species or Booster Energy on a Paradox Pokémon stays), the item's End
/// on the victim (Mirror Herb forgets its copied raises), then `setItem` on a thief with HP
/// (its Start: `moves::trick_item_start`); a thief without HP makes the victim take it back
/// (`victim.item = item.id`: no Start). Returns whether the thief got the item. An item the
/// engine cannot move ([`item_moves`]) is unsupported.
fn steal_item<const N: usize>(
    b: &mut Battle<'_, N>,
    victim: SlotRef,
    thief: SlotRef,
) -> Result<bool, super::TurnError> {
    use crate::instruction::Instruction;
    let (Some(holder), item) = (b.occupant(victim), b.raw_item(victim)) else {
        return Ok(false);
    };
    if item.is_none() {
        return Ok(false);
    }
    if !item_moves(item) {
        return Err(b.unsupported(format!(
            "an ability stealing {} ({:?})",
            item.data().name,
            item.data().handlers
        )));
    }
    if !b.take_item(victim) {
        return Ok(false);
    }
    if item == items::MIRROR_HERB {
        b.mirror_herb.retain(|&(p, _)| p != holder);
    }
    let Some(taker) = b.alive(thief) else {
        b.apply(Instruction::SetItem {
            target: holder,
            old: ItemId::NONE,
            new: item,
        });
        return Ok(false);
    };
    b.apply(Instruction::SetItem {
        target: taker,
        old: ItemId::NONE,
        new: item,
    });
    super::moves::trick_item_start(b, thief, item);
    Ok(true)
}

/// Showdown `speedSort` of `slots` by `pokemon.speed` ([`Battle::event_speed`]), fastest first:
/// equal Speeds are shuffled (uniformly) only when two of them are `relevant`, the only case in
/// which their order can change the outcome.
fn speed_sorted<const N: usize>(
    b: &mut Battle<'_, N>,
    mut slots: Vec<SlotRef>,
    relevant: impl Fn(&Battle<'_, N>, SlotRef) -> bool,
) -> Vec<SlotRef> {
    slots.sort_by_key(|&s| std::cmp::Reverse(b.event_speed(s)));
    let mut start = 0;
    while start < slots.len() {
        let speed = b.event_speed(slots[start]);
        let end = (start..slots.len())
            .find(|&i| b.event_speed(slots[i]) != speed)
            .unwrap_or(slots.len());
        let count = (start..end).filter(|&i| relevant(b, slots[i])).count();
        if count >= 2 {
            for k in start..end - 1 {
                let pick = k + b.rng.uniform(end - k);
                slots.swap(k, pick);
            }
        }
        start = end;
    }
    slots
}

/// Pickpocket's `onAfterMoveSecondary` on the Pokémon in `target`, hit by `user`'s move `data`:
/// a move with the `contact` flag (after ModifyMove: Punching Glove removes it; Protective Pads
/// do not matter) from another Pokémon, a holder with no item (`target.item`, suppressed or
/// not) that is neither switching out nor being dragged out, and a user without an Emergency
/// Exit / Eject Button switch (`source.switchFlag === true`; a U-turn's does not count): the
/// holder steals the user's item ([`steal_item`]).
pub(crate) fn pickpocket<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
) -> Result<(), super::TurnError> {
    use crate::state::SwitchFlag;
    if target == user
        || b.ability(target) != abilities::PICKPOCKET
        || !super::items::makes_contact(b, user, data)
        || !b.raw_item(target).is_none()
        || b.state.slot(target).switch_flag != SwitchFlag::None
        || b.force_switch.contains(&target)
        || b.state.slot(user).switch_flag == SwitchFlag::Effect
    {
        return Ok(());
    }
    steal_item(b, user, target)?;
    Ok(())
}

/// Magician's `onAfterMoveSecondarySelf` for the user in `user` of the damaging move `id`
/// (after the item's handlers, which cannot act: Magician needs the user to hold nothing): the
/// user steals the item of the first of `hit_targets` (the move's `hitTargets`, a substitute's
/// owner included; itself excluded) in Speed order that gives one ([`steal_item`]). Nothing
/// for a user with an Emergency Exit / Eject Button switch or holding an item (gems and Fling are
/// refused).
pub(crate) fn magician<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    hit_targets: Vec<SlotRef>,
) -> Result<(), super::TurnError> {
    use crate::state::SwitchFlag;
    if b.ability(user) != abilities::MAGICIAN
        || b.state.slot(user).switch_flag == SwitchFlag::Effect
        || !b.raw_item(user).is_none()
        || id.data().category == MoveCategory::Status
    {
        return Ok(());
    }
    let targets: Vec<SlotRef> = hit_targets.into_iter().filter(|&t| t != user).collect();
    let order = speed_sorted(b, targets, |b, s| {
        !b.raw_item(s).is_none() && b.item_can_be_taken(s)
    });
    for victim in order {
        if steal_item(b, victim, user)? {
            break;
        }
    }
    Ok(())
}

/// Showdown `battle.skillSwap(source, target)` (Wandering Spirit's `onDamagingHit`, with the
/// attacker as `source`): nothing if either has fainted (processed: both are still in their
/// slots at DamagingHit, even at 0 HP), either ability is `failskillswap`, or `SetAbility`
/// fails for either (Ability Shield); otherwise each ability's End (`switching::end_ability`),
/// the two trade places (the base abilities stay: they come back on switching out), and each
/// new one starts (`switching::start_ability`: the target's first).
pub(crate) fn skill_swap<const N: usize>(
    b: &mut Battle<'_, N>,
    source: SlotRef,
    target: SlotRef,
) -> Result<(), super::TurnError> {
    use crate::instruction::Instruction;
    let (Some(source_pokemon), Some(target_pokemon)) = (b.occupant(source), b.occupant(target))
    else {
        return Ok(());
    };
    // `source.getAbility()` / `target.getAbility()`: the raw abilities.
    let (source_ability, target_ability) = (b.raw_ability(source), b.raw_ability(target));
    let fails = |a: AbilityId| a.data().flags.contains(AbilityFlags::FAILSKILLSWAP);
    if fails(source_ability) || fails(target_ability) {
        return Ok(());
    }
    // `runEvent('SetAbility')` on the target, then on the source: Ability Shield returns `null`.
    if b.item(target) == items::ABILITY_SHIELD || b.item(source) == items::ABILITY_SHIELD {
        return Ok(());
    }
    // What the state is otherwise checked for before a turn: Symbiosis must be able to pass the
    // item its new holder has, Rivalry needs every gender decided.
    for (ability, holder) in [(source_ability, target), (target_ability, source)] {
        let mut mon = b.slot_mon(holder).expect("an occupant").clone();
        mon.ability = ability;
        let why = symbiosis_problem(&mon).or_else(|| {
            let undecided = b
                .state
                .sides
                .iter()
                .flat_map(|side| side.party.iter())
                .any(|m| !m.species.is_none() && m.gender == Gender::Random);
            (ability == abilities::RIVALRY && undecided)
                .then(|| "Rivalry next to a Pokémon of undecided gender".to_owned())
        });
        if let Some(why) = why {
            return Err(b.unsupported(format!("Wandering Spirit's swap: {why}")));
        }
    }
    super::switching::end_ability(b, source, source_ability)?;
    super::switching::end_ability(b, target, target_ability)?;
    b.apply(Instruction::SetAbility {
        target: source_pokemon,
        old: source_ability,
        new: target_ability,
    });
    b.apply(Instruction::SetAbility {
        target: target_pokemon,
        old: target_ability,
        new: source_ability,
    });
    super::switching::start_ability(b, source, target_ability)?;
    super::switching::start_ability(b, target, source_ability)?;
    Ok(())
}

/// Color Change's `onAfterMoveSecondary` for the Pokémon in `target`, hit by a damaging move of
/// type `move_type` (after ModifyType): a holder with HP that is not already of that type
/// becomes that type alone (`setType(type)`: not for `???`, nor on Arceus or Silvally; through
/// Roost's filter, `moves::set_types`). Only its holder changes.
pub(crate) fn color_change<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    move_type: Type,
    category: MoveCategory,
) {
    let Some(pokemon) = b.alive(target) else {
        return;
    };
    if b.ability(target) != abilities::COLOR_CHANGE
        || category == MoveCategory::Status
        || matches!(move_type, Type::None | Type::Unknown)
        || b.has_type(target, move_type)
        || [493, 773].contains(&b.mon(pokemon).species.data().num)
    {
        return;
    }
    super::moves::set_types(b, target, [move_type, Type::None]);
}

/// Harvest's `onResidual` for its holder: in harsh sunlight (`this.field.isWeather`: the field's
/// effective weather) or on `this.randomChance(1, 2)`, a holder with HP, no item and a berry as
/// `lastItem` gets it back (`setItem(lastItem)`: berries have no Start) and forgets it
/// (`lastItem = ''`). The chance is not drawn when nothing could be restored (the outcome is
/// the same either way).
pub(crate) fn harvest<const N: usize>(b: &mut Battle<'_, N>, pokemon: PokemonRef) {
    use crate::instruction::Instruction;
    let mon = b.mon(pokemon);
    let (item, last) = (mon.item, mon.last_item);
    if mon.hp <= 0 || !item.is_none() || last.is_none() || !last.data().is_berry {
        return;
    }
    let sun = b.effective_weather() == Weather::Sun;
    if !sun && !b.rng.chance(1, 2) {
        return;
    }
    b.apply(Instruction::SetItem {
        target: pokemon,
        old: ItemId::NONE,
        new: last,
    });
    b.apply(Instruction::SetLastItem {
        target: pokemon,
        old: last,
        new: ItemId::NONE,
    });
}

/// Soul-Heart's `onAnyFaint` for one processed faint (`runEvent('Faint')` in `faintMessages`):
/// every active holder not at 0 HP raises its SpA by 1 (`this.boost({spa: 1},
/// this.effectState.target)`; `boost` does nothing at 0 HP, and fails once the holder's foes have
/// no Pokémon left). Each only changes its holder.
pub(crate) fn soul_heart<const N: usize>(b: &mut Battle<'_, N>, holders: &[SlotRef]) {
    for &slot in holders {
        if b.alive(slot).is_some() && b.raw_ability(slot) == abilities::SOUL_HEART {
            let mut up = NO_BOOSTS;
            up[2] = 1;
            b.boost_by(slot, &up, None, BoostEffect::Ability(abilities::SOUL_HEART));
        }
    }
}

/// The active Soul-Heart holders whose `onAnyFaint` acts ([`soul_heart`]), taken while the
/// fainting Pokémon is still in its slot (a fainting Neutralizing Gas holder still suppresses
/// them in the Faint event).
pub(crate) fn soul_heart_holders<const N: usize>(b: &Battle<'_, N>) -> Vec<SlotRef> {
    b.all_alive()
        .into_iter()
        .filter(|&s| b.ability(s) == abilities::SOUL_HEART)
        .collect()
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
/// its holder, so their order across Pokémon does not matter. Commander's (not breakable,
/// [`commander_update`]) changes its holder and its Dondozo ally only, and no other Update
/// listener reads what it changes.
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
    // Oblivious: `if (pokemon.volatiles['attract']) pokemon.removeVolatile('attract')` (its
    // immunity keeps it from getting one; a move that ignores it could).
    if ability == abilities::OBLIVIOUS && b.volatile(slot, Volatile::Attract).active {
        b.remove_volatile(slot, Volatile::Attract);
    }
    if ability == abilities::COMMANDER {
        commander_update(b, slot);
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
            // `pokemon.heal`: no TryHeal, so Heal Block does not stop it.
            b.heal_unblocked(slot, max_hp / 3.0);
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
/// berry once it has started (`effectState.unnerved`, set by its start): a holder that has
/// switched in but whose `runSwitch` has not run yet ([`Battle::unstarted`]: the Update between
/// a switch and its `runSwitch`) does not block, so a foe's pending berry is eaten while one
/// Unnerve holder replaces another. They only return booleans, so their order does not matter.
/// `false` = the berry is not eaten.
pub(crate) fn try_eat_item<const N: usize>(b: &Battle<'_, N>, eater: SlotRef) -> bool {
    let Some(mon) = b.slot_mon(eater) else {
        return false;
    };
    let item = mon.item;
    let pending = has_berserk_check(b.ability(eater))
        && b.volatile(eater, Volatile::AngerShellUnchecked).active
        && HEALING_BERRIES.contains(&item);
    let unnerved = b.alive_slots(eater.side.other()).into_iter().any(|foe| {
        [
            abilities::UNNERVE,
            abilities::AS_ONE_GLASTRIER,
            abilities::AS_ONE_SPECTRIER,
        ]
        .contains(&b.ability(foe))
            && !b.occupant(foe).is_some_and(|p| b.unstarted.contains(&p))
    });
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
    match b.event_speed(holder).cmp(&b.event_speed(target)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b.rng.uniform(2) == 0,
    }
}

/// Whether the active Pokémon in `slot` is trapped, so it cannot choose to switch: Showdown's
/// `pokemon.trapped` as `endTurn` sets it before the choices (`runEvent('TrapPokemon')`). The
/// state between turns is the state `endTurn` saw (after the replacements), so it is derived
/// here instead of stored. Implemented handlers:
/// - the conditions' `onTrapPokemon` (`conditions::trapped`): No Retreat, Mean Look / Block /
///   Spider Web (`trapped`), Ingrain, partial trapping while its source is active;
/// - the foes' `onFoeTrapPokemon` (every foe not at 0 HP is adjacent in singles and doubles):
///   Shadow Tag traps a Pokémon without Shadow Tag, Arena Trap a grounded one, Magnet Pull a
///   Steel type, each through `tryTrap`, which the Ghost type's `trapped` immunity stops (no
///   `Immunity` handler covers `trapped`);
/// - Shed Shell's `onTrapPokemon` (priority -10, after every other): `pokemon.trapped = false`,
///   unless the item is suppressed (Magic Room, Klutz: `ignoringItem` skips the handler).
///
/// `onFoeMaybeTrapPokemon` only sets the `maybeTrapped` display flag. Fairy Lock, Jaw Lock,
/// Octolock and the trapping moves Anchor Shot, Spirit Shackle and Thousand Waves are not
/// implemented. A switch the move request forbids is rejected by
/// `Ruleset::validate_slot_action` (`ActionError::Trapped`), so `Ruleset::joint_actions` never
/// generates it; forced switches (replacements) ignore it.
pub fn trapped<const N: usize>(state: &State<N>, slot: SlotRef) -> bool {
    // The conditions' `TrapPokemon` handlers (No Retreat, partial trapping).
    if super::conditions::trapped(state, slot).is_some() {
        return true;
    }
    // The foes' abilities as they act (a suppressed Shadow Tag traps nobody).
    let foe_traps = State::<N>::slot_refs().any(|s| {
        s.side != slot.side
            && state.active(s).is_some_and(|m| m.hp > 0)
            && [
                abilities::SHADOW_TAG,
                abilities::ARENA_TRAP,
                abilities::MAGNET_PULL,
            ]
            .contains(&effective_ability(state, s))
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
                a if a == abilities::SHADOW_TAG => b.ability(slot) != abilities::SHADOW_TAG,
                a if a == abilities::ARENA_TRAP => b.is_grounded(slot),
                a if a == abilities::MAGNET_PULL => mon.types.contains(&Type::Steel),
                _ => false,
            });
    // The effective item: a suppressed Shed Shell's handler does not run.
    trapped && b.item(slot) != items::SHED_SHELL
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

/// The abilities whose `onWeatherChange` a weather suppressor's `onEnd` would run: Protosynthesis
/// and Flower Gift (Ice Face ignores a suppressor's event).
pub(crate) fn reacts_to_suppressor_end(ability: AbilityId) -> bool {
    ability == abilities::PROTOSYNTHESIS || ability == abilities::FLOWER_GIFT
}

/// Why a Protosynthesis or Flower Gift holder and a weather-suppressing ability (Air Lock, Cloud
/// Nine) cannot be on the field together: the suppressor leaving (fainting, switching out) runs
/// its `onEnd` `WeatherChange`, which would start Protosynthesis or change Cherrim's forme in
/// sun, and the engine does not run an ability's `End` there.
pub(crate) fn paradox_suppressor_problem<const N: usize>(state: &State<N>) -> Option<String> {
    let actives: Vec<&Pokemon> = State::<N>::slot_refs()
        .filter_map(|s| state.active(s))
        .filter(|m| m.hp > 0)
        .collect();
    let paradox = actives.iter().any(|m| reacts_to_suppressor_end(m.ability));
    let suppressor = actives.iter().any(|m| m.ability.data().suppress_weather);
    (paradox && suppressor).then(|| {
        "Protosynthesis / Flower Gift next to Air Lock / Cloud Nine (the suppressor's End \
         WeatherChange)"
            .into()
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
    // The user's own ability as it acts (Gastro Acid, Neutralizing Gas).
    let ability = b.ability(user);
    // Water Bubble: the user's Water moves `chainModify(2)` (no priority).
    if ability == abilities::WATER_BUBBLE && move_type == Type::Water {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_DOUBLE));
    }
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
    if let Some(modifier) =
        own_attack_modifier(b, user, target, attacker, ability, physical, move_type)
    {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, modifier));
    }
    // Slow Start (priority 5): `if (this.effectState.counter) return this.chainModify(0.5)`
    // (Attack only).
    if physical && slow_start_halves(b, user) {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_HALF));
    }
    // Gorilla Tactics (priority 1): `chainModify(1.5)` (Attack only; Dynamax is off).
    if physical && ability == abilities::GORILLA_TACTICS {
        let p = priority(ability.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
    }
    // Flower Gift's `onAllyModifyAtk` (priority 3): 1.5x for each Cherrim on the user's side in
    // the user's sun (Attack only).
    if physical {
        let orders = abilities::FLOWER_GIFT.data().event_orders;
        let p = priority(orders, "onAllyModifyAtkPriority");
        for holder in super::forme::flower_gift_holders(b, user, user, data) {
            out.push(Handler::of(b, holder, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
        }
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
    // Tablets of Ruin (`onAnyModifyAtk`) / Vessel of Ruin (`onAnyModifySpA`).
    let (ruin, any_event) = if physical {
        (abilities::TABLETS_OF_RUIN, "onAnyModifyAtkPriority")
    } else {
        (abilities::VESSEL_OF_RUIN, "onAnyModifySpAPriority")
    };
    out.extend(ruin_handler(b, ruin, user, any_event));
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

/// The user's own `onModifyAtk` / `onModifySpA` (WORKPLAN O-Q unit 1), all at priority 5 and
/// none breakable (they are the user's):
/// - Huge Power, Pure Power: `chainModify(2)` (Attack only).
/// - Steelworker, Dragon's Maw, Rocky Payload, Fire Mane: `chainModify(1.5)` for a Steel /
///   Dragon / Rock / Fire move (`move.type`, after ModifyType); Transistor `[5325, 4096]` for an
///   Electric move.
/// - Defeatist: `pokemon.hp <= pokemon.maxhp / 2` halves.
/// - Stakeout: `!defender.activeTurns` doubles. `activeTurns` is 0 from the switch-in to the
///   next `endTurn`, exactly while `newlySwitched` is set (both reset on switching in and
///   cleared together in `endTurn`, the battle start's included), so the slot history's
///   `newly_switched` is read.
/// - Plus, Minus (Special Attack only): an ally (`pokemon.allies()`: not the holder, not at
///   0 HP) with Plus or Minus gives `chainModify(1.5)`.
fn own_attack_modifier<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    attacker: &Pokemon,
    ability: AbilityId,
    physical: bool,
    move_type: Type,
) -> Option<u32> {
    let typed = |ty: Type, modifier: u32| (move_type == ty).then_some(modifier);
    match ability {
        a if a == abilities::HUGE_POWER || a == abilities::PURE_POWER => {
            physical.then_some(MOD_DOUBLE)
        }
        a if a == abilities::STEELWORKER => typed(Type::Steel, MOD_ONE_POINT_FIVE),
        a if a == abilities::TRANSISTOR => typed(Type::Electric, 5325),
        a if a == abilities::DRAGONS_MAW => typed(Type::Dragon, MOD_ONE_POINT_FIVE),
        a if a == abilities::ROCKY_PAYLOAD => typed(Type::Rock, MOD_ONE_POINT_FIVE),
        a if a == abilities::FIRE_MANE => typed(Type::Fire, MOD_ONE_POINT_FIVE),
        a if a == abilities::DEFEATIST => {
            (2 * i32::from(attacker.hp) <= i32::from(attacker.max_hp)).then_some(MOD_HALF)
        }
        a if a == abilities::STAKEOUT => b
            .state
            .slot(target)
            .history
            .newly_switched
            .then_some(MOD_DOUBLE),
        a if a == abilities::PLUS || a == abilities::MINUS => {
            let partner = b.alive_slots(user.side).into_iter().any(|ally| {
                ally != user && [abilities::PLUS, abilities::MINUS].contains(&b.ability(ally))
            });
            (!physical && partner).then_some(MOD_ONE_POINT_FIVE)
        }
        _ => None,
    }
}

/// The paradox ability whose condition the Pokémon in `slot` has, with the condition's best
/// stat (0 Atk, 1 Def, 2 SpA, 3 SpD, 4 Spe; the condition's handlers are in the ability's data).
pub(crate) fn paradox_volatile_of<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
) -> Option<(AbilityId, u16)> {
    // `if (this.effectState.bestStat !== ... || pokemon.ignoringAbility()) return;`
    if ignoring_ability(b.state, slot) {
        return None;
    }
    let proto = b.volatile(slot, Volatile::Protosynthesis);
    if proto.active {
        return Some((abilities::PROTOSYNTHESIS, proto.counter));
    }
    let quark = b.volatile(slot, Volatile::QuarkDrive);
    quark
        .active
        .then_some((abilities::QUARK_DRIVE, quark.counter))
}

/// A Ruin ability's `onAny<Stat>` handler for the Modify event of the stat it lowers
/// (Tablets: ModifyAtk, Vessel: ModifySpA, Sword: ModifyDef, Beads: ModifySpD). `stat_holder`
/// is the Pokémon whose stat the event modifies (the user for Atk / SpA, the target for Def /
/// SpD); the handler is collected from every active Pokémon not at 0 HP (`alliesAndSelf()` and
/// `foes()` of it) and is not breakable.
///
/// Nothing when `stat_holder` has the same ability (`source.hasAbility(...)` /
/// `target.hasAbility(...)`). Otherwise every holder's handler runs, but only the first one in
/// the event's order chains 0.75: it stores itself in `move.ruinedAtk` (etc.) and the others
/// return. The handlers share a priority, so the first is the fastest holder (holders with the
/// same Speed sit at the same place in the order): one handler at that holder's Speed.
///
/// The stored holder lasts for the whole move (Tablets / Vessel keep it; Sword / Beads replace
/// it only once it no longer has the ability), so a later target or hit of the same move could
/// in principle keep a holder that has meanwhile become slower than another holder; that only
/// changes the result with two holders of the same Ruin ability whose Speed order flips between
/// hits of one multi-hit move while three or more other factors chain, and is not modelled.
fn ruin_handler<const N: usize>(
    b: &Battle<'_, N>,
    ruin: AbilityId,
    stat_holder: SlotRef,
    priority_name: &str,
) -> Option<Handler> {
    if b.ability(stat_holder) == ruin {
        return None;
    }
    let holder = b
        .all_alive()
        .into_iter()
        .filter(|&s| b.ability(s) == ruin)
        .max_by_key(|&s| b.action_speed(s))?;
    let p = priority(ruin.data().event_orders, priority_name);
    Some(Handler::of(b, holder, p, SUB_ABILITY, MOD_THREE_QUARTERS))
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
    // Fur Coat (`chainModify(2)`) and Grass Pelt (`if (this.field.isTerrain('grassyterrain'))
    // return this.chainModify(1.5)`): `onModifyDef`, priority 6, both breakable.
    let coat = match ability {
        a if a == abilities::FUR_COAT => Some(MOD_DOUBLE),
        a if a == abilities::GRASS_PELT => {
            (b.terrain() == crate::field::Terrain::Grassy).then_some(MOD_ONE_POINT_FIVE)
        }
        _ => None,
    };
    if let Some(modifier) = coat.filter(|_| defense_stat == Stat::Def) {
        let p = priority(ability.data().event_orders, "onModifyDefPriority");
        out.push(Handler::of(b, target, p, SUB_ABILITY, modifier));
    }
    // Flower Gift's `onAllyModifySpD` (priority 4): 1.5x for each Cherrim on the target's side
    // in the target's sun.
    if defense_stat == Stat::Spd {
        let orders = abilities::FLOWER_GIFT.data().event_orders;
        let p = priority(orders, "onAllyModifySpDPriority");
        for holder in super::forme::flower_gift_holders(b, target, user, data) {
            out.push(Handler::of(b, holder, p, SUB_ABILITY, MOD_ONE_POINT_FIVE));
        }
    }
    // Sword of Ruin (`onAnyModifyDef`) / Beads of Ruin (`onAnyModifySpD`), by the stat the
    // move targets (Psyshock meets Sword of Ruin).
    let (ruin, any_event) = if defense_stat == Stat::Def {
        (abilities::SWORD_OF_RUIN, "onAnyModifyDefPriority")
    } else {
        (abilities::BEADS_OF_RUIN, "onAnyModifySpDPriority")
    };
    out.extend(ruin_handler(b, ruin, target, any_event));
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
/// (`!pokemon.hasAbility('guts')`: `ability` is the user's as it acts).
pub(crate) fn burn_halves(attacker: &Pokemon, ability: AbilityId, data: &MoveData) -> bool {
    attacker.status == Status::Burn
        && data.category == MoveCategory::Physical
        && ability != abilities::GUTS
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

/// The user's ability `onModifyCritRatio` as an addition to the move's crit ratio (the result
/// is clamped to 0..=4): Super Luck `critRatio + 1`; Merciless `return 5` against a poisoned or
/// badly poisoned target, which after the clamp is a sure critical hit whatever else adds to it.
pub(crate) fn crit_ratio_bonus<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
) -> i32 {
    match b.ability(user) {
        a if a == abilities::SUPER_LUCK => 1,
        a if a == abilities::MERCILESS => {
            let poisoned = b
                .slot_mon(target)
                .is_some_and(|m| matches!(m.status, Status::Poison | Status::Toxic));
            if poisoned {
                5
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// `ModifyAccuracy` handlers of abilities (moves with a numeric accuracy): the user's
/// `onSourceModifyAccuracy`, the target's `onModifyAccuracy` (all breakable) and every active
/// Pokémon's `onAnyModifyAccuracy`.
pub(crate) fn accuracy_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let ability = b.ability(user);
    let source_modifier = match ability {
        // Hustle: physical moves 3277/4096.
        a if a == abilities::HUSTLE => (data.category == MoveCategory::Physical).then_some(3277),
        // Compound Eyes: 5325/4096.
        a if a == abilities::COMPOUND_EYES => Some(5325),
        _ => None,
    };
    if let Some(modifier) = source_modifier {
        let p = priority(
            ability.data().event_orders,
            "onSourceModifyAccuracyPriority",
        );
        out.push(Handler::of(b, user, p, SUB_ABILITY, modifier));
    }
    // The target's (priority -1): Sand Veil in sand and Snow Cloak in snow 3277/4096
    // (`this.field.isWeather(...)`: the field's effective weather), Tangled Feet 0.5 while the
    // target is confused. Wonder Skin replaces the accuracy instead ([`accuracy_direct`]).
    let defending = ability_for_move(b, target, user, data);
    let weather = b.effective_weather();
    let target_modifier = match defending {
        a if a == abilities::SAND_VEIL => (weather == Weather::Sand).then_some(3277),
        a if a == abilities::SNOW_CLOAK => (weather == Weather::Snow).then_some(3277),
        a if a == abilities::TANGLED_FEET => b
            .volatile(target, Volatile::Confusion)
            .active
            .then_some(MOD_HALF),
        _ => None,
    };
    if let Some(modifier) = target_modifier {
        let p = priority(defending.data().event_orders, "onModifyAccuracyPriority");
        out.push(Handler::of(b, target, p, SUB_ABILITY, modifier));
    }
    // Victory Star (`onAnyModifyAccuracy`, priority -1, not breakable): 4506/4096 for a move
    // of its holder or the holder's ally (`source.isAlly(holder)`), once per holder.
    for holder in b.alive_slots(user.side) {
        let star = b.ability(holder);
        if star == abilities::VICTORY_STAR {
            let p = priority(star.data().event_orders, "onAnyModifyAccuracyPriority");
            out.push(Handler::of(b, holder, p, SUB_ABILITY, 4506));
        }
    }
    out
}

/// The accuracy the `ModifyAccuracy` event starts chaining from: Wonder Skin (the target's,
/// breakable, priority 10) `return 50` for a status move with a numeric accuracy, which
/// replaces the accuracy; the chained factors apply to the result at the end of the event.
pub(crate) fn accuracy_direct<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    accuracy: i32,
) -> i32 {
    let wonder_skin = ability_for_move(b, target, user, data) == abilities::WONDER_SKIN;
    if wonder_skin && data.category == MoveCategory::Status {
        50
    } else {
        accuracy
    }
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

/// `ModifyDamage` handlers of abilities: the user's `onModifyDamage`, the target's
/// `onSourceModifyDamage` and every active Pokémon's `onAnyModifyDamage`. `type_mod` is the
/// hit's clamped effectiveness exponent (`getMoveHitData(move).typeMod`), `critical` whether it
/// is a critical hit (`getMoveHitData(move).crit`); `move_type` is the type of the move being
/// used.
pub(crate) fn modify_damage_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    move_type: Type,
    type_mod: i32,
    critical: bool,
) -> Vec<Handler> {
    let mut out = Vec::new();
    // The user's own (priority 0): Sniper 1.5x on a critical hit, Tinted Lens 2x on a resisted
    // hit (`typeMod < 0`), Neuroforce `[5120, 4096]` on a super-effective one.
    let own = b.ability(user);
    let own_modifier = match own {
        a if a == abilities::SNIPER => critical.then_some(MOD_ONE_POINT_FIVE),
        a if a == abilities::TINTED_LENS => (type_mod < 0).then_some(MOD_DOUBLE),
        a if a == abilities::NEUROFORCE => (type_mod > 0).then_some(5120),
        _ => None,
    };
    if let Some(modifier) = own_modifier {
        let p = priority(own.data().event_orders, "onModifyDamagePriority");
        out.push(Handler::of(b, user, p, SUB_ABILITY, modifier));
    }
    let Some(defender) = b.slot_mon(target) else {
        return out;
    };
    // `move.flags['contact']` after ModifyMove (Punching Glove).
    let contact = super::items::makes_contact(b, user, data);
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
