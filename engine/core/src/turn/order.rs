//! Action order: Showdown `getActionSpeed` (priority and Speed) and the queue's
//! `comparePriority` with random tie-breaks.
//!
//! Gen 8+ re-sorts the remaining queue after every action with fresh Speed and priority, and
//! `speedSort` shuffles ties uniformly. Picking the best remaining action each time, uniform
//! among ties, gives the same distribution of orders.

use crate::damage::{chain_modifiers, MOD_ONE, MOD_ONE_POINT_FIVE};
use crate::dex::{abilities, moves, AbilityId, MoveCategory, MoveFlags, MoveId, Type};
use crate::field::{FieldEffect, SideEffect, Terrain, Weather};
use crate::state::{SlotRef, Status};

use super::battle::Battle;
use super::items as item_events;

/// Showdown action `order` values.
pub(crate) const ORDER_SWITCH: u32 = 103;
/// `megaEvo`: after switches, before every move.
pub(crate) const ORDER_MEGA: u32 = 104;
pub(crate) const ORDER_MOVE: u32 = 200;
/// Showdown's default for handlers without an order (sorts last).
pub(crate) const ORDER_DEFAULT: u32 = u32::MAX;

/// Stat after stage boosts (`floor(stat * multiplier)`).
pub(crate) fn boosted_stat(stat: i32, boost: i8) -> i32 {
    let boost = i32::from(boost.clamp(-6, 6));
    if boost >= 0 {
        stat * (2 + boost) / 2
    } else {
        stat * 2 / (2 - boost)
    }
}

/// Showdown `battle.modify(value, modifier)` with a 4096-based modifier (half rounds down).
pub(crate) fn modify(value: i32, modifier: u32) -> i32 {
    let product = i64::from(value) * i64::from(modifier);
    ((product + 2047) / 4096) as i32
}

impl<const N: usize> Battle<'_, N> {
    /// Showdown `getStat('spe')` (boosts and ModifySpe handlers).
    pub(crate) fn speed_stat(&self, slot: SlotRef) -> i32 {
        let Some(mon) = self.slot_mon(slot) else {
            return 0;
        };
        let mut spe = boosted_stat(i32::from(mon.stats[4]), self.state.slot(slot).boosts[4]);
        // Chained ModifySpe handlers, in handler order: side conditions, then abilities.
        let mut chain = Vec::new();
        if self.side_effect_active(slot.side, SideEffect::Tailwind) {
            chain.push(2 * MOD_ONE);
        }
        // `pokemon.effectiveWeather()` (Chlorophyll, Swift Swim); Sand Rush and Slush Rush read
        // `field.isWeather`, which Utility Umbrella does not touch either way.
        let weather = self.weather_for(slot);
        let doubled = match mon.ability {
            a if a == abilities::SAND_RUSH => weather == Weather::Sand,
            a if a == abilities::CHLOROPHYLL => weather == Weather::Sun,
            a if a == abilities::SWIFT_SWIM => weather == Weather::Rain,
            a if a == abilities::SLUSH_RUSH => weather == Weather::Snow,
            _ => false,
        };
        if doubled {
            chain.push(2 * MOD_ONE);
        }
        // Quick Feet: `if (pokemon.status) return this.chainModify(1.5)`.
        let quick_feet = mon.ability == abilities::QUICK_FEET;
        if quick_feet && mon.status != Status::None {
            chain.push(MOD_ONE_POINT_FIVE);
        }
        // The item (Choice Scarf). The factors are all powers of two times 1.5, so the chain
        // is exact in any order.
        chain.extend(item_events::speed_modifier(mon.item));
        if !chain.is_empty() {
            spe = modify(spe, chain_modifiers(&chain, 0, u32::MAX));
        }
        // Paralysis (priority -101): after every other modifier, `floor(spe * 50 / 100)`
        // unless the Pokémon has Quick Feet.
        if mon.status == Status::Paralyze && !quick_feet {
            spe = spe * 50 / 100;
        }
        spe.min(10000)
    }

    /// Showdown (Champions) `getActionSpeed`: Speed, negated under Trick Room.
    pub(crate) fn action_speed(&self, slot: SlotRef) -> i32 {
        let speed = self.speed_stat(slot);
        if self.field_active(FieldEffect::TrickRoom) {
            -speed
        } else {
            speed
        }
    }

    /// Priority of `id` used by the Pokémon in `slot` (ModifyPriority handlers): the move's own
    /// (`singleEvent`: Grassy Glide), then the user's ability (`runEvent`: Prankster, Gale
    /// Wings, Triage; each adds to the priority it is given).
    pub(crate) fn move_priority(&self, slot: SlotRef, id: MoveId) -> i32 {
        let data = id.data();
        let mut priority = i32::from(data.priority);
        if id == moves::GRASSY_GLIDE && self.terrain() == Terrain::Grassy && self.is_grounded(slot)
        {
            priority += 1;
        }
        if self.prankster_boosted(slot, id) {
            priority += 1;
        }
        match self.ability(slot) {
            // Gale Wings: `move.type === 'Flying' && pokemon.hp === pokemon.maxhp`.
            a if a == abilities::GALE_WINGS
                && data.move_type == Type::Flying
                && self.slot_mon(slot).is_some_and(|m| m.hp == m.max_hp) =>
            {
                priority += 1;
            }
            // Triage: `move.flags['heal']`.
            a if a == abilities::TRIAGE && data.flags.contains(MoveFlags::HEAL) => priority += 3,
            _ => {}
        }
        priority
    }

    /// Prankster raises the priority of status moves (and marks them for the Dark immunity).
    /// Showdown sets `move.pranksterBoosted` in the same `getActionSpeed` that computes the
    /// priority, and `useMove` copies it from the active move, so it is decided when the move
    /// is used, like the priority.
    pub(crate) fn prankster_boosted(&self, slot: SlotRef, id: MoveId) -> bool {
        self.ability(slot) == abilities::PRANKSTER && id.data().category == MoveCategory::Status
    }
}

/// Showdown `runEvent('FractionalPriority')` for a move action, in tenths. It is evaluated
/// once, when the turn's actions are queued (`resolveAction`), and added to the action's
/// priority for sorting only (`move.priority`, which Psychic Terrain and Prankster read, does
/// not include it). The only supported source is Stall's constant `onFractionalPriority: -0.1`;
/// a constant handler replaces the value instead of adding to it.
pub(crate) fn fractional_priority_tenths(ability: AbilityId) -> i8 {
    if ability == abilities::STALL {
        ability.data().fractional_priority_tenths
    } else {
        0
    }
}
