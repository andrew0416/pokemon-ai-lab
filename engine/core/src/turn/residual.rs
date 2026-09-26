//! End of turn: Showdown's `residual` action (`fieldEvent('Residual')`), then
//! `checkFainted` and `endTurn`.
//!
//! Handlers are collected once, sorted like Showdown's `comparePriority` (order ascending,
//! priority descending, Speed descending, sub-order ascending; ties shuffled), and run in
//! turn. A handler whose effect has a duration decrements it first; one that reaches 0 ends
//! the effect instead of running. Handlers of fainted Pokémon and of effects that ended
//! earlier in the residual are skipped. Faints are processed after every handler.

use crate::dex::{abilities, items, AbilityId, ItemId, Type, TypeImmunities, NO_BOOSTS};
use crate::field::{Effect, FieldEffect, SideEffect, SlotCondition, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, SlotRef, State, Status};
use crate::volatile::{Volatile, VolatileState};

use super::abilities as ability_events;
use super::abilities::SUB_SLOT_CONDITION;
use super::battle::{Battle, BoostEffect, DamageSource};
use super::conditions;
use super::items as item_events;
use super::order::ORDER_DEFAULT;
use super::TurnError;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// Weather duration, then its `onFieldResidual` (sandstorm damage).
    Weather,
    FieldDuration(FieldEffect),
    SideDuration(SideId, SideEffect),
    VolatileDuration(PokemonRef, SlotRef, Volatile),
    StatusDamage(PokemonRef, SlotRef),
    GrassyHeal(PokemonRef, SlotRef),
    Leftovers(PokemonRef, SlotRef),
    /// Speed Boost `onResidual` (order 28, sub-order 2).
    SpeedBoost(PokemonRef, SlotRef),
    /// Shed Skin, Hydration and Healer `onResidual` (order 5, sub-order 3).
    StatusCure(PokemonRef, SlotRef, AbilityId),
    /// An item's `onResidual` (`items::on_residual`).
    Item(PokemonRef, SlotRef, ItemId),
    /// Leech Seed's `onResidual` (order 8; no duration).
    LeechSeed(PokemonRef, SlotRef),
    /// The `onResidual` of a volatile without a duration: Aqua Ring (order 6), Ingrain (7),
    /// Nightmare (11), Curse (12), Salt Cure (13), Octolock (14).
    VolatileEffect(PokemonRef, SlotRef, Volatile),
    /// A slot condition's `onResidual` (future moves order 3, Wish 4; Revival Blessing's
    /// duration), slot-condition sub-order 3. Showdown collects it for the Pokémon in the
    /// position (`side.active`), and runs a slot condition's handler even when that Pokémon has
    /// fainted (`if (!handler.state?.isSlotCondition) continue;`), so it runs for a position
    /// held by a fainted Pokémon not yet replaced too.
    SlotCondition(SlotRef, SlotCondition),
    /// A forme ability's `onResidual` (order 29, ability sub-order): Schooling, Shields Down,
    /// Hunger Switch (`forme::residual`).
    Forme(PokemonRef, SlotRef, AbilityId),
    /// Harvest `onResidual` (order 28, sub-order 2).
    Harvest(PokemonRef, SlotRef),
    /// Another ability's `onResidual` (`abilities::on_residual`: Slow Start), at its own order.
    Ability(PokemonRef, SlotRef, AbilityId),
}

#[derive(Clone, Copy, Debug)]
struct Handler {
    order: u32,
    speed: i32,
    sub_order: u32,
    kind: Kind,
}

impl Handler {
    fn key(&self) -> (u32, i32, u32) {
        (self.order, -self.speed, self.sub_order)
    }
}

/// Showdown's effect-type sub-orders (`resolvePriority`).
const SUB_CONDITION: u32 = 2;
const SUB_SIDE_CONDITION: u32 = 4;
const SUB_FIELD_CONDITION: u32 = 5;
const SUB_WEATHER: u32 = 5;
const SUB_STATUS: u32 = 0;

pub(crate) fn residual<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    let mut handlers = collect(b);
    speed_sort(b, &mut handlers);
    for handler in handlers {
        // Showdown `fieldEvent`: `faintMessages()` follows every handler except one whose
        // holder has already fainted (skipped) or whose effect's duration ran out (`End`, then
        // `continue`): a faint queued by an `End` (Perish Song) waits for the next handler.
        if run(b, handler)? && b.faint_messages(true)? {
            return Ok(());
        }
    }
    // `runAction`'s `faintMessages()` after the residual action.
    b.faint_messages(true)?;
    Ok(())
}

