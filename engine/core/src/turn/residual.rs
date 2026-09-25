//! End of turn: Showdown's `residual` action (`fieldEvent('Residual')`), then
//! `checkFainted` and `endTurn`.
//!
//! Handlers are collected once, sorted like Showdown's `comparePriority` (order ascending,
//! priority descending, Speed descending, sub-order ascending; ties shuffled), and run in
//! turn. A handler whose effect has a duration decrements it first; one that reaches 0 ends
//! the effect instead of running. Handlers of fainted Pokémon and of effects that ended
//! earlier in the residual are skipped. Faints are processed after every handler.

use crate::dex::{abilities, items, AbilityId, Type, TypeImmunities, NO_BOOSTS};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, SlotRef, State, Status};
use crate::volatile::Volatile;

use super::abilities as ability_events;
use super::battle::{Battle, BoostEffect, DamageSource};
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
    /// Shed Skin and Hydration `onResidual` (order 5, sub-order 3).
    StatusCure(PokemonRef, SlotRef, AbilityId),
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
        run(b, handler)?;
        if b.faint_messages(true) {
            return Ok(());
        }
    }
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
    for side in [SideId::One, SideId::Two] {
        for (effect, sub_order) in [
            (SideEffect::Reflect, 1),
            (SideEffect::LightScreen, 2),
            (SideEffect::Tailwind, 5),
            (SideEffect::AuroraVeil, 10),
        ] {
            if b.side_effect_active(side, effect) {
                out.push(field(26, sub_order, Kind::SideDuration(side, effect)));
            }
        }
        let _ = SUB_SIDE_CONDITION;
        for slot in Battle::<N>::slots(side) {
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
                if state.duration > 0 {
                    // Encore's own handler carries its duration tick (`onResidualOrder: 16`).
                    let order = if volatile == Volatile::Encore {
                        16
                    } else {
                        ORDER_DEFAULT
                    };
                    out.push(Handler {
                        order,
                        speed,
                        sub_order: SUB_CONDITION,
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
            if mon.item == items::LEFTOVERS {
                out.push(Handler {
                    order: 5,
                    speed,
                    sub_order: 4,
                    kind: Kind::Leftovers(pokemon, slot),
                });
            }
            match mon.ability {
                a if a == abilities::SPEED_BOOST => out.push(Handler {
                    order: 28,
                    speed,
                    sub_order: 2,
                    kind: Kind::SpeedBoost(pokemon, slot),
                }),
                a if a == abilities::SHED_SKIN || a == abilities::HYDRATION => out.push(Handler {
                    order: 5,
                    speed,
                    sub_order: 3,
                    kind: Kind::StatusCure(pokemon, slot, a),
                }),
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

fn run<const N: usize>(b: &mut Battle<'_, N>, handler: Handler) -> Result<(), TurnError> {
    match handler.kind {
        Kind::Weather => {
            let mut effect = b.state.field[FieldEffect::Weather as usize];
            if !effect.is_active() {
                return Ok(());
            }
            effect.turns -= 1;
            if effect.turns == 0 {
                b.set_field(FieldEffect::Weather, Effect::NONE);
                return Ok(());
            }
            b.set_field(FieldEffect::Weather, effect);
            // `onFieldResidual` of every supported weather: eachEvent('Weather'), actives in
            // Speed order, ties shuffled. A suppressed weather still counts down, but its own
            // `onWeather` and every `Weather` handler are skipped (sandstorm and snow do not even
            // run the event; sun and rain run it with every handler skipped).
            let weather = b.effective_weather();
            if weather == Weather::None {
                return Ok(());
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
                return Ok(());
            }
            sort_by_speed(b, &mut actives);
            for (slot, _) in actives {
                if b.alive(slot).is_some() {
                    weather_event(b, slot, weather);
                }
            }
            // `eachEvent('Weather')` ends with an Update (gen 7+).
            super::update::update_event(b)?;
        }
        Kind::FieldDuration(which) => {
            let mut effect = b.state.field[which as usize];
            if !effect.is_active() {
                return Ok(());
            }
            effect.turns -= 1;
            b.set_field(
                which,
                if effect.turns == 0 {
                    Effect::NONE
                } else {
                    effect
                },
            );
        }
        Kind::SideDuration(side, which) => {
            let mut effect = b.state.side(side).effects[which as usize];
            if !effect.is_active() {
                return Ok(());
            }
            effect.turns -= 1;
            b.set_side_effect(
                side,
                which,
                if effect.turns == 0 {
                    Effect::NONE
                } else {
                    effect
                },
            );
        }
        Kind::VolatileDuration(pokemon, slot, volatile) => {
            if !still_active(b, pokemon, slot) {
                return Ok(());
            }
            let mut state = b.volatile(slot, volatile);
            if !state.active || state.duration == 0 {
                return Ok(());
            }
            state.duration -= 1;
            if state.duration == 0 {
                // The condition ends (`onEnd`), and its `onResidual` does not run.
                b.remove_volatile(slot, volatile);
                return Ok(());
            }
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
                return Ok(());
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            match b.mon(pokemon).status {
                Status::Burn => {
                    let damage = ability_events::burn_damage(b.mon(pokemon).ability, max_hp);
                    b.damage(slot, damage, DamageSource::Indirect);
                }
                Status::Poison => {
                    b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
                }
                Status::Toxic => {
                    let stage = b.mon(pokemon).status_turns;
                    let stage = if stage < 15 { stage + 1 } else { stage };
                    b.set_status_turns(pokemon, stage);
                    let unit = (max_hp / 16.0).floor().max(1.0);
                    b.damage(slot, unit * f64::from(stage), DamageSource::Indirect);
                }
                _ => {}
            }
        }
        Kind::GrassyHeal(pokemon, slot) => {
            if !still_active(b, pokemon, slot) || b.terrain() != Terrain::Grassy {
                return Ok(());
            }
            if b.is_grounded(slot) {
                let max_hp = f64::from(b.mon(pokemon).max_hp);
                b.heal(slot, max_hp / 16.0);
            }
        }
        Kind::Leftovers(pokemon, slot) => {
            if !still_active(b, pokemon, slot) || b.mon(pokemon).item != items::LEFTOVERS {
                return Ok(());
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            b.heal(slot, max_hp / 16.0);
        }
        Kind::SpeedBoost(pokemon, slot) => {
            // Skipped if the ability changed since the handlers were collected.
            if !still_active(b, pokemon, slot) || b.mon(pokemon).ability != abilities::SPEED_BOOST {
                return Ok(());
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
            if !still_active(b, pokemon, slot) || b.mon(pokemon).ability != ability {
                return Ok(());
            }
            if b.mon(pokemon).status == Status::None {
                return Ok(());
            }
            let cure = if ability == abilities::SHED_SKIN {
                // `pokemon.hp && pokemon.status && this.randomChance(33, 100)` (not modded in
                // Champions).
                b.rng.chance(33, 100)
            } else {
                // Hydration: `pokemon.effectiveWeather()` is rain (Utility Umbrella and
                // Primordial Sea are not supported).
                b.effective_weather() == Weather::Rain
            };
            if cure {
                b.cure_status(pokemon);
            }
        }
    }
    let _ = Type::None;
    Ok(())
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
        if !b.status_immune(slot, TypeImmunities::SANDSTORM) {
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
