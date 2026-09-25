//! Move-specific Showdown callbacks (`data/moves.ts`, with the Champions overrides of
//! `data/mods/champions/moves.ts`) for moves listed in `support::MOVES_WITH_HANDLERS`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. A move not
//! handled here gets the event's neutral result.

use crate::damage::MOD_ONE_POINT_FIVE;
use crate::dex::moves;
use crate::field::{FieldEffect, Terrain};
use crate::state::SlotRef;

use super::super::battle::Battle;
use super::ActiveMove;

/// The move's `basePowerCallback` (`getDamage`, before the critical hit roll).
pub(super) fn base_power_callback<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    mv: &ActiveMove,
    base_power: i32,
) -> i32 {
    match mv.id {
        // Rising Voltage: `if (this.field.isTerrain('electricterrain') && target.isGrounded())
        // return move.basePower * 2;`
        moves::RISING_VOLTAGE if b.terrain() == Terrain::Electric && b.is_grounded(target) => {
            base_power * 2
        }
        _ => base_power,
    }
}

/// The move's own `onBasePower` modifier (BasePower handler priority 0, after type items and
/// terrain). Knock Off's is in `get_damage`.
pub(super) fn on_base_power<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove) -> Option<u32> {
    match mv.id {
        // Grav Apple: `if (this.field.getPseudoWeather('gravity')) return this.chainModify(1.5);`
        moves::GRAV_APPLE if b.field_active(FieldEffect::Gravity) => Some(MOD_ONE_POINT_FIVE),
        // Psyblade: `if (this.field.isTerrain('electricterrain')) return this.chainModify(1.5);`
        moves::PSYBLADE if b.terrain() == Terrain::Electric => Some(MOD_ONE_POINT_FIVE),
        _ => None,
    }
}
