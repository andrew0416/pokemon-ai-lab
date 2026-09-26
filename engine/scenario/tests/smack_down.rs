//! Smack Down and Thousand Arrows (Opus T): the `smackdown` volatile grounds an airborne
//! Pokémon (and brings one down from Fly); Thousand Arrows ignores Ground immunity and hits an
//! airborne Flying type neutrally. Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// A Flying type hit by Smack Down is grounded for a later Ground move.
#[test]
fn smack_down_grounds_a_flying_type() {
    assert_exact_parity("smack-down");
}

/// Thousand Arrows hits a Flying type neutrally and a Levitate holder, grounding both.
#[test]
fn thousand_arrows_hits_airborne_targets() {
    assert_exact_parity("thousand-arrows");
}

/// Smack Down on a Pokémon in the air with Fly ends Fly and cancels its attack.
#[test]
fn smack_down_brings_down_a_flying_attacker() {
    assert_exact_parity("smack-down-fly");
}
