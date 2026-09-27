//! Future Sight and Doom Desire (Opus T): the `futuremove` slot condition (F12 machinery), set
//! by the moves' `onTry` and hitting at the residual of the turn after next (order 3). Fixtures
//! from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// The use adds the slot condition and succeeds without a hit or Life Orb recoil; a second
/// future move at the same position fails.
#[test]
fn future_sight_use_sets_the_slot_condition() {
    assert_exact_parity("future-sight-use");
}

/// The hits two turns later: Future Sight's hit respects type immunity (Dark) yet costs Life Orb
/// recoil; Doom Desire uses its user's current boosts.
#[test]
fn future_moves_hit_two_turns_later() {
    assert_exact_parity("future-sight-hit");
}

/// The hit lands on whoever holds the position; a fainted holder is not hit.
#[test]
fn future_sight_hits_the_position() {
    assert_exact_parity("future-sight-switched-target");
}

/// A slot condition's residual runs for a fainted occupant: the Wish ends without a heal.
#[test]
fn wish_ends_on_a_fainted_holder() {
    assert_exact_parity("wish-fainted-holder");
}

/// A future move whose user has left the field: Showdown computes it with the benched user's
/// stored stats, its ability and item ignored (was refused; board R5a, `uu_switch_flags.rs`).
#[test]
fn future_sight_after_its_user_left() {
    assert_exact_parity("future-sight-user-left");
}
