//! Lock-On (Opus Z unit 4): the `lockon` volatile on its user (duration 2; the locked target is
//! its `effectState.source`); the user's moves against that target never miss
//! (`onSourceAccuracy`, OHKO moves included) and reach it while semi-invulnerable
//! (`onSourceInvulnerability`). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Lock-On puts `lockon` on its user, 1 turn left after the residual.
#[test]
fn lock_on_adds_its_volatile() {
    assert_exact_parity("z-lock-on");
}

/// Zap Cannon (50%) and Fissure (30%, OHKO) never miss the locked targets.
#[test]
fn locked_on_moves_never_miss() {
    assert_exact_parity("z-lock-on-hit");
}

/// Growl reaches the locked target underground (Dig).
#[test]
fn locked_on_moves_reach_a_semi_invulnerable_target() {
    assert_exact_parity("z-lock-on-dig");
}
