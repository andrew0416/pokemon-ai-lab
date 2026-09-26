//! Parity of `runMove`'s AfterMove for a called move (Opus X unit B4): after `useMove`, Showdown
//! runs AfterMove with `battle.activeMove`, the move Sleep Talk or Copycat called, not the
//! caller. Charge's `onAfterMove` then sees an Electric called move and ends the charge.

mod common;

use common::assert_exact_parity;

/// Sleep Talk calls Shock Wave: the charge ends.
#[test]
fn sleep_talk_calling_an_electric_move_ends_charge_matches_showdown() {
    assert_exact_parity("x-sleep-talk-charge");
}

/// Copycat calls Shock Wave (formerly refused): the charge ends.
#[test]
fn copycat_calling_an_electric_move_ends_charge_matches_showdown() {
    assert_exact_parity("x-copycat-charge");
}
