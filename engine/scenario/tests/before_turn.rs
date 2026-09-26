//! Moves with an action before the turn's moves (Opus T): Counter and Mirror Coat's
//! `beforeTurnMove` (order 5); Focus Punch, Beak Blast and Shell Trap's `priorityChargeMove`
//! (order 107). Each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

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

/// A damaging hit breaks Focus Punch (no PP, no `lastMove`); a status move does not.
#[test]
fn focus_punch_fails_after_a_damaging_hit() {
    assert_exact_parity("focus-punch");
}

/// Beak Blast's charge burns a contact attacker (not through Protective Pads) and ends after the
/// move.
#[test]
fn beak_blast_burns_contact_attackers() {
    assert_exact_parity("beak-blast");
}

/// A foe's physical hit sets off Shell Trap at once (order 3), before the slower foe moves.
#[test]
fn shell_trap_goes_next_after_a_physical_hit() {
    assert_exact_parity("shell-trap");
}

/// Without a physical hit Shell Trap stops with a `null` result.
#[test]
fn shell_trap_without_a_physical_hit_stops() {
    assert_exact_parity("shell-trap-no-hit");
}
