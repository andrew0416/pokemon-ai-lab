//! Parity of Damp (Opus Q unit 7: `onAnyTryMove`, `onAnyDamage`) with Showdown: each
//! scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Damp stops Explosion before its user faints; Mold Breaker's Self-Destruct ignores it.
#[test]
fn damp_explosion_and_mold_breaker_match_showdown() {
    assert_exact_parity("q-damp-explosion");
}

/// Damp stops Aftermath's damage.
#[test]
fn damp_aftermath_matches_showdown() {
    assert_exact_parity("q-damp-aftermath");
}
