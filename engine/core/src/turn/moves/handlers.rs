//! Move-specific Showdown callbacks (`data/moves.ts`, with the Champions overrides of
//! `data/mods/champions/moves.ts`) for moves listed in `support::MOVES_WITH_HANDLERS`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. A move not
//! handled here gets the event's neutral result.

use crate::damage::MOD_ONE_POINT_FIVE;
use crate::dex::moves;
use crate::field::FieldEffect;

use super::super::battle::Battle;
use super::ActiveMove;

/// The move's own `onBasePower` modifier (BasePower handler priority 0, after type items and
/// terrain). Knock Off's is in `get_damage`.
pub(super) fn on_base_power<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove) -> Option<u32> {
    match mv.id {
        // Grav Apple: `if (this.field.getPseudoWeather('gravity')) return this.chainModify(1.5);`
        moves::GRAV_APPLE if b.field_active(FieldEffect::Gravity) => Some(MOD_ONE_POINT_FIVE),
        _ => None,
    }
}
