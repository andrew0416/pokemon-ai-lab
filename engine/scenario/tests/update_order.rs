//! The order of the `Update` pass (Opus W unit 7): Showdown's `eachEvent('Update')` sorts the
//! actives by `pokemon.speed` and shuffles ties; the only implemented handlers that depend on
//! each other are an item use and the ally's Symbiosis, so such a tie must branch.

mod common;

use common::assert_exact_parity;

/// A Symbiosis holder tied with its ally, both eating a Sitrus Berry at the turn-start Update:
/// whoever goes first decides whether the holder's berry is eaten or passed on.
#[test]
fn symbiosis_speed_tie_in_the_update_pass_matches_showdown() {
    assert_exact_parity("w-update-symbiosis-tie");
}