fn collect<const N: usize>(b: &Battle<'_, N>) -> Vec<Handler> {
    let mut out = Vec::new();
    let field = |order, sub_order, kind| Handler {
        order,
        speed: 0,
        sub_order,
        kind,
    };
    if b.weather() != Weather::None {
        out.push(field(1, SUB_WEATHER, Kind::Weather));
    }
    if b.terrain() != Terrain::None {
        out.push(field(27, 7, Kind::FieldDuration(FieldEffect::Terrain)));
    }
    if b.field_active(FieldEffect::Gravity) {
        out.push(field(27, 2, Kind::FieldDuration(FieldEffect::Gravity)));
    }
    if b.field_active(FieldEffect::TrickRoom) {
        out.push(field(27, 1, Kind::FieldDuration(FieldEffect::TrickRoom)));
    }
    if b.field_active(FieldEffect::WonderRoom) {
        out.push(field(27, 5, Kind::FieldDuration(FieldEffect::WonderRoom)));
    }
    if b.field_active(FieldEffect::MagicRoom) {
        out.push(field(27, 6, Kind::FieldDuration(FieldEffect::MagicRoom)));
    }
    // Fairy Lock: no `onFieldResidualOrder` (Showdown's default order, the field-condition
    // sub-order); it ends silently.
    if b.field_active(FieldEffect::FairyLock) {
        out.push(field(
            ORDER_DEFAULT,
            SUB_FIELD_CONDITION,
            Kind::FieldDuration(FieldEffect::FairyLock),
        ));
    }
    for side in [SideId::One, SideId::Two] {
        for (effect, order, sub_order) in [
            (SideEffect::Reflect, 26, 1),
            (SideEffect::LightScreen, 26, 2),
            (SideEffect::Safeguard, 26, 3),
            (SideEffect::Mist, 26, 4),
            (SideEffect::Tailwind, 26, 5),
            (SideEffect::LuckyChant, 26, 6),
            (SideEffect::AuroraVeil, 26, 10),
            // No `onSideResidualOrder`: Showdown's default order, the side-condition sub-order.
            (SideEffect::WideGuard, ORDER_DEFAULT, SUB_SIDE_CONDITION),
            (SideEffect::QuickGuard, ORDER_DEFAULT, SUB_SIDE_CONDITION),
            (SideEffect::CraftyShield, ORDER_DEFAULT, SUB_SIDE_CONDITION),
            (SideEffect::MatBlock, ORDER_DEFAULT, SUB_SIDE_CONDITION),
        ] {
            if b.side_effect_active(side, effect) {
                out.push(field(order, sub_order, Kind::SideDuration(side, effect)));
            }
        }
        for slot in Battle::<N>::slots(side) {
            // Slot conditions, for the Pokémon holding the position, standing or fainted. A
            // fainted holder's `pokemon.speed` is from before it fainted, which the state does
            // not keep; its handlers only end their condition (no heal, no hit), which no other
            // handler's order depends on.
            let held = b.occupant(slot).is_some() || b.state.slot(slot).fainted_occupant.is_some();
            if held {
                let speed = if b.alive(slot).is_some() {
                    b.action_speed(slot)
                } else {
                    0
                };
                for (condition, order) in [
                    (SlotCondition::FutureMove, 3),
                    (SlotCondition::Wish, 4),
                    (SlotCondition::RevivalBlessing, ORDER_DEFAULT),
                ] {
                    if conditions::slot_condition(b, slot, condition).is_active() {
                        out.push(Handler {
                            order,
                            speed,
                            sub_order: SUB_SLOT_CONDITION,
                            kind: Kind::SlotCondition(slot, condition),
                        });
                    }
                }
            }
            let Some(pokemon) = b.alive(slot) else {
                continue;
            };
            // `pokemon.speed` (Champions `getActionSpeed`, negated under Trick Room).
            let speed = b.action_speed(slot);
            let mon = b.mon(pokemon);
            let status_order = match mon.status {
                Status::Burn => Some(10),
                Status::Poison | Status::Toxic => Some(9),
                _ => None,
            };
            if let Some(order) = status_order {
                out.push(Handler {
                    order,
                    speed,
                    sub_order: SUB_STATUS,
                    kind: Kind::StatusDamage(pokemon, slot),
                });
            }
            for (volatile, state) in b.state.slot(slot).volatiles.iter() {
                if volatile == Volatile::LeechSeed {
                    out.push(Handler {
                        order: volatile.residual_order().unwrap_or(ORDER_DEFAULT),
                        speed,
                        sub_order: SUB_CONDITION,
                        kind: Kind::LeechSeed(pokemon, slot),
                    });
                }
                if matches!(
                    volatile,
                    Volatile::Ingrain
                        | Volatile::SaltCure
                        | Volatile::Nightmare
                        | Volatile::Octolock
                        | Volatile::AquaRing
                        | Volatile::Curse
                ) {
                    out.push(Handler {
                        order: volatile.residual_order().unwrap_or(ORDER_DEFAULT),
                        speed,
                        sub_order: SUB_CONDITION,
                        kind: Kind::VolatileEffect(pokemon, slot, volatile),
                    });
                }
                if state.duration > 0 {
                    // Uproar's `onResidualSubOrder: 1`; the others take the condition's.
                    let sub_order = if volatile == Volatile::Uproar {
                        1
                    } else {
                        SUB_CONDITION
                    };
                    out.push(Handler {
                        order: volatile.residual_order().unwrap_or(ORDER_DEFAULT),
                        speed,
                        sub_order,
                        kind: Kind::VolatileDuration(pokemon, slot, volatile),
                    });
                }
            }
            if b.terrain() == Terrain::Grassy {
                out.push(Handler {
                    order: 5,
                    speed,
                    sub_order: 2,
                    kind: Kind::GrassyHeal(pokemon, slot),
                });
            }
            // The effective item: a suppressed one (Magic Room, Klutz) has no residual.
            let item = b.item(slot);
            if let Some((order, sub_order)) = item_events::residual_order(item) {
                out.push(Handler {
                    order,
                    speed,
                    sub_order,
                    kind: Kind::Item(pokemon, slot, item),
                });
            }
            if item == items::LEFTOVERS {
                out.push(Handler {
                    order: 5,
                    speed,
                    sub_order: 4,
                    kind: Kind::Leftovers(pokemon, slot),
                });
            }
            // Gathered by the raw ability (`getAbility()`); a suppressed one is skipped when it
            // runs (`singleEvent`: `ignoringAbility`).
            match mon.ability {
                a if a == abilities::SPEED_BOOST => out.push(Handler {
                    order: 28,
                    speed,
                    sub_order: 2,
                    kind: Kind::SpeedBoost(pokemon, slot),
                }),
                a if a == abilities::HARVEST => out.push(Handler {
                    order: 28,
                    speed,
                    sub_order: 2,
                    kind: Kind::Harvest(pokemon, slot),
                }),
                a if a == abilities::SHED_SKIN
                    || a == abilities::HYDRATION
                    || a == abilities::HEALER =>
                {
                    out.push(Handler {
                        order: 5,
                        speed,
                        sub_order: 3,
                        kind: Kind::StatusCure(pokemon, slot, a),
                    })
                }
                a if ability_events::has_residual(a) => {
                    let (order, sub_order) = ability_events::residual_order(a);
                    out.push(Handler {
                        order,
                        speed,
                        sub_order,
                        kind: Kind::Ability(pokemon, slot, a),
                    })
                }
                a if super::forme::has_residual(a) => {
                    let orders = a.data().event_orders;
                    out.push(Handler {
                        order: ability_events::priority(orders, "onResidualOrder") as u32,
                        speed,
                        // No `onResidualSubOrder`: the ability's effect-type sub-order.
                        sub_order: ability_events::SUB_ABILITY,
                        kind: Kind::Forme(pokemon, slot, a),
                    });
                }
                _ => {}
            }
        }
    }
    let _ = SUB_FIELD_CONDITION;
    out
}

