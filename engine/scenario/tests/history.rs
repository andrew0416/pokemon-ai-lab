//! Damage history (WORKPLAN F13): `hurtThisTurn`, `attackedBy`, `timesAttacked`,
//! `moveLastTurnResult`, `newlySwitched` and the side's `totalFainted`, read by Assurance,
//! Avalanche, Rage Fist, Stomping Tantrum, Payback and Last Respects, and by Metal Burst's
//! scripted target and damage. Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Avalanche doubles after being hit by its target this turn; Payback stays at 50 against a
/// Pokémon that switched in this turn.
#[test]
fn avalanche_and_payback_match_showdown_exactly() {
    assert_exact_parity("history-avalanche-payback");
}

/// Stomping Tantrum doubles after last turn's failed Sucker Punch; Assurance doubles on a
/// target hurt earlier this turn.
#[test]
fn stomping_tantrum_and_assurance_match_showdown_exactly() {
    assert_exact_parity("history-tantrum-assurance");
}

/// Rage Fist counts the hits taken since switching in across turns; Metal Burst returns 1.5x
/// the last foe's damage to that foe's slot.
#[test]
fn rage_fist_and_metal_burst_match_showdown_exactly() {
    assert_exact_parity("history-ragefist-metalburst");
}

/// Last Respects counts the side's faints, including one earlier this turn.
#[test]
fn last_respects_matches_showdown_exactly() {
    assert_exact_parity("history-last-respects");
}
