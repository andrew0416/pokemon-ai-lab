//! Parity of the target's `ModifySecondaries` with King's Rock's appended flinch (Opus BB unit
//! B20): the flinch is one of `move.secondaries` when the target's handlers run, so Shield Dust
//! (breakable) removes it like the move's own secondaries.

mod common;

use common::assert_exact_parity;

/// King's Rock never flinches a Shield Dust target (the engine rolled 10%); a Mold Breaker
/// user's King's Rock ignores Shield Dust and flinches 10% of the time.
#[test]
fn kings_rock_shield_dust_matches_showdown() {
    assert_exact_parity("bb-kings-rock-shield-dust");
}