/// Showdown `speedSort`: selection by key, ties in uniformly random order.
fn speed_sort<const N: usize>(b: &mut Battle<'_, N>, list: &mut [Handler]) {
    let mut sorted = 0;
    while sorted < list.len() {
        let best = list[sorted..]
            .iter()
            .map(Handler::key)
            .min()
            .expect("non-empty");
        let mut tied: Vec<usize> = (sorted..list.len())
            .filter(|&i| list[i].key() == best)
            .collect();
        // Move the tied group to the front in order, then pick their order at random.
        for (offset, &i) in tied.iter().enumerate() {
            list.swap(sorted + offset, i);
        }
        let count = tied.len();
        tied.clear();
        for k in 0..count {
            let pick = b.rng.uniform(count - k);
            list.swap(sorted + k, sorted + k + pick);
        }
        sorted += count;
    }
}

fn still_active<const N: usize>(b: &Battle<'_, N>, pokemon: PokemonRef, slot: SlotRef) -> bool {
    b.alive(slot) == Some(pokemon)
}

impl Kind {
    /// The Pokémon holding the handler's effect, for handlers of a Pokémon.
    fn holder(self) -> Option<(PokemonRef, SlotRef)> {
        match self {
            Kind::VolatileDuration(p, s, _)
            | Kind::StatusDamage(p, s)
            | Kind::GrassyHeal(p, s)
            | Kind::Leftovers(p, s)
            | Kind::SpeedBoost(p, s)
            | Kind::StatusCure(p, s, _)
            | Kind::Item(p, s, _)
            | Kind::LeechSeed(p, s)
            | Kind::VolatileEffect(p, s, _) => Some((p, s)),
            Kind::Forme(p, s, _) | Kind::Harvest(p, s) | Kind::Ability(p, s, _) => Some((p, s)),
            Kind::Weather
            | Kind::FieldDuration(_)
            | Kind::SideDuration(..)
            | Kind::SlotCondition(..) => None,
        }
    }
}

