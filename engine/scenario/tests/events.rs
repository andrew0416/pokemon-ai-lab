//! Boost events (WORKPLAN F16: `ChangeBoost`, `TryBoost`, `AfterEachBoost`, `AfterBoost`,
//! `ModifyBoost`) and the `DamagingHit` event (F15) against Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Intimidate at the start: Competitive and Defiant answer the drop with +2, Mirror Armor
/// bounces it back to Gyarados; then one Waterfall.
#[test]
fn intimidate_reactions_match_showdown_exactly() {
    assert_exact_parity("boost-events");
}

/// Contact into Rough Skin, and into Iron Barbs plus Rocky Helmet (order 1 then 2).
#[test]
fn damaging_hit_handlers_match_showdown_exactly() {
    assert_exact_parity("damaging-hit");
}

/// Contrary and Simple change the boosts a move applies (setup turn); Unaware ignores the
/// attacker's boosts when hit (decision turn).
#[test]
fn contrary_simple_and_unaware_match_showdown_exactly() {
    assert_exact_parity("unaware-contrary");
}
