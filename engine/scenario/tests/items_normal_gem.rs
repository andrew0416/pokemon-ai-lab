//! Normal Gem (Opus AA items unit). Fixture from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Normal Gem is used at the first TryPrimaryHit of a Normal move and boosts it on every target;
/// a non-Normal move leaves it.
#[test]
fn normal_gem_boosts_a_normal_move_once() {
    assert_exact_parity("aa-normal-gem");
}

/// The `gem` condition (duration 1) is still up when an Emergency Exit stops the turn.
#[test]
fn normal_gem_condition_lasts_the_turn() {
    assert_exact_parity("aa-normal-gem-pause");
}
