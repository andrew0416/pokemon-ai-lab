//! Opportunist, Receiver and Power of Alchemy (Opus U unit 5). Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Opportunist copies the foes' raises at each move's AfterMove; Mirror Herb does not copy
/// Opportunist's boosts.
#[test]
fn opportunist_copies_foe_raises() {
    assert_exact_parity("u-opportunist");
}

/// A raise copied at the turn-start Update waits for the next move's AfterMove (the copies are
/// state, not a stage-local list).
#[test]
fn opportunist_copies_wait_for_the_next_trigger() {
    assert_exact_parity("u-opportunist-berry");
}

/// Receiver takes a fainted ally's Intimidate, which starts.
#[test]
fn receiver_takes_the_fainted_allys_ability() {
    assert_exact_parity("u-receiver");
}
