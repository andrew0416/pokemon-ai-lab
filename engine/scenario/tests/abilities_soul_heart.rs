//! Parity of Soul-Heart (Opus S unit 7a: `onAnyFaint`) with Showdown: each scenario's exact
//! outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// One boost per processed faint: +2 after a double knockout.
#[test]
fn soul_heart_after_a_double_knockout_matches_showdown() {
    assert_exact_parity("s-soul-heart");
}

/// The boost fails at the faint of the last foe (`foePokemonLeft`).
#[test]
fn soul_heart_at_the_last_foe_matches_showdown() {
    assert_exact_parity("s-soul-heart-last-foe");
}
