//! Poison Heal, Slow Start and Truant (Opus U unit 2). Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Poison Heal heals 1/8 instead of taking poison or toxic damage (the toxic stage still rises);
/// at full HP it does nothing.
#[test]
fn poison_heal_heals_instead_of_poison_damage() {
    assert_exact_parity("u-poison-heal");
}

/// Slow Start halves Attack and Speed from the battle start.
#[test]
fn slow_start_halves_attack_and_speed() {
    assert_exact_parity("u-slow-start");
}

/// Slow Start's counter runs out after five residuals of turns its holder was active from the
/// start (setup turns); on turn 6 its Attack and Speed are whole again.
#[test]
fn slow_start_ends_after_five_turns() {
    assert_exact_parity("u-slow-start-ends");
}

/// Truant loafs every other move attempt; sleep stops the move first and leaves the truant
/// volatile.
#[test]
fn truant_loafs_and_sleep_comes_first() {
    assert_exact_parity("u-truant");
}

/// Truant restarting (Neutralizing Gas's End after Gastro Acid) on a holder that has moved this
/// turn gets the truant volatile.
#[test]
fn truant_restart_after_moving_loafs_next() {
    assert_exact_parity("u-truant-restart");
}

/// The recharge turn after Hyper Beam removes the truant volatile with mustrecharge.
#[test]
fn recharge_turn_clears_truant() {
    assert_exact_parity("u-truant-recharge");
}
