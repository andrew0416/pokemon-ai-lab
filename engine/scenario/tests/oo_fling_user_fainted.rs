//! Opus OO R16-fling-user-fainted: a reaction to Fling's hit (Innards Out) knocks its user out
//! before the hit loop's Update. Showdown's `eachEvent('Update')` still holds the 0-HP user (its
//! faint is processed after the loop), so Fling's condition `onUpdate` runs on it:
//! `setItem('')` fails (`!this.hp`) and the item stays held, `lastItem` is set, AfterUseItem runs
//! (an ally's Symbiosis gives its item and gets it back when `setItem` fails on the fainted
//! Pokémon). `rr-fling-innards-out` (`refusals_rr.rs`) is the plain case.

mod common;

use common::assert_exact_parity;

/// As `rr-fling-innards-out` with Oranguru (Symbiosis, Sitrus Berry) next to the Fling user:
/// Oranguru keeps its berry. Enumerated with `enumerate.cjs --staged` (full mode; the unstaged
/// Update Speed ties are 8.4 million branches).
#[test]
fn symbiosis_cannot_give_its_item_to_the_fainted_fling_user() {
    assert_exact_parity("oo-fling-innards-out-symbiosis");
}
