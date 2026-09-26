//! `pokemon.speed` after a mid-action forme change: Showdown's `setSpecies` sets it to the raw
//! stored Speed until the next `updateSpeed()` (after the action), and event handlers are
//! sorted by it. Fixture from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Stance Change in Smart Strike's ModifyMove drops a +2 Aegislash to its raw Speed for the
/// ModifyDamage chain: Multiscale, Friend Guard, then Expert Belt (1843/4096, not 1844).
#[test]
fn stance_change_raw_speed_matches_showdown_exactly() {
    assert_exact_parity("stance-change-raw-speed");
}
