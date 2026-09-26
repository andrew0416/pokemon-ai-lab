//! Parity of Safety Goggles held by a Pokémon that ignores its item (Opus X unit B6): Showdown's
//! `runStatusImmunity` goes through `runEvent('Immunity')`, which skips the item's `onImmunity`
//! while `ignoringItem()` holds (Klutz, Magic Room), so the holder takes sandstorm damage and is
//! drawn by Rage Powder.

mod common;

use common::assert_exact_parity;

/// Klutz: Rage Powder draws the holder's Seismic Toss, Spore puts it to sleep (the Goggles'
/// `onTryHit` is skipped too) and the sandstorm hurts it.
#[test]
fn klutz_ignores_safety_goggles_matches_showdown() {
    assert_exact_parity("x-safety-goggles-klutz");
}

/// Magic Room: both Goggles holders take sandstorm damage and Rage Powder draws one's move.
#[test]
fn magic_room_ignores_safety_goggles_matches_showdown() {
    assert_exact_parity("x-safety-goggles-magic-room");
}
