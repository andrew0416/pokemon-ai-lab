//! Parity of Infiltrator (Opus S unit 5: `move.infiltrates`) with Showdown: each scenario's
//! exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Through a Substitute and Reflect; Defog's evasion drop through the Substitute.
#[test]
fn infiltrator_through_substitute_and_screens_matches_showdown() {
    assert_exact_parity("s-infiltrator-sub-screens");
}

/// Through Safeguard (Toxic) and Mist (Charm) on the foes' side.
#[test]
fn infiltrator_through_safeguard_and_mist_matches_showdown() {
    assert_exact_parity("s-infiltrator-safeguard-mist");
}
