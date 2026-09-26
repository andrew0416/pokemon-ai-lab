//! Parity of the Metronome item and Destiny Knot (Opus W unit 2) with Showdown.

mod common;

use common::assert_exact_parity;

/// A third consecutive successful use of the same move: 1.4x.
#[test]
fn metronome_counts_consecutive_uses() {
    assert_exact_parity("w-metronome-consecutive");
}

/// A different move in between resets the count.
#[test]
fn metronome_resets_on_another_move() {
    assert_exact_parity("w-metronome-reset");
}

/// A use blocked by Protect (result `null`) breaks the chain for the next turn.
#[test]
fn metronome_resets_after_a_protected_use() {
    assert_exact_parity("w-metronome-protect");
}

/// A charging move starts at 1 on its attacking turn (`twoturnmove`).
#[test]
fn metronome_counts_a_two_turn_move_from_one() {
    assert_exact_parity("w-metronome-two-turn");
}

/// Destiny Knot on the Pokémon Cute Charm attracts attracts the Cute Charm holder back.
#[test]
fn destiny_knot_attracts_back() {
    assert_exact_parity("w-destiny-knot");
}
