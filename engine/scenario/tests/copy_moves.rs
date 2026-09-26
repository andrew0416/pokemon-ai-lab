//! Moves that copy or answer earlier moves (Opus V unit 5): Copycat (the battle's last move,
//! `State::last_move`), Mirror Move (the target's last move with the `mirror` flag) and
//! Retaliate (double power after an ally fainted last turn). Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Copycat uses the battle's last move.
#[test]
fn copycat_copies_the_last_move() {
    assert_exact_parity("copycat");
}

/// Copycat fails after a `failcopycat` move (Protect).
#[test]
fn copycat_fails_after_protect() {
    assert_exact_parity("copycat-fail");
}

/// The battle's last move is kept across turns; before any move Copycat does nothing.
#[test]
fn copycat_copies_across_turns() {
    assert_exact_parity("copycat-turns");
}

/// A copied attack is aimed at a random foe.
#[test]
fn copycat_attack_picks_a_random_target() {
    assert_exact_parity("copycat-attack");
}

/// Mirror Move uses the target's last move at it, and fails without a `mirror` move.
#[test]
fn mirror_move_reflects_the_target_move() {
    assert_exact_parity("mirror-move");
}

/// Retaliate doubles the turn after an ally fainted.
#[test]
fn retaliate_doubles_after_a_faint() {
    assert_exact_parity("retaliate");
}
