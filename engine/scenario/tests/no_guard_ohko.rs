//! Parity of OHKO accuracy with the `Accuracy` event (Opus BB unit B13): Showdown's
//! `hitStepAccuracy` gives an OHKO move its own accuracy (30, Sheer Cold 20 for a non-Ice user,
//! plus the level difference; a semi-invulnerable target keeps the move's accuracy and skips the
//! level and type checks) and then runs `runEvent('Accuracy')` like any other move, so No Guard
//! makes it hit.

mod common;

use common::assert_exact_parity;

/// The user's No Guard: Machamp's Fissure always KOs Snorlax (the engine rolled 30%).
#[test]
fn no_guard_user_fissure_matches_showdown() {
    assert_exact_parity("bb-no-guard-fissure");
}

/// The target's No Guard: Sheer Cold always KOs Golurk; Horn Drill on a Pokémon without it
/// stays 30%.
#[test]
fn no_guard_target_ohko_matches_showdown() {
    assert_exact_parity("bb-no-guard-target-ohko");
}

/// A semi-invulnerable target: No Guard's Sheer Cold KOs an underground Ice-type Mamoswine
/// (no Ice immunity check), while an Ice type above ground stays immune.
#[test]
fn ohko_semi_invulnerable_target_matches_showdown() {
    assert_exact_parity("bb-no-guard-ohko-dig");
}