/// Runs one residual handler. Returns whether Showdown's `fieldEvent` calls `faintMessages()`
/// after it: not when the holder has already fainted (the handler is skipped), nor when the
/// handler's effect ran out of duration (its `End` runs, then `continue`).
fn run<const N: usize>(b: &mut Battle<'_, N>, handler: Handler) -> Result<bool, TurnError> {
    if let Some((pokemon, slot)) = handler.kind.holder() {
        if b.occupant(slot) != Some(pokemon) {
            return Ok(false);
        }
    }
    match handler.kind {
        Kind::Weather => {
            let mut effect = b.state.field[FieldEffect::Weather as usize];
            if !effect.is_active() {
                return Ok(true);
            }
            effect.turns -= 1;
            if effect.turns == 0 {
                // The `end` callback is `field.clearWeather`: WeatherChange follows.
                b.set_field(FieldEffect::Weather, Effect::NONE);
                super::field_events::weather_changed(b);
                return Ok(false);
            }
            b.set_field(FieldEffect::Weather, effect);
            // `onFieldResidual` of every supported weather: eachEvent('Weather'), actives in
            // Speed order, ties shuffled. A suppressed weather still counts down, but its own
            // `onWeather` and every `Weather` handler are skipped (sandstorm and snow do not even
            // run the event; sun and rain run it with every handler skipped).
            let weather = b.effective_weather();
            if weather == Weather::None {
                return Ok(true);
            }
            let mut actives: Vec<(SlotRef, i32)> = b
                .all_alive()
                .into_iter()
                .map(|s| (s, b.action_speed(s)))
                .collect();
            // Without a handler that acts, the order (and its random tie-breaks) is moot.
            let acts = weather == Weather::Sand
                || actives.iter().any(|&(s, _)| {
                    [
                        abilities::RAIN_DISH,
                        abilities::ICE_BODY,
                        abilities::SOLAR_POWER,
                        abilities::DRY_SKIN,
                    ]
                    .contains(&b.ability(s))
                });
            if !acts {
                return Ok(true);
            }
            sort_by_speed(b, &mut actives);
            for (slot, _) in actives {
                if b.alive(slot).is_some() {
                    // The abilities' `onWeather` read `target.effectiveWeather()` (Utility
                    // Umbrella hides sun and rain); sandstorm's damage does not.
                    let seen = if weather == Weather::Sand {
                        weather
                    } else {
                        b.weather_for(slot)
                    };
                    weather_event(b, slot, seen);
                }
            }
            // `eachEvent('Weather')` ends with an Update (gen 7+).
            super::update::update_event(b)?;
        }
        Kind::FieldDuration(which) => {
            let mut effect = b.state.field[which as usize];
            if !effect.is_active() {
                return Ok(true);
            }
            effect.turns -= 1;
            let ended = effect.turns == 0;
            b.set_field(which, if ended { Effect::NONE } else { effect });
            // A terrain's `end` callback is `field.clearTerrain`: TerrainChange follows.
            if ended && which == FieldEffect::Terrain {
                super::field_events::terrain_changed(b);
            }
            return Ok(!ended);
        }
        Kind::SideDuration(side, which) => {
            let mut effect = b.state.side(side).effects[which as usize];
            if !effect.is_active() {
                return Ok(true);
            }
            effect.turns -= 1;
            let ended = effect.turns == 0;
            b.set_side_effect(side, which, if ended { Effect::NONE } else { effect });
            return Ok(!ended);
        }
        Kind::SlotCondition(slot, condition) => {
            conditions::slot_condition_residual(b, slot, condition)?;
        }
        Kind::VolatileDuration(pokemon, slot, volatile) => {
            let mut state = b.volatile(slot, volatile);
            if !state.active || state.duration == 0 {
                return Ok(true);
            }
            let ended = state.duration == 1;
            // `removeVolatile` does nothing for a Pokémon at 0 HP (it faints next anyway).
            if !still_active(b, pokemon, slot) {
                return Ok(!ended);
            }
            if ended {
                // `removeVolatile`: the condition's `End`, then the volatile is gone.
                conditions::volatile_end(b, pokemon, slot, volatile)?;
                b.set_volatile_state(slot, volatile, VolatileState::NONE);
                return Ok(false);
            }
            state.duration -= 1;
            b.set_volatile_state(slot, volatile, state);
            // The condition's own `onResidual`.
            match volatile {
                Volatile::LockedMove => {
                    // `if (target.status === 'slp') delete target.volatiles['lockedmove']`
                    // (no onEnd), then `trueDuration--`.
                    if b.mon(pokemon).status == Status::Sleep {
                        b.delete_volatile(slot, volatile);
                    } else {
                        state.hidden = state.hidden.saturating_sub(1);
                        b.set_volatile_state(slot, volatile, state);
                    }
                }
                Volatile::PartiallyTrapped => conditions::partially_trapped_residual(b, slot),
                // Uproar: `if (target.volatiles['throatchop']) { target.removeVolatile('uproar');
                // return; }` then `if (target.lastMove?.id === 'struggle') delete
                // target.volatiles['uproar'];` (both ends only log).
                Volatile::Uproar => {
                    if b.volatile(slot, Volatile::ThroatChop).active {
                        b.remove_volatile(slot, volatile);
                    } else if b.state.slot(slot).last_move == crate::dex::moves::STRUGGLE {
                        b.delete_volatile(slot, volatile);
                    }
                }
                // Syrup Bomb: `this.boost({spe: -1}, pokemon, this.effectState.source)`.
                Volatile::SyrupBomb => conditions::syrup_bomb_residual(b, slot)?,
                // Rollout, Ice Ball: `if (target.lastMove && target.lastMove.id === 'struggle')
                // delete target.volatiles['rollout'];` (no lock after Struggle).
                Volatile::Rollout | Volatile::IceBall => {
                    if b.state.slot(slot).last_move == crate::dex::moves::STRUGGLE {
                        b.delete_volatile(slot, volatile);
                    }
                }
                Volatile::Encore => {
                    // Over once the encored move has no PP left.
                    let out_of_pp = b
                        .mon(pokemon)
                        .moves
                        .iter()
                        .find(|m| m.id == state.mv)
                        .is_none_or(|m| m.pp == 0);
                    if out_of_pp {
                        b.remove_volatile(slot, volatile);
                    }
                }
                _ => {}
            }
        }
        Kind::StatusDamage(pokemon, slot) => {
            if !still_active(b, pokemon, slot) {
                return Ok(true);
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            // Poison Heal (`onDamage`, priority 1) turns the poison damage into a heal.
            match b.mon(pokemon).status {
                Status::Burn => {
                    let damage = ability_events::burn_damage(b.ability(slot), max_hp);
                    b.damage(slot, damage, DamageSource::Indirect);
                }
                Status::Poison => {
                    if !ability_events::poison_heal(b, slot) {
                        b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
                    }
                }
                Status::Toxic => {
                    let stage = b.mon(pokemon).status_turns;
                    let stage = if stage < 15 { stage + 1 } else { stage };
                    b.set_status_turns(pokemon, stage);
                    let unit = (max_hp / 16.0).floor().max(1.0);
                    if !ability_events::poison_heal(b, slot) {
                        b.damage(slot, unit * f64::from(stage), DamageSource::Indirect);
                    }
                }
                _ => {}
            }
        }
        Kind::GrassyHeal(pokemon, slot) => {
            if !still_active(b, pokemon, slot) || b.terrain() != Terrain::Grassy {
                return Ok(true);
            }
            if b.is_grounded(slot) {
                let max_hp = f64::from(b.mon(pokemon).max_hp);
                b.heal(slot, max_hp / 16.0);
            }
        }
        Kind::Leftovers(pokemon, slot) => {
            if !still_active(b, pokemon, slot) || b.item(slot) != items::LEFTOVERS {
                return Ok(true);
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            b.heal(slot, max_hp / 16.0);
        }
        Kind::Item(pokemon, slot, item) => {
            if !still_active(b, pokemon, slot) || b.item(slot) != item {
                return Ok(true);
            }
            item_events::on_residual(b, slot, item);
        }
        Kind::LeechSeed(pokemon, slot) => {
            if !still_active(b, pokemon, slot) {
                return Ok(true);
            }
            conditions::leech_seed_residual(b, slot);
        }
        Kind::VolatileEffect(pokemon, slot, volatile) => {
            // Skipped if the volatile ended since the handlers were collected.
            if !still_active(b, pokemon, slot) || !b.volatile(slot, volatile).active {
                return Ok(true);
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            match volatile {
                // Ingrain: `this.heal(pokemon.baseMaxhp / 16)`.
                Volatile::Ingrain => {
                    b.heal(slot, max_hp / 16.0);
                }
                // Salt Cure (Champions): `this.damage(pokemon.baseMaxhp / (pokemon.hasType(['Water',
                // 'Steel']) ? 8 : 16))`.
                Volatile::SaltCure => {
                    let tougher = b.has_type(slot, Type::Water) || b.has_type(slot, Type::Steel);
                    let divisor = if tougher { 8.0 } else { 16.0 };
                    b.damage(slot, max_hp / divisor, DamageSource::Indirect);
                }
                // Nightmare: `this.damage(pokemon.baseMaxhp / 4)` (the condition's damage, not a
                // move's: Magic Guard stops it).
                Volatile::Nightmare => {
                    b.damage(slot, max_hp / 4.0, DamageSource::Indirect);
                }
                Volatile::Octolock => conditions::octolock_residual(b, slot),
                // Curse: `this.damage(pokemon.baseMaxhp / 4)` (the condition's damage: Magic Guard
                // stops it).
                Volatile::Curse => {
                    b.damage(slot, max_hp / 4.0, DamageSource::Indirect);
                }
                // Aqua Ring: `this.heal(pokemon.baseMaxhp / 16)` (its effect is listed by Big
                // Root).
                Volatile::AquaRing => {
                    b.heal_rooted(slot, max_hp / 16.0);
                }
                _ => {}
            }
        }
        Kind::Forme(pokemon, slot, ability) => {
            // Skipped if the ability changed since the handlers were collected, or is
            // suppressed (Gastro Acid, Neutralizing Gas).
            if !still_active(b, pokemon, slot) || b.ability(slot) != ability {
                return Ok(true);
            }
            super::forme::residual(b, slot, ability)?;
        }
        Kind::Ability(pokemon, slot, ability) => {
            // Skipped if the ability changed since the handlers were collected, or is
            // suppressed.
            if !still_active(b, pokemon, slot) || b.ability(slot) != ability {
                return Ok(true);
            }
            ability_events::on_residual(b, slot, ability)?;
        }
        Kind::Harvest(pokemon, slot) => {
            // Skipped if the ability changed since the handlers were collected.
            if !still_active(b, pokemon, slot) || b.ability(slot) != abilities::HARVEST {
                return Ok(true);
            }
            ability_events::harvest(b, pokemon);
        }
        Kind::SpeedBoost(pokemon, slot) => {
            // Skipped if the ability changed since the handlers were collected.
            if !still_active(b, pokemon, slot) || b.ability(slot) != abilities::SPEED_BOOST {
                return Ok(true);
            }
            // `if (pokemon.activeTurns) this.boost({spe: 1})`.
            if b.active_since_turn_start(slot) {
                let mut boost = NO_BOOSTS;
                boost[4] = 1;
                b.boost_by(
                    slot,
                    &boost,
                    Some(slot),
                    BoostEffect::Ability(abilities::SPEED_BOOST),
                );
            }
        }
        Kind::StatusCure(pokemon, slot, ability) => {
            if !still_active(b, pokemon, slot) || b.ability(slot) != ability {
                return Ok(true);
            }
            // Healer (Champions): every adjacent ally not at 0 HP with a status is cured on
            // `this.randomChance(1, 2)` (mainline: 3/10).
            if ability == abilities::HEALER {
                for ally in b.alive_slots(slot.side) {
                    let Some(partner) = b.alive(ally).filter(|_| ally != slot) else {
                        continue;
                    };
                    if b.mon(partner).status != Status::None && b.rng.chance(1, 2) {
                        b.cure_status(partner);
                    }
                }
                return Ok(true);
            }
            if b.mon(pokemon).status == Status::None {
                return Ok(true);
            }
            let cure = if ability == abilities::SHED_SKIN {
                // `pokemon.hp && pokemon.status && this.randomChance(33, 100)` (not modded in
                // Champions).
                b.rng.chance(33, 100)
            } else {
                // Hydration: `pokemon.effectiveWeather()` is rain (Utility Umbrella hides it;
                // Primordial Sea is not supported).
                b.weather_for(slot) == Weather::Rain
            };
            if cure {
                b.cure_status(pokemon);
            }
        }
    }
    let _ = Type::None;
    Ok(true)
}

/// `runEvent('Weather', pokemon)` during the weather's residual: the sandstorm's own
/// `onWeather` (1/16 damage unless immune) and the ability's `onWeather` (they never both act on
/// one Pokémon, so their order does not matter). `effectiveWeather()` is the weather (the caller
/// skips a suppressed one): Utility Umbrella and the primal weathers are not supported.
/// - Rain Dish: heal 1/16 in rain; Ice Body: heal 1/16 in snow;
/// - Solar Power: 1/8 damage in sun (`this.damage(maxhp / 8, target, target)`);
/// - Dry Skin: heal 1/8 in rain, 1/8 damage in sun.
fn weather_event<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, weather: Weather) {
    let max_hp = f64::from(b.slot_mon(slot).expect("alive").max_hp);
    if weather == Weather::Sand {
        // Dig's and Dive's `onImmunity` (`type === 'sandstorm'`): no damage underground.
        let underground =
            b.volatile(slot, Volatile::Dig).active || b.volatile(slot, Volatile::Dive).active;
        if !underground && !b.status_immune(slot, TypeImmunities::SANDSTORM) {
            b.damage(slot, max_hp / 16.0, DamageSource::Indirect);
        }
        return;
    }
    match (b.ability(slot), weather) {
        (a, Weather::Rain) if a == abilities::RAIN_DISH => {
            b.heal(slot, max_hp / 16.0);
        }
        (a, Weather::Snow) if a == abilities::ICE_BODY => {
            b.heal(slot, max_hp / 16.0);
        }
        (a, Weather::Rain) if a == abilities::DRY_SKIN => {
            b.heal(slot, max_hp / 8.0);
        }
        (a, Weather::Sun) if a == abilities::DRY_SKIN || a == abilities::SOLAR_POWER => {
            b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
        }
        _ => {}
    }
}

/// `speedSort` of Pokémon by `pokemon.speed`, ties shuffled.
pub(crate) fn sort_by_speed<const N: usize>(b: &mut Battle<'_, N>, list: &mut [(SlotRef, i32)]) {
    let mut sorted = 0;
    while sorted < list.len() {
        let best = list[sorted..]
            .iter()
            .map(|&(_, s)| s)
            .max()
            .expect("non-empty");
        let tied: Vec<usize> = (sorted..list.len())
            .filter(|&i| list[i].1 == best)
            .collect();
        for (offset, &i) in tied.iter().enumerate() {
            list.swap(sorted + offset, i);
        }
        let count = tied.len();
        for k in 0..count {
            let pick = b.rng.uniform(count - k);
            list.swap(sorted + k, sorted + k + pick);
        }
        sorted += count;
    }
}

/// `checkFainted`: a fainted Pokémon still holding an active position gets `fnt`.
pub(crate) fn check_fainted<const N: usize>(b: &mut Battle<'_, N>) {
    for slot in State::<N>::slot_refs() {
        let Some(party) = b.state.slot(slot).fainted_occupant else {
            continue;
        };
        let pokemon = PokemonRef {
            side: slot.side,
            party,
        };
        let old = b.mon(pokemon).status;
        if old != Status::Fainted {
            b.apply(Instruction::ChangeStatus {
                target: pokemon,
                old,
                new: Status::Fainted,
            });
        }
    }
}

/// `endTurn`: the turn advances unless a side must first replace a fainted Pokémon.
pub(crate) fn end_turn<const N: usize>(b: &mut Battle<'_, N>) {
    let needs_switch = [SideId::One, SideId::Two]
        .into_iter()
        .any(|side| needs_replacement(b, side));
    if !needs_switch {
        // `endTurn`: the DisableMove handlers of every active Pokémon, and the per-turn
        // damage-history resets (F13).
        item_events::end_turn_disable_move(b);
        b.end_turn_history();
        b.reset_stat_changes();
        let turn = b.state.turn;
        b.apply(Instruction::SetTurn {
            old: turn,
            new: turn + 1,
        });
    }
}

/// A side has an empty active slot and a healthy Pokémon on the bench.
pub(crate) fn needs_replacement<const N: usize>(b: &Battle<'_, N>, side: SideId) -> bool {
    let s = b.state.side(side);
    let empty = s.slots.iter().any(|slot| slot.party_index.is_none());
    empty && bench(b, side).next().is_some()
}

pub(crate) fn bench<'b, const N: usize>(
    b: &'b Battle<'_, N>,
    side: SideId,
) -> impl Iterator<Item = u8> + 'b {
    let s = b.state.side(side);
    (0..s.party.len() as u8).filter(move |&i| {
        s.party[i as usize].hp > 0 && !s.slots.iter().any(|slot| slot.party_index == Some(i))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wonder Room's residual order and sub-order hard-coded in `collect` are the dex's.
    #[test]
    fn wonder_room_residual_order_matches_the_dex() {
        use crate::dex::moves;
        let orders = moves::WONDER_ROOM.data().event_orders;
        assert!(orders.contains(&("condition.onFieldResidualOrder", 27)));
        assert!(orders.contains(&("condition.onFieldResidualSubOrder", 5)));
        assert_eq!(moves::WONDER_ROOM.data().condition_duration, 5);
    }

    /// Fairy Lock's condition lasts 2 turns and has no residual order (`collect` uses the
    /// default order).
    #[test]
    fn fairy_lock_residual_order_matches_the_dex() {
        let data = crate::dex::moves::FAIRY_LOCK.data();
        assert_eq!(data.condition_duration, 2);
        assert!(!data
            .event_orders
            .iter()
            .any(|(n, _)| n.contains("Residual")));
    }

    /// The side-condition residual orders hard-coded in `collect` are the dex's.
    #[test]
    fn side_condition_residual_orders_match_the_dex() {
        use crate::dex::moves;
        for (id, order) in [
            (moves::SAFEGUARD, Some((26, 3))),
            (moves::MIST, Some((26, 4))),
            (moves::LUCKY_CHANT, Some((26, 6))),
            (moves::WIDE_GUARD, None),
            (moves::QUICK_GUARD, None),
            (moves::CRAFTY_SHIELD, None),
            (moves::MAT_BLOCK, None),
        ] {
            let orders = id.data().event_orders;
            let found = orders
                .iter()
                .find(|(n, _)| *n == "condition.onSideResidualOrder")
                .map(|&(_, o)| o);
            let sub = orders
                .iter()
                .find(|(n, _)| *n == "condition.onSideResidualSubOrder")
                .map(|&(_, o)| o);
            match order {
                Some((o, s)) => {
                    assert_eq!(found, Some(o), "{id:?}");
                    assert_eq!(sub, Some(s), "{id:?}");
                }
                None => assert_eq!((found, sub), (None, None), "{id:?}"),
            }
        }
    }

    /// The residual orders hard-coded in `collect` are the dex's.
    #[test]
    fn ability_residual_orders_match_the_dex() {
        for (ability, order, sub_order) in [
            (abilities::SPEED_BOOST, 28, 2),
            (abilities::SHED_SKIN, 5, 3),
            (abilities::HYDRATION, 5, 3),
        ] {
            let orders = ability.data().event_orders;
            assert!(orders.contains(&("onResidualOrder", order)), "{ability:?}");
            assert!(
                orders.contains(&("onResidualSubOrder", sub_order)),
                "{ability:?}"
            );
            assert_eq!(ability.data().handlers, ["onResidual"], "{ability:?}");
        }
    }
}
