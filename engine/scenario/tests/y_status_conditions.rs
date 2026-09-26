//! Tidy Up, Syrup Bomb and Curse (Opus Y unit 2). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Tidy Up removes every substitute and both sides' hazards, then raises Atk and Spe.
#[test]
fn tidy_up_clears_substitutes_and_hazards() {
    assert_exact_parity("y-tidy-up");
}

/// Syrup Bomb's condition drops Speed at the residual from its source (Defiant reacts).
#[test]
fn syrup_bomb_drops_speed_at_the_residual() {
    assert_exact_parity("y-syrup-bomb");
}

/// Syrup Bomb ends at the Update once its source switched out.
#[test]
fn syrup_bomb_ends_when_its_source_leaves() {
    assert_exact_parity("y-syrup-bomb-source-leaves");
}

/// A Ghost's Curse goes through Protect and costs half the user's HP; a non-Ghost's boosts.
#[test]
fn curse_ghost_and_non_ghost() {
    assert_exact_parity("y-curse");
}

/// Curse fails on a target that is already cursed.
#[test]
fn curse_fails_on_a_cursed_target() {
    assert_exact_parity("y-curse-again");
}

/// A Ghost's Curse aimed at its ally curses a random foe.
#[test]
fn curse_at_an_ally_picks_a_random_foe() {
    assert_exact_parity("y-curse-ally");
}

/// Protean makes the non-Ghost user a Ghost before onTryHit: it curses itself.
#[test]
fn curse_from_protean_curses_the_user() {
    assert_exact_parity("y-curse-protean");
}
