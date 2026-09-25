//! End of turn: Showdown's `residual` action (`fieldEvent('Residual')`), then
//! `checkFainted` and `endTurn`.
//!
//! Handlers are collected once, sorted like Showdown's `comparePriority` (order ascending,
//! priority descending, Speed descending, sub-order ascending; ties shuffled), and run in
//! turn. A handler whose effect has a duration decrements it first; one that reaches 0 ends
//! the effect instead of running. Handlers of fainted Pokémon and of effects that ended
//! earlier in the residual are skipped. Faints are processed after every handler.

use crate::dex::{items, Type, TypeImmunities};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, SlotRef, Status};
use crate::volatile::{Volatile, VolatileState};

use super::abilities;
use super::battle::{Battle, DamageSource};
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
                    out.push(Handler {
                        order: ORDER_DEFAULT,
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
            let weather = b.weather();
            // Without a listener the event only shuffles speed ties, which changes nothing.
            let listeners = weather == Weather::Sand
                || b.all_alive()
                    .into_iter()
                    .any(|s| abilities::has_weather_handler(b.ability(s)));
            if listeners {
                // eachEvent('Weather'): actives in Speed order, ties shuffled. Each runs its
                // ability's onWeather (Dry Skin), then the weather's own (sandstorm damage);
                // no Pokémon has both.
                let mut actives: Vec<(SlotRef, i32)> = b
                    .all_alive()
                    .into_iter()
                    .map(|s| (s, b.action_speed(s)))
                    .collect();
                sort_by_speed(b, &mut actives);
                for (slot, _) in actives {
                    if b.alive(slot).is_none() {
                        continue;
                    }
                    abilities::on_weather(b, slot, weather);
                    if weather != Weather::Sand
                        || b.alive(slot).is_none()
                        || b.status_immune(slot, TypeImmunities::SANDSTORM)
                    {
                        continue;
                    }
                    let max_hp = f64::from(b.slot_mon(slot).expect("alive").max_hp);
                    b.damage(slot, max_hp / 16.0, DamageSource::Indirect);
                }
            }
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
                state = VolatileState::NONE;
            }
            b.set_volatile_state(slot, volatile, state);
        }
        Kind::StatusDamage(pokemon, slot) => {
            if !still_active(b, pokemon, slot) {
                return Ok(());
            }
            let max_hp = f64::from(b.mon(pokemon).max_hp);
            match b.mon(pokemon).status {
                Status::Burn => {
                    let damage = abilities::burn_damage(b.mon(pokemon).ability, max_hp);
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
    }
    let _ = Type::None;
    Ok(())
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

/// `checkFainted` and `endTurn`: fainted Pokémon in active positions get `fnt`; if a side
/// must replace one, the turn waits for that decision, otherwise it advances.
pub(crate) fn end_turn<const N: usize>(b: &mut Battle<'_, N>) {
    let fainted = std::mem::take(&mut b.fainted_positions);
    for &(_, pokemon) in &fainted {
        let old = b.mon(pokemon).status;
        if old != Status::Fainted {
            b.apply(Instruction::ChangeStatus {
                target: pokemon,
                old,
                new: Status::Fainted,
            });
        }
    }
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
