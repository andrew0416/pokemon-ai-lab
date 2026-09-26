//! Moves that read sleep and paralysis (Opus V unit 1): Dream Eater (`onTryImmunity`: asleep
//! or Comatose; drain), Nightmare (a condition costing a sleeping holder 1/4 of its max HP at the
//! residual, gone when it wakes), Wake-Up Slap and Smelling Salts (double power against a
//! sleeping / paralyzed target, which they cure). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Dream Eater drains a sleeping target and fails on an awake one.
#[test]
fn dream_eater_needs_a_sleeping_target() {
    assert_exact_parity("dream-eater");
}

/// Comatose counts as sleep for Dream Eater.
#[test]
fn dream_eater_hits_comatose() {
    assert_exact_parity("dream-eater-comatose");
}

/// Nightmare costs a sleeping holder a quarter of its max HP and fails on an awake target.
#[test]
fn nightmare_hurts_a_sleeping_target() {
    assert_exact_parity("nightmare");
}

/// Nightmare ends when its holder wakes up.
#[test]
fn nightmare_ends_on_waking() {
    assert_exact_parity("nightmare-wake");
}

/// Wake-Up Slap doubles against a sleeping target, wakes it and so ends its Nightmare.
#[test]
fn wake_up_slap_wakes_and_ends_nightmare() {
    assert_exact_parity("wake-up-slap");
}

/// Smelling Salts doubles against a paralyzed target and cures it.
#[test]
fn smelling_salts_cures_paralysis() {
    assert_exact_parity("smelling-salts");
}
