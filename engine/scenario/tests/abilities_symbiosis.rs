//! Parity of Symbiosis (Opus S unit 7c: `onAllyAfterUseItem`) with Showdown: each scenario's
//! exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// The ally eats its Sitrus Berry, gets the holder's and eats it at the next Update.
#[test]
fn symbiosis_passes_its_item_after_a_berry_matches_showdown() {
    assert_exact_parity("s-symbiosis");
}

/// Eject Button sets the switch flag before the item is used: nothing is passed.
#[test]
fn symbiosis_skips_an_eject_button_switch_matches_showdown() {
    assert_exact_parity("s-symbiosis-eject-button");
}
