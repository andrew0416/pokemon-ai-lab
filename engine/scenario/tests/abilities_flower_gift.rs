//! Parity of Flower Gift (Opus S unit 7b: the Cherrim formes and the allies' Atk / SpD in sun)
//! with Showdown: each scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Cherrim-Sunshine from the start's Drought; an ally's Atk and SpD 1.5x.
#[test]
fn flower_gift_in_sun_matches_showdown() {
    assert_exact_parity("s-flower-gift");
}

/// Rain replaces the sun: Cherrim reverts and the boost ends.
#[test]
fn flower_gift_when_the_sun_ends_matches_showdown() {
    assert_exact_parity("s-flower-gift-rain");
}

/// Cherrim switching in during sun changes forme in its onStart.
#[test]
fn flower_gift_on_switch_in_matches_showdown() {
    assert_exact_parity("s-flower-gift-switch-in");
}
