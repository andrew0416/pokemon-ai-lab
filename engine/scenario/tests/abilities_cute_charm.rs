//! Parity of Cute Charm and the Attract volatile (Opus S unit 7d) with Showdown: each
//! scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// 30% Attract on a contact move from the other gender; none from the same gender.
#[test]
fn cute_charm_matches_showdown() {
    assert_exact_parity("s-cute-charm");
}

/// An attracted Pokémon cannot move half the time.
#[test]
fn attract_before_move_matches_showdown() {
    assert_exact_parity("s-cute-charm-attracted");
}

/// Attract ends at the Update once its source switched out.
#[test]
fn attract_ends_when_its_source_leaves_matches_showdown() {
    assert_exact_parity("s-cute-charm-source-leaves");
}

/// Mental Herb cures Attract at the next Update.
#[test]
fn mental_herb_cures_attract_matches_showdown() {
    assert_exact_parity("s-cute-charm-mental-herb");
}
