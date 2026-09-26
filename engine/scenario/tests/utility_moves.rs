//! Utility moves (Opus V unit 6): Aqua Ring, Covet / Thief, False Swipe / Hold Back, Power
//! Trick / Power Shift, Power Split / Guard Split, Bestow, Acupressure. Fixtures from Showdown's
//! exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Aqua Ring heals 1/16 at the residual; Big Root raises it.
#[test]
fn aqua_ring_heals_each_turn() {
    assert_exact_parity("aqua-ring");
}

/// Covet steals the target's item for an empty-handed user; Thief does nothing for a holder.
#[test]
fn covet_and_thief_steal_items() {
    assert_exact_parity("covet-thief");
}

/// False Swipe leaves its target at 1 HP; Hold Back deals normal damage above that.
#[test]
fn false_swipe_leaves_one_hp() {
    assert_exact_parity("false-swipe");
}

/// Power Trick swaps Attack and Defense; using Power Shift again swaps back.
#[test]
fn power_trick_swaps_attack_and_defense() {
    assert_exact_parity("power-trick");
}

/// Power Split and Guard Split average the stored stats of user and target.
#[test]
fn power_and_guard_split_average_stats() {
    assert_exact_parity("power-split");
}

/// Bestow gives the user's item to a target without one, and fails on a holder.
#[test]
fn bestow_gives_the_item() {
    assert_exact_parity("bestow");
}

/// Acupressure raises a uniformly drawn stat below +6 by 2.
#[test]
fn acupressure_raises_a_random_stat() {
    assert_exact_parity("acupressure");
}
