//! Dragon Darts (Opus Z unit 6): `smartTarget`. The move takes its target and that target's
//! adjacent ally (`getSmartTargets`) and each dart strikes one of them as a single-target hit;
//! a hit step dropping either (an immunity, a miss, Protect's NOT_FAIL) or a Follow Me / Rage
//! Powder / Lightning Rod / Storm Drain redirection turns `smartTarget` off, and the one target
//! left takes both darts. Champions' hit loop then loses the first target's `damage` entry: only
//! the second target gets the EmergencyExit check, and `move.hitTargets` is the second target
//! alone. Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// One dart for the target, one for its ally.
#[test]
fn dragon_darts_hits_both_smart_targets() {
    assert_exact_parity("z-dragon-darts");
}

/// The first target protects: the ally takes both darts.
#[test]
fn dragon_darts_into_protect() {
    assert_exact_parity("z-dragon-darts-protect");
}

/// The first target is immune: the ally takes both darts.
#[test]
fn dragon_darts_into_an_immune_target() {
    assert_exact_parity("z-dragon-darts-immune");
}

/// Follow Me takes the move: the redirector takes both darts.
#[test]
fn dragon_darts_into_follow_me() {
    assert_exact_parity("z-dragon-darts-follow-me");
}

/// The first dart knocks out its target; the second still goes to the ally.
#[test]
fn dragon_darts_after_the_first_target_faints() {
    assert_exact_parity("z-dragon-darts-faint");
}

/// Aimed at the user's ally, the ally is the only target.
#[test]
fn dragon_darts_at_an_ally() {
    assert_exact_parity("z-dragon-darts-ally");
}

/// AfterMoveSecondary covers every smart target, the one behind a substitute too (its Eject
/// Button acts).
#[test]
fn dragon_darts_after_move_secondary_reaches_a_substitute() {
    assert_exact_parity("z-dragon-darts-substitute");
}

/// Each smart target counts one attack (Rage Fist then has 100 power). The full distribution is
/// too large for a fixture: the oracle's `--mode extremes` (min and max rolls) against
/// `RollMode::Extremes`.
#[test]
fn dragon_darts_counts_one_attack_per_smart_target() {
    common::assert_extremes_parity("z-dragon-darts-rage-fist");
}

/// Only the second target's Emergency Exit is checked.
#[test]
fn dragon_darts_emergency_exit_checks_the_second_target_only() {
    assert_exact_parity("z-dragon-darts-emergency-exit");
}
