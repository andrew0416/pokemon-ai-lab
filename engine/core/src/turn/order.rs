//! Action order: Showdown `getActionSpeed` (priority and Speed) and the queue's
//! `comparePriority` with random tie-breaks.
//!
//! Gen 8+ re-sorts the remaining queue after every action with fresh Speed and priority, and
//! `speedSort` shuffles ties uniformly. Picking the best remaining action each time, uniform
//! among ties, gives the same distribution of orders.

use crate::damage::{chain_modifiers, MOD_ONE};
use crate::dex::{abilities, moves, MoveCategory, MoveId};
use crate::field::{FieldEffect, SideEffect, Terrain, Weather};
use crate::state::{SlotRef, Status};

use super::battle::Battle;

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
        let weather = self.weather();
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
        if !chain.is_empty() {
            spe = modify(spe, chain_modifiers(&chain, 0, u32::MAX));
        }
        // Paralysis (priority -101): after every other modifier, `floor(spe * 50 / 100)`.
        if mon.status == Status::Paralyze {
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

    /// Priority of `id` used by the Pokémon in `slot` (ModifyPriority handlers).
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
        priority
    }

    /// Prankster raises the priority of status moves (and marks them for the Dark immunity).
    pub(crate) fn prankster_boosted(&self, slot: SlotRef, id: MoveId) -> bool {
        self.ability(slot) == abilities::PRANKSTER && id.data().category == MoveCategory::Status
    }
}
