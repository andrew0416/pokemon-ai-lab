//! Opus OO R7-ally-switch-target: Showdown's `resolveAction` records each move action's
//! `originalTarget` (the Pokémon at its `targetLoc` when queued), and `getTarget` aims a tracking
//! move at it while it is active (`move.tracksTarget`: Snipe Shot; `hasAbility(['stalwart',
//! 'propellertail'])`: every move of the holder), wherever Ally Switch moved it; otherwise at the
//! position. Fixtures enumerated with `enumerate.cjs --staged` (full mode; the unstaged runs are
//! 0.6 and 52 million branches). `rr-ally-switch-snipe-shot` (`refusals_rr.rs`),
//! `ally-switch-snipe-shot` (`moves_extra.rs`) and `s-stalwart-ally-switch`
//! (`abilities_stalwart.rs`) are the other cases.

mod common;

use common::assert_exact_parity;

/// Archaludon (Stalwart) aims Flash Cannon at Starmie; Starmie's Ally Switch moves it to p2b
/// first, and Flash Cannon follows it instead of hitting Snorlax.
#[test]
fn stalwart_follows_the_target_ally_switch_moved() {
    assert_exact_parity("oo-ally-switch-stalwart");
}

/// Starmie switches out before Inteleon's Snipe Shot aimed at it: the original target is no
/// longer active, so the move aims at the position and hits the replacement.
#[test]
fn a_tracked_target_that_left_the_field_leaves_the_position() {
    assert_exact_parity("oo-snipe-shot-target-switched");
}
