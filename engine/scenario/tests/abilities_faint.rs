//! Parity of the after-faint abilities (Opus Q unit 4: `onSourceAfterFaint` from the end of
//! `faintMessages`) with Showdown: each scenario's exact outcome distribution must equal its
//! oracle fixture.

mod common;

use common::assert_exact_parity;

/// One `faintMessages` call with two faints: Moxie raises Attack by 2.
#[test]
fn moxie_after_a_double_knockout_matches_showdown() {
    assert_exact_parity("q-moxie-spread");
}

/// The boost fails once the foe side has no Pokémon left.
#[test]
fn moxie_after_the_last_foe_faints_matches_showdown() {
    assert_exact_parity("q-moxie-last-foe");
}

#[test]
fn beast_boost_and_grim_neigh_match_showdown() {
    assert_exact_parity("q-beast-boost-grim-neigh");
}

/// Chilling Neigh; Eelevate's best-stat boost and its Levitate-like grounding.
#[test]
fn chilling_neigh_and_eelevate_match_showdown() {
    assert_exact_parity("q-chilling-neigh-eelevate");
}

/// As One (Spectrier): Unnerve's berry block and Grim Neigh's boost.
#[test]
fn as_one_matches_showdown() {
    assert_exact_parity("q-as-one");
}
