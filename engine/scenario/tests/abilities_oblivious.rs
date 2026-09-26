//! Parity of Oblivious against moves that ignore it and of its Update cure (Opus X unit B5):
//! Oblivious's breakable `onTryHit` stops Attract, Captivate and Taunt; a Mold Breaker user's
//! move gets through, and Oblivious's `onUpdate` then removes `attract` and `taunt`.

mod common;

use common::assert_exact_parity;

/// Mold Breaker Taunt lands and the next Update removes it; Mold Breaker Captivate lowers the
/// Special Attack of both Oblivious foes.
#[test]
fn oblivious_against_mold_breaker_matches_showdown() {
    assert_exact_parity("x-oblivious-mold-breaker");
}

/// A taunted Pokémon that gets Oblivious (Skill Swap with its ally) loses the taunt at the
/// Update after the move.
#[test]
fn oblivious_gained_cures_taunt_matches_showdown() {
    assert_exact_parity("x-oblivious-skill-swap-taunt");
}
