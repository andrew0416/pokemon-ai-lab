//! Moves with an action before the turn's moves (Opus T): Counter and Mirror Coat's
//! `beforeTurnMove` (order 5). Each scenario's exact outcome distribution must equal its oracle
//! fixture (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Mirror Coat records only the special hit and returns twice its damage to that foe's slot;
/// Counter without a physical hit fails.
#[test]
fn counter_and_mirror_coat_match_showdown_exactly() {
    assert_exact_parity("counter-mirror-coat");
}

/// Follow Me's redirection (priority 1) beats Counter's own (priority -1).
#[test]
fn counter_against_follow_me_matches_showdown_exactly() {
    assert_exact_parity("counter-follow-me");
}

/// An ally's hit and a special hit are not recorded: Counter fails.
#[test]
fn counter_ignores_an_allys_hit() {
    assert_exact_parity("counter-ally-hit");
}

/// Counter hits the recorded slot: the Pokémon U-turn brought in takes twice U-turn's damage.
#[test]
fn counter_hits_the_attackers_slot_after_u_turn() {
    assert_exact_parity("counter-uturn");
}
