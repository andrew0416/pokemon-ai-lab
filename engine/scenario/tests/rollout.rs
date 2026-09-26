//! Rollout and Ice Ball (Opus V unit 9c): their own condition locks the user in (aimed at the
//! location it chose, no PP), the power doubles per earlier hit and with Defense Curl, and a turn
//! without a hit ends it. Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// A first Rollout adds its condition; Defense Curl adds its volatile.
#[test]
fn rollout_starts_its_lock() {
    assert_exact_parity("rollout-start");
}

/// The locked Rollout doubles per hit and with Defense Curl, and costs no PP.
#[test]
fn rollout_locked_turn_doubles() {
    assert_exact_parity("rollout-locked");
}

/// A Rollout that does not hit ends the lock at the residual.
#[test]
fn rollout_blocked_ends_the_lock() {
    assert_exact_parity("rollout-protect");
}

/// Ice Ball works like Rollout with its own condition.
#[test]
fn ice_ball_starts_its_lock() {
    assert_exact_parity("ice-ball-start");
}
