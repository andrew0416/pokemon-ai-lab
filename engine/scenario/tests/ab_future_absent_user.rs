//! A future move whose user left the field hits from the bench (board R5c, Opus AB, wave 16
//! lane L3): the engine seats the benched user in a position of its side whose occupant nothing
//! in the hit would ask for, and the target's handlers that act on the source skip it as
//! Showdown's inactive source (`spreadDamage`/`boost`: `!target.isActive`, Gulp Missile's
//! `!source.isActive`). Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Future Sight hits Clefable (Sitrus Berry) after Slowking switched out for Tyranitar
/// (Unnerve): the engine seats Slowking in Umbreon's position, so Tyranitar's Unnerve still
/// stops the berry (was refused).
#[test]
fn future_sight_from_the_bench_keeps_an_unnerve_occupant() {
    assert_exact_parity("uu-future-sight-absent-user-unnerve");
}

/// The same with an Unaware Clefable in the other position: Unaware's `onAnyModifyBoost` never
/// acts for a Pokémon neither using nor taking the move, so its position can hold the user.
#[test]
fn future_sight_from_the_bench_seats_the_user_over_unaware() {
    assert_exact_parity("ab-future-sight-absent-user-unaware");
}

/// Future Sight knocks out Pyukumuku: Innards Out does nothing to the inactive user.
#[test]
fn innards_out_spares_a_future_move_user_on_the_bench() {
    assert_exact_parity("ab-future-sight-absent-user-innards-out");
}

/// Future Sight hits Cramorant-Gulping: no spit (and no forme change) for an inactive user.
#[test]
fn gulp_missile_ignores_a_future_move_user_on_the_bench() {
    assert_exact_parity("ab-future-sight-absent-user-gulp-missile");
}

/// Future Sight from a fainted user (its position empty) hits Eldegoss: Cotton Down lowers the
/// Speed of every active Pokémon but the inactive user.
#[test]
fn cotton_down_skips_a_future_move_user_on_the_bench() {
    assert_exact_parity("ab-future-sight-absent-user-cotton-down");
}
