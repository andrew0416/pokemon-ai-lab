//! Showdown's turn limit (board A1-t1, Opus AB): `endTurn` ties the battle once the turn counter
//! passes 1000 (`maybeTriggerEndlessBattleClause`, not a part of Endless Battle Clause). The
//! scenarios patch `battle.turn` (`patch.turn`). Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Turn 1000's end advances to 1001: a tie.
#[test]
fn the_battle_ties_past_turn_1000() {
    assert_exact_parity("ab-turn-limit-tie");
}

/// Turn 999's end advances to 1000: the battle goes on.
#[test]
fn the_battle_goes_on_at_turn_1000() {
    assert_exact_parity("ab-turn-limit-last-turn");
}
