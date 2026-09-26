//! Moves reading this turn's stat changes (Opus T): `statsLoweredThisTurn` (Lash Out) and
//! `statsRaisedThisTurn` (Burning Jealousy, Alluring Voice), hidden `SlotHistory` flags set by
//! `Battle::boost_by`. Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Intimidate at the start of the battle still counts on turn 1; Clear Body's blocked drop
/// does not.
#[test]
fn lash_out_doubles_after_intimidate_at_the_start() {
    assert_exact_parity("lash-out-intimidate");
}

/// A drop in an earlier turn is forgotten at its end; one this turn doubles Lash Out.
#[test]
fn lash_out_doubles_only_for_a_drop_this_turn() {
    assert_exact_parity("lash-out-turn");
}

/// Burning Jealousy burns a target that raised its stats this turn.
#[test]
fn burning_jealousy_burns_a_raised_target() {
    assert_exact_parity("burning-jealousy");
}

/// Alluring Voice confuses a target that raised its stats this turn.
#[test]
fn alluring_voice_confuses_a_raised_target() {
    assert_exact_parity("alluring-voice");
}
