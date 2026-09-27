//! The action queue as moves see it (WORKPLAN F8): Sucker Punch / Thunderclap read the
//! target's pending move, Quash and After You reorder it. Fixtures from Showdown's exact
//! enumeration.

mod common;

use common::assert_exact_parity;

/// Sucker Punch hits a target that still has an attack queued; Thunderclap fails against a
/// target that chose Protect.
#[test]
fn sucker_punch_and_thunderclap_match_showdown_exactly() {
    assert_exact_parity("sucker-punch");
}

/// Quash against a target that has already moved fails (`queue.willMove` finds no action):
/// Grassy Glide (+1 on Grassy Terrain, faster) goes before the Prankster Quash (board B37: the
/// scenario used to claim it moved Grassy Glide back). Quash that does send its target's move
/// to the back (order 201) is `ll_staged_serializer::quash_order_outlasts_the_resorts`.
#[test]
fn quash_matches_showdown_exactly() {
    assert_exact_parity("quash");
}

/// After You makes the target act next (order 3).
#[test]
fn after_you_matches_showdown_exactly() {
    assert_exact_parity("after-you");
}
